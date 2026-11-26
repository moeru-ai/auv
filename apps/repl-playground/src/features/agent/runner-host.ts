import type { DuplexCall } from '@auv-js/sdk'
import type { CodemodeTool } from '@exterioragentprotocol/sdk/client/codemode'

import type { SdkAccess } from '../../backend/types'
import type { recordCall } from '../../runtime/record-call'
import type { BridgedCallEnd, BridgedCallStart } from '../../runtime/sdk-bridge'

import { base64Decode, base64Encode } from '@bufbuild/protobuf/wire'
import { z } from 'zod'

import { bridgedError, recordUnaryCall } from '../../runtime/sdk-bridge'

type RecordStart = (start: Pick<BridgedCallStart, 'body' | 'method'>) => ReturnType<typeof recordCall>
interface Stream {
  call: DuplexCall
  end: BridgedCallEnd
  method: string
  recorded?: ReturnType<RecordStart>
  replies: AsyncIterator<Uint8Array>
}

const method = z.string().regex(/^\/auv\.api\.driver\.(?:\w+\.)+[A-Za-z]\w*\/[A-Za-z]\w*$/)
const streamId = z.number().int().positive()

export function createRunnerHost(access: SdkAccess | undefined, runSignal: AbortSignal, record: RecordStart) {
  const lifetime = new AbortController()
  const pending = new Set<Promise<unknown>>()
  const streams = new Map<number, Stream>()
  let lastId = 0
  const headers = new Headers(access?.target.headers)
  if (access) {
    headers.set('auv-runner-class', access.route.runnerClass)
    if (access.route.deviceId)
      headers.set('auv-device-id', access.route.deviceId)
    if (access.route.runId)
      headers.set('auv-run-id', access.route.runId)
  }

  function transport() {
    if (!access)
      throw new Error('AUV SDK calls need a Device or the mock desktop.')
    return access.target.transport
  }

  async function closeStream(id: number, error?: unknown) {
    const stream = streams.get(id)
    if (!stream)
      return
    streams.delete(id)
    try {
      await stream.call.close({})
    }
    finally {
      if (error)
        stream.end.error = bridgedError(error)
      const finish = await stream.recorded
      finish?.(stream.end)
    }
  }

  async function onStream<T>(id: number, work: (stream: Stream) => Promise<T>) {
    const stream = streams.get(id)
    if (!stream)
      throw new Error('This Runner stream has closed.')
    try {
      return await work(stream)
    }
    catch (error) {
      await closeStream(id, error)
      throw error
    }
  }

  function binding<S extends z.ZodType>(name: string, schema: S, run: (value: z.output<S>, signal: AbortSignal) => Promise<unknown>): CodemodeTool {
    return {
      async execute(value, context) {
        const signal = AbortSignal.any([context.signal, runSignal, lifetime.signal])
        signal.throwIfAborted()
        const result = run(schema.parse(value), signal)
        pending.add(result)
        try {
          return await result
        }
        finally {
          pending.delete(result)
        }
      },
      name,
    }
  }

  const globals = [
    binding('runnerUnary', z.strictObject({ body: z.string(), jsonBody: z.string(), method }), async (request, signal) => {
      const target = transport()
      const body = base64Decode(request.body)
      const finish = await record({ body, method: request.method })
      const end: BridgedCallEnd = { exchange: [] }
      try {
        const response = await recordUnaryCall(target, {
          body,
          decodeJson: () => new Uint8Array(),
          headers,
          http: { method: 'POST', path: `/apis/auv/runtime/v1/invoke${request.method}` },
          jsonBody: request.jsonBody,
          method: request.method,
          signal,
        }, end)
        return end.json === undefined ? { body: base64Encode(response) } : { json: end.json }
      }
      finally {
        finish?.(end)
      }
    }),
    binding('runnerOpen', method, async (method, signal) => {
      const call = await transport().duplex({ headers, method, signal })
      if (signal.aborted) {
        await call.close({})
        signal.throwIfAborted()
      }
      const id = ++lastId
      streams.set(id, { call, end: { exchange: [] }, method, replies: call.responses[Symbol.asyncIterator]() })
      return id
    }),
    binding('runnerSend', z.strictObject({ body: z.string(), id: streamId }), ({ body, id }) => onStream(id, async (stream) => {
      const request = base64Decode(body)
      stream.recorded ??= record({ body: request, method: stream.method })
      await stream.recorded
      stream.end.exchange.push({ request })
      await stream.call.send(request)
    })),
    binding('runnerNext', streamId, id => onStream(id, async (stream) => {
      const reply = await stream.replies.next()
      if (reply.done)
        return null
      stream.end.exchange.push({ response: reply.value })
      return base64Encode(reply.value)
    })),
    binding('runnerHalfClose', streamId, id => onStream(id, stream => stream.call.halfClose())),
    binding('runnerClose', streamId, id => closeStream(id)),
    binding('runnerWait', z.number().nonnegative(), (delay, signal) => new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        signal.removeEventListener('abort', abort)
        resolve()
      }, delay)
      function abort() {
        clearTimeout(timer)
        reject(signal.reason)
      }
      signal.addEventListener('abort', abort, { once: true })
    })),
  ]
  return {
    async close() {
      lifetime.abort()
      await Promise.allSettled([...streams.keys()].map(id => closeStream(id, lifetime.signal.reason)))
      await Promise.allSettled(pending)
    },
    globals,
  }
}
