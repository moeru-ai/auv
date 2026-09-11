import type { Client as DaemonApiClient } from '@auv-js/api-client'
import type { DescMessage, JsonValue, MessageInitShape, MessageShape } from '@bufbuild/protobuf'

import type {
  DuplexCall,
  HttpBinding,
  OperationOptions,
  Transport,
} from './types'

import { create, fromBinary, fromJson, fromJsonString, toBinary, toJson, toJsonString } from '@bufbuild/protobuf'

import { abortError, auvHttpError, AuvProtocolError, AuvTransportError, throwIfAborted } from './errors'

/** Inherited operation placement carried by a connection. */
export interface ConnectionRoute {
  deviceId?: string
  runId?: string
}

export interface ConnectOptions extends OperationOptions {
  credential?: DeviceCredential
  endpoint?: string | URL
  local?: boolean
  route?: ConnectionRoute
  transport?: 'grpc' | 'http' | 'npipe' | 'unix' | Transport
}

/** Opaque long-lived credential issued by successful Device pairing. */
export type DeviceCredential = string

export interface RestCallOptions {
  readonly body: unknown
  readonly client: DaemonApiClient
  readonly headers: Record<string, string>
  readonly meta: { signal?: AbortSignal }
  readonly signal?: AbortSignal
}

export interface RestCallResult {
  readonly data?: unknown
  readonly error?: unknown
  readonly response?: Response
}

export interface RpcDefinition<I extends DescMessage, O extends DescMessage> {
  readonly http?: (input: MessageShape<I>) => HttpBinding
  readonly input: I
  readonly method: string
  readonly output: O
  readonly rest?: (options: RestCallOptions) => Promise<RestCallResult>
}

export interface TypedDuplexCall<I extends DescMessage, O extends DescMessage> {
  close: (options?: OperationOptions) => Promise<void>
  halfClose: () => Promise<void>
  readonly responses: AsyncIterable<MessageShape<O>>
  send: (input: MessageInitShape<I>) => Promise<void>
}

export interface UnaryOptions extends OperationOptions {
  headers?: HeadersInit
}

/** A connected AUV protocol transport with optional Device authentication. */
export class AuvConnection {
  /** Whether operation placement is constrained to the daemon's local Device. */
  readonly local: boolean
  /** Default Device and Run placement inherited by routed operations. */
  readonly route: Readonly<ConnectionRoute>
  readonly #credential?: DeviceCredential
  readonly #transport: Transport

  constructor(
    transport: Transport,
    credential?: DeviceCredential,
    local = false,
    route: ConnectionRoute = {},
  ) {
    this.#transport = transport
    this.#credential = credential
    this.local = local
    this.route = Object.freeze({ ...route })
  }

  /** Closes the underlying transport. */
  async close(options: OperationOptions = {}): Promise<void> {
    throwIfAborted(options.signal)
    await this.#transport.close(options)
  }

  async duplex<I extends DescMessage, O extends DescMessage>(
    definition: RpcDefinition<I, O>,
    options: UnaryOptions = {},
  ): Promise<TypedDuplexCall<I, O>> {
    throwIfAborted(options.signal)
    const headers = new Headers(options.headers)
    if (this.#credential !== undefined) {
      headers.set('authorization', `Bearer ${this.#credential}`)
    }
    const stream: DuplexCall = await this.#transport.duplex({
      headers,
      method: definition.method,
      signal: options.signal,
    })
    return {
      async close(closeOptions = {}) {
        throwIfAborted(closeOptions.signal)
        await stream.close(closeOptions)
      },
      halfClose() {
        return stream.halfClose()
      },
      responses: decodeResponses(definition.output, stream.responses),
      send(input) {
        return stream.send(toBinary(definition.input, create(definition.input, input)))
      },
    }
  }

  async unary<I extends DescMessage, O extends DescMessage>(
    definition: RpcDefinition<I, O>,
    input: MessageInitShape<I>,
    options: UnaryOptions = {},
  ): Promise<MessageShape<O>> {
    throwIfAborted(options.signal)
    const request = create(definition.input, input)
    const headers = new Headers(options.headers)
    if (this.#credential !== undefined) {
      headers.set('authorization', `Bearer ${this.#credential}`)
    }
    if (definition.rest !== undefined && this.#transport.daemonApi !== undefined) {
      const result = await definition.rest({
        body: toJson(definition.input, request),
        client: this.#transport.daemonApi,
        headers: Object.fromEntries(headers),
        meta: { signal: options.signal },
        signal: options.signal,
      })
      if (result.error !== undefined) {
        if (options.signal?.aborted) {
          throw abortError(options.signal)
        }
        if (result.response === undefined) {
          throw new AuvTransportError('AUV HTTP transport failed', result.error)
        }
        throw auvHttpError(result.response, result.error)
      }
      if (result.data === undefined) {
        throw new AuvProtocolError(`${definition.method} returned an empty REST response`)
      }
      try {
        return fromJson(definition.output, result.data as JsonValue)
      }
      catch (error) {
        throw new AuvProtocolError(`${definition.method} returned invalid ProtoJSON`, error)
      }
    }
    const response = await this.#transport.unary({
      body: toBinary(definition.input, request),
      decodeJson: body => toBinary(definition.output, fromJsonString(definition.output, body)),
      headers,
      http: definition.http?.(request),
      jsonBody: toJsonString(definition.input, request),
      method: definition.method,
      signal: options.signal,
    })
    try {
      return fromBinary(definition.output, response)
    }
    catch (error) {
      throw new AuvProtocolError(`${definition.method} returned invalid Protobuf`, error)
    }
  }
}

/** Opens a connection over a transport selected by an environment entrypoint. */
export async function connectTransport(
  transport: Transport,
  options: ConnectOptions = {},
): Promise<AuvConnection> {
  await transport.connect({ signal: options.signal })
  return new AuvConnection(transport, options.credential, options.local, options.route)
}

async function* decodeResponses<O extends DescMessage>(
  schema: O,
  responses: AsyncIterable<Uint8Array>,
): AsyncIterable<MessageShape<O>> {
  for await (const response of responses) {
    try {
      yield fromBinary(schema, response)
    }
    catch (error) {
      throw new AuvProtocolError('AUV stream returned invalid Protobuf', error)
    }
  }
}
