import type { Client as DaemonApiClient } from '@auv-js/api-client'

/** An established bidirectional RPC. */
export interface DuplexCall {
  close: (options: OperationOptions) => Promise<void> | void
  halfClose: () => Promise<void>
  readonly responses: AsyncIterable<Uint8Array>
  send: (body: Uint8Array) => Promise<void>
}

/** One routed streaming RPC passed from the SDK to a transport. */
export interface DuplexCallOptions {
  readonly headers: Headers
  readonly method: string
  readonly signal?: AbortSignal
}

/** HTTP projection for one canonical unary RPC. */
export interface HttpBinding {
  encoding?: 'json' | 'protobuf'
  method: 'DELETE' | 'GET' | 'POST'
  path: string
}

/** Options shared by asynchronous SDK operations. */
export interface OperationOptions {
  signal?: AbortSignal
}

/** Protocol adapter used by an AUV connection. */
export interface Transport {
  close: (options: OperationOptions) => Promise<void> | void
  connect: (options: OperationOptions) => Promise<void>
  /** Generated daemon REST client exposed only by the HTTP transport. */
  readonly daemonApi?: DaemonApiClient
  duplex: (call: DuplexCallOptions) => Promise<DuplexCall>
  unary: (call: UnaryCall) => Promise<Uint8Array>
}

/** One encoded unary RPC passed from the SDK to a transport. */
export interface UnaryCall {
  readonly body: Uint8Array
  readonly decodeJson: (body: string) => Uint8Array
  readonly headers: Headers
  readonly http?: HttpBinding
  readonly jsonBody: string
  readonly method: string
  readonly signal?: AbortSignal
}
