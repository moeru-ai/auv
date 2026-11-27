import type { DuplexCall, RunnerRouteOptions, Transport } from '@auv-js/sdk'
import type { BridgedCallEnd, RpcCallStart } from '@auv-js/sdk/transport/recording'

import { AuvConnection, createRunnerClient, runnerHttpBinding, runnerRouteHeaders } from '@auv-js/sdk'
import { bridgedError, recordDuplexCall, recordUnaryCall } from '@auv-js/sdk/transport/recording'
import { base64Decode, base64Encode } from '@bufbuild/protobuf/wire'

type RecordCall = (start: Required<RpcCallStart>) => Promise<((end: BridgedCallEnd) => void) | undefined>

type Reply = null | number | string | UnaryReply

interface RunnerAccess {
  route: RunnerRouteOptions
  target: { headers?: HeadersInit, transport: Transport }
}

interface Stream {
  call: DuplexCall
  end: BridgedCallEnd
  method: string
  reader: AsyncIterator<Uint8Array>
  recorded?: ReturnType<RecordCall>
}
type UnaryReply = { body: string } | { json: string }
declare function runnerCall(request: ['unary', string, string, string]): Promise<UnaryReply>
declare function runnerCall(request: ['open', string]): Promise<number>
declare function runnerCall(request: ['next', number]): Promise<null | string>
declare function runnerCall(request: ['close' | 'halfClose' | 'wait', number] | ['send', number, string]): Promise<null>

export function installSandboxTimers(): void {
  const timers = new Set<number>()
  let nextTimer = 0
  Object.assign(globalThis, {
    clearTimeout: (id: number) => timers.delete(id),
    setTimeout(callback: () => void, delay = 0) {
      const id = ++nextTimer
      timers.add(id)
      void runnerCall(['wait', delay]).then(() => {
        if (timers.delete(id))
          callback()
      }, () => timers.delete(id))
      return id
    },
  })
}

export function createSandboxRunner() {
  return createRunnerClient(new AuvConnection({
    close() {},
    async connect() {},
    async duplex({ method }) {
      const id = await runnerCall(['open', method])
      async function close() {
        await runnerCall(['close', id])
      }
      return {
        close,
        async halfClose() { await runnerCall(['halfClose', id]) },
        responses: (async function* () {
          try {
            for (let body = await runnerCall(['next', id]); body !== null; body = await runnerCall(['next', id]))
              yield base64Decode(body)
          }
          finally {
            await close()
          }
        })(),
        async send(body) { await runnerCall(['send', id, base64Encode(body)]) },
      }
    },
    async unary({ body, decodeJson, jsonBody, method }) {
      const reply = await runnerCall(['unary', method, base64Encode(body), jsonBody])
      return 'json' in reply ? decodeJson(reply.json) : base64Decode(reply.body)
    },
  }), { runnerClass: 'auv.core.local' })
}

export function createSandboxRunnerHost(access: RunnerAccess | undefined, runSignal: AbortSignal, record: RecordCall) {
  const lifetime = new AbortController()
  const pending = new Set<Promise<unknown>>()
  const streams = new Map<number, Stream>()
  const headers = access && runnerRouteHeaders(access.route, access.target.headers)
  let nextId = 0

  async function closeStream(id: number, cause?: unknown) {
    const stream = streams.get(id)
    if (!stream)
      return
    streams.delete(id)
    try {
      await stream.call.close({})
      await stream.reader.return?.()
    }
    finally {
      if (cause !== undefined)
        stream.end.error ??= bridgedError(cause)
      const finish = await stream.recorded
      finish?.(stream.end)
    }
  }

  async function dispatch(request: unknown, signal: AbortSignal): Promise<Reply> {
    if (!Array.isArray(request))
      throw new TypeError('Invalid Runner request.')
    const [kind, input, payload, jsonBody]: unknown[] = request
    if (kind === 'wait') {
      if (typeof input !== 'number' || !Number.isFinite(input) || input < 0)
        throw new TypeError('Invalid timer delay.')
      return wait(input, signal)
    }
    if (kind === 'unary' || kind === 'open') {
      if (typeof input !== 'string' || !/^\/auv\.api\.driver\.(?:\w+\.)+[A-Za-z]\w*\/[A-Za-z]\w*$/.test(input))
        throw new TypeError('Invalid Runner method.')
      if (!access || !headers)
        throw new Error('AUV SDK calls need a Device or the mock desktop.')
      const { transport } = access.target
      const options = { headers, method: input, signal }
      const end: BridgedCallEnd = { exchange: [] }
      if (kind === 'open') {
        const call = recordDuplexCall(await transport.duplex(options), end)
        if (signal.aborted) {
          await call.close({})
          signal.throwIfAborted()
        }
        const id = ++nextId
        streams.set(id, { call, end, method: input, reader: call.responses[Symbol.asyncIterator]() })
        return id
      }
      if (typeof payload !== 'string' || typeof jsonBody !== 'string')
        throw new TypeError('Invalid Runner request body.')

      const body = base64Decode(payload)
      const finish = await record({ body, method: input })
      const [, service, operation] = input.split('/')
      try {
        const response = await recordUnaryCall(transport, {
          ...options,
          body,
          decodeJson: () => new Uint8Array(),
          http: runnerHttpBinding(service, operation),
          jsonBody,
        }, end)
        return end.json === undefined ? { body: base64Encode(response) } : { json: end.json }
      }
      finally {
        finish?.(end)
      }
    }
    if (typeof input !== 'number' || !Number.isSafeInteger(input))
      throw new TypeError('Invalid Runner stream ID.')
    if (kind === 'close') {
      await closeStream(input)
      return null
    }
    const stream = streams.get(input)
    if (!stream)
      throw new Error('The Runner stream is closed.')
    try {
      switch (kind) {
        case 'halfClose':
          await stream.call.halfClose()
          return null
        case 'next': {
          const next = await stream.reader.next()
          return next.done ? null : base64Encode(next.value)
        }
        case 'send': {
          if (typeof payload !== 'string')
            throw new TypeError('Invalid Runner stream message.')
          const body = base64Decode(payload)
          stream.recorded ??= record({ body, method: stream.method })
          await stream.recorded
          signal.throwIfAborted()
          await stream.call.send(body)
          return null
        }
      }
    }
    catch (error) {
      await closeStream(input, error)
      throw error
    }
    throw new TypeError('Invalid Runner stream operation.')
  }

  return {
    async close() {
      lifetime.abort()
      const cause = runSignal.aborted ? runSignal.reason : lifetime.signal.reason
      await Promise.allSettled([...Array.from(streams.keys(), id => closeStream(id, cause)), ...pending])
    },
    globals: [{
      async execute(request: unknown, context: { signal: AbortSignal }) {
        const signal = AbortSignal.any([context.signal, runSignal, lifetime.signal])
        signal.throwIfAborted()
        const result = dispatch(request, signal)
        pending.add(result)
        return result.finally(() => pending.delete(result))
      },
      name: 'runnerCall',
    }],
  }
}

function wait(delay: number, signal: AbortSignal): Promise<null> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', abort)
      resolve(null)
    }, delay)
    function abort() {
      clearTimeout(timer)
      reject(signal.reason)
    }
    signal.addEventListener('abort', abort, { once: true })
  })
}
