import type { DuplexCall, HttpBinding, Transport } from '@auv-js/sdk'
import type { BridgedCallEnd, BridgedError, RpcCallStart } from '@auv-js/sdk/transport/recording'
import type { EventContext } from '@moeru/eventa'

import { AuvTransportError } from '@auv-js/sdk'
import { bridgedError, recordDuplexCall, recordUnaryCall, remoteError } from '@auv-js/sdk/transport/recording'
import { defineInvoke, defineInvokeEventa, defineInvokeHandler, defineStreamInvoke, defineStreamInvokeHandler } from '@moeru/eventa'

// Carries encoded SDK RPCs between the execution worker and the host, so
// scripts can use `@auv-js/sdk` directly while the host owns the credential,
// the backend choice and call recording. See
// `docs/ai/references/inspect/2026-10-08-playground-sdk-transport-design.md`.

/** Message posted to the worker with the port this bridge runs on. */
export const SDK_PORT_MESSAGE = 'auv:sdk-port'

/** Where an encoded RPC came from in the script. */
interface BridgedCall {
  headers: [string, string][]
  /** gRPC path, `/package.Service/Method`. */
  method: string
  stepId: null | number
}

type DuplexFrame = { body: Uint8Array } | { open: BridgedCall }

type DuplexReply = { body: Uint8Array } | { error: BridgedError }
type UnaryReply = { body: Uint8Array } | { error: BridgedError } | { json: string }
interface UnaryRequest extends BridgedCall {
  body: Uint8Array
  http?: HttpBinding
  jsonBody: string
}

const unaryRpc = defineInvokeEventa<UnaryReply, UnaryRequest>('auv:sdk:unary')
const duplexRpc = defineInvokeEventa<DuplexReply, ReadableStream<DuplexFrame>>('auv:sdk:duplex')

/** One forwarded RPC as the host sees it, for recording. */
export type BridgedCallStart = Pick<BridgedCall, 'stepId'> & RpcCallStart

export interface BridgeHost {
  /** Called when a forwarded RPC starts; the returned function when it ends. */
  record?: (call: BridgedCallStart) => ((end: BridgedCallEnd) => void) | Promise<((end: BridgedCallEnd) => void) | undefined>
  /** The active backend's transport, or why the backend cannot serve SDK calls. */
  target: () => BridgeTarget | string
}

/** The transport the host forwards to, and the headers it adds (the credential). */
export interface BridgeTarget {
  headers?: Record<string, string>
  signal?: AbortSignal
  transport: Transport
}

type AnyContext = EventContext<any, any>

/**
 * The SDK transport inside the worker. `currentStep` attributes each call to
 * the statement that made it.
 */
export function createBridgeTransport(context: AnyContext, currentStep: () => null | number): Transport {
  const unary = defineInvoke(context, unaryRpc)
  const duplex = defineStreamInvoke(context, duplexRpc)
  return {
    close() {},
    async connect() {},
    async duplex(call): Promise<DuplexCall> {
      let frames!: ReadableStreamDefaultController<DuplexFrame>
      let open = true
      const requests = new ReadableStream<DuplexFrame>({
        start: (controller) => {
          frames = controller
        },
      })
      frames.enqueue({ open: { headers: [...call.headers], method: call.method, stepId: currentStep() } })
      const abort = new AbortController()
      call.signal?.addEventListener('abort', () => abort.abort(call.signal?.reason), { once: true })
      const replies = duplex(requests, { signal: abort.signal })
      const end = () => {
        if (open) {
          open = false
          frames.close()
        }
      }
      return {
        close: () => {
          end()
          abort.abort()
        },
        halfClose: async () => end(),
        responses: (async function* () {
          for await (const reply of replies) {
            if ('error' in reply)
              throw remoteError(reply.error)
            yield reply.body
          }
        })(),
        send: async (body) => {
          if (open)
            frames.enqueue({ body })
        },
      }
    },
    async unary(call) {
      const reply = await unary({
        body: call.body,
        headers: [...call.headers],
        http: call.http,
        jsonBody: call.jsonBody,
        method: call.method,
        stepId: currentStep(),
      }, { signal: call.signal })
      if ('error' in reply)
        throw remoteError(reply.error)
      // JSON-encoded HTTP bindings are decoded here, where the SDK's schema is.
      return 'json' in reply ? call.decodeJson(reply.json) : reply.body
    },
  }
}

/** Serves bridged RPCs on the host by forwarding them to the active backend. */
export function serveBridge(context: AnyContext, host: BridgeHost): void {
  defineInvokeHandler(context, unaryRpc, async (request, options): Promise<UnaryReply> => {
    const finish = await host.record?.({ body: request.body, method: request.method, stepId: request.stepId })
    const end: BridgedCallEnd = { exchange: [] }
    try {
      const { transport, ...call } = forwardedCall(host, request, options?.abortController?.signal)
      // NOTICE(bridge-decode-json): the HTTP transport decodes JSON bindings
      // through `UnaryCall.decodeJson`, which only the worker's SDK can build.
      // Capture the response text here and decode it back in the worker.
      const body = await recordUnaryCall(transport, {
        ...call,
        body: request.body,
        decodeJson: () => new Uint8Array(),
        http: request.http,
        jsonBody: request.jsonBody,
      }, end)
      finish?.(end)
      return end.json === undefined ? { body } : { json: end.json }
    }
    catch (error) {
      const failure = end.error ?? bridgedError(error)
      const exchange = end.error ? end.exchange : [{ request: request.body }]
      finish?.({ error: failure, exchange })
      return { error: failure }
    }
  })

  defineStreamInvokeHandler(context, duplexRpc, async function* (incoming, options) {
    const frames = iterate(incoming)
    const first = await frames.next()
    if (first.done || !('open' in first.value)) {
      yield { error: { message: 'bridged stream did not start with its call', name: 'AuvTransportError' } }
      return
    }
    const opened = first.value.open
    const end: BridgedCallEnd = { exchange: [] }
    let finish: ((end: BridgedCallEnd) => void) | undefined
    let call: DuplexCall | undefined
    try {
      const { transport, ...optionsForCall } = forwardedCall(host, opened, options?.abortController?.signal)
      const stream = recordDuplexCall(await transport.duplex(optionsForCall), end)
      call = stream
      let started = false
      // Forward requests concurrently with responses: scroll-until answers
      // updates with decisions on the same stream.
      void (async () => {
        for (let next = await frames.next(); !next.done; next = await frames.next()) {
          if ('body' in next.value) {
            if (!started) {
              started = true
              finish = await host.record?.({ body: next.value.body, method: opened.method, stepId: opened.stepId })
            }
            await stream.send(next.value.body)
          }
        }
        await stream.halfClose()
      })().catch(() => {})
      for await (const body of stream.responses)
        yield { body }
      finish?.(end)
    }
    catch (error) {
      const failure = end.error ??= bridgedError(error)
      finish?.(end)
      yield { error: failure }
    }
    finally {
      await call?.close({})
    }
  })
}

function forwardedCall(host: BridgeHost, call: BridgedCall, invocationSignal: AbortSignal | undefined) {
  const target = host.target()
  if (typeof target === 'string')
    throw new AuvTransportError(target)
  const headers = new Headers(call.headers)
  for (const [name, value] of Object.entries(target.headers ?? {}))
    headers.set(name, value)
  const { signal, transport } = target
  return {
    headers,
    method: call.method,
    signal: signal && invocationSignal ? AbortSignal.any([signal, invocationSignal]) : signal ?? invocationSignal,
    transport,
  }
}

/** eventa hands a stream handler a `ReadableStream`; read it without relying on its async iterator. */
function iterate<T>(incoming: AsyncIterable<T> | ReadableStream<T>): AsyncIterator<T> {
  return incoming instanceof ReadableStream ? iterateStream(incoming) : incoming[Symbol.asyncIterator]()
}

async function* iterateStream<T>(stream: ReadableStream<T>): AsyncGenerator<T> {
  const reader = stream.getReader()
  try {
    for (let next = await reader.read(); !next.done; next = await reader.read())
      yield next.value
  }
  finally {
    reader.releaseLock()
  }
}
