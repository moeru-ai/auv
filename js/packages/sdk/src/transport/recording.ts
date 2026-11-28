import type { DuplexCall, Transport, UnaryCall } from './types'

import { AuvRemoteError, AuvRpcError, AuvTransportError } from './errors'

/** How a forwarded RPC ended: its encoded messages, JSON, or an error. */
export interface BridgedCallEnd {
  error?: BridgedError
  /**
   * Request and response messages in the order they crossed. Replay needs the
   * order: scroll-until answers each update with a decision.
   */
  exchange: Exchanged[]
  /** A JSON-encoded HTTP binding's response text, in place of a response message. */
  json?: string
}

/** A remote error, rebuilt in the worker as the same SDK error class. */
export interface BridgedError {
  message: string
  name: string
  rpcCode?: number
}

export type Exchanged = { request: Uint8Array } | { response: Uint8Array }

export interface RpcCallStart {
  /** Encoded request; the first message of a stream. */
  body?: Uint8Array
  method: string
}

/** An error as it crosses the bridge or is recorded for replay. */
export function bridgedError(error: unknown): BridgedError {
  if (error instanceof AuvRpcError)
    return { message: error.message, name: error.name, rpcCode: error.rpcCode }
  if (error instanceof Error)
    return { message: error.message, name: error.name }
  return { message: String(error), name: 'Error' }
}

export function recordDuplexCall(stream: DuplexCall, entry: BridgedCallEnd): DuplexCall {
  return {
    close: options => stream.close(options),
    halfClose: () => stream.halfClose(),
    responses: (async function* () {
      try {
        for await (const body of stream.responses) {
          entry.exchange.push({ response: body })
          yield body
        }
      }
      catch (error) {
        entry.error = bridgedError(error)
        throw error
      }
    })(),
    async send(body) {
      entry.exchange.push({ request: body })
      await stream.send(body)
    },
  }
}

export async function recordUnaryCall(transport: Transport, call: UnaryCall, entry: BridgedCallEnd): Promise<Uint8Array> {
  entry.exchange.push({ request: call.body })
  try {
    call.signal?.throwIfAborted()
    const body = await transport.unary({
      ...call,
      decodeJson: (text) => {
        entry.json = text
        return call.decodeJson(text)
      },
    })
    if (entry.json === undefined)
      entry.exchange.push({ response: body })
    return body
  }
  catch (error) {
    entry.error = bridgedError(error)
    throw error
  }
}

/** Rebuilds a bridged error as the SDK error class it was. */
export function remoteError(error: BridgedError): Error {
  const rebuilt = error.rpcCode !== undefined
    ? new AuvRpcError(error.rpcCode, error.message)
    : error.name === 'AuvTransportError'
      ? new AuvTransportError(error.message)
      : new AuvRemoteError(error.message)
  rebuilt.name = error.name
  return rebuilt
}
