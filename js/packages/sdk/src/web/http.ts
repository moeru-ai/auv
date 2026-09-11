import type { DuplexCall, DuplexCallOptions, OperationOptions, Transport, UnaryCall } from '../transport/types'

import { createClient as createDaemonApiClient } from '@auv-js/api-client'
import { create, fromBinary, toBinary } from '@bufbuild/protobuf'

import { ClientMessageSchema, ServerMessageSchema } from '../gen/auv/api/transport/websocket/v1/websocket_pb'
import { AsyncQueue } from '../transport/async-queue'
import {
  abortError,
  AuvConfigurationError,
  auvHttpError,
  AuvHttpError,
  AuvProtocolError,
  AuvRemoteError,
  AuvRpcError,
  AuvTransportError,
  throwIfAborted,
} from '../transport/errors'

export interface HttpTransportOptions {
  endpoint?: string | URL
  fetch?: typeof globalThis.fetch
  webSocket?: typeof WebSocket
}

interface WebSocketRoute {
  method: string
  runnerClass: string
  service: string
}

export { AuvHttpError }

/** gRPC status returned by an AUV WebSocket operation. */
export class AuvWebSocketError extends AuvRpcError {
  constructor(readonly grpcStatus: number, message: string) {
    super(grpcStatus, message)
    this.name = 'AuvWebSocketError'
  }
}

/** Creates the browser-safe Protobuf-over-HTTP AUV transport. */
export function createHttpTransport(options: HttpTransportOptions = {}): Transport {
  const endpoint = new URL(options.endpoint ?? 'http://127.0.0.1:9847')

  const fetch = options.fetch ?? globalThis.fetch
  const WebSocketConstructor = options.webSocket ?? globalThis.WebSocket

  const sockets = new Set<WebSocket>()
  const requestSignals = new WeakMap<Request, AbortSignal>()

  const daemonApi = createDaemonApiClient({
    baseUrl: endpoint.href.replace(/\/$/u, ''),
    fetch: async (input, init) => {
      const request = input instanceof Request ? input : new Request(input, init)
      const body = request.method === 'GET' || request.method === 'HEAD'
        ? undefined
        : await request.text()
      const signal = requestSignals.get(request) ?? request.signal
      if (signal.aborted)
        throw signal.reason
      return fetch(request.url, {
        body,
        headers: request.headers,
        method: request.method,
        signal,
      })
    },
    parseAs: 'json',
  })

  daemonApi.interceptors.request.use((request, requestOptions) => {
    const signal = (Reflect.get(requestOptions, 'meta') as undefined | { signal?: AbortSignal })?.signal
    if (signal !== undefined)
      requestSignals.set(request, signal)
    return request
  })

  return {
    close({ signal }) {
      throwIfAborted(signal)
      for (const socket of sockets) socket.close()
      sockets.clear()
    },
    async connect({ signal }: OperationOptions) {
      throwIfAborted(signal)
    },
    daemonApi,
    duplex(call: DuplexCallOptions) {
      throwIfAborted(call.signal)

      const route = webSocketRoute(call)
      const url = new URL('/apis/auv/runtime/v1/invoke', endpoint)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'

      const socket = new WebSocketConstructor(url)
      sockets.add(socket)
      socket.binaryType = 'arraybuffer'

      return openWebSocketCall(socket, call, route, () => sockets.delete(socket))
    },
    async unary(call: UnaryCall) {
      if (call.http === undefined) {
        throw new Error(`${call.method} has no HTTP binding`)
      }

      const headers = new Headers(call.headers)
      const json = call.http.encoding === 'json'
      const body = call.http.method === 'GET'
        ? undefined
        : json
          ? call.jsonBody
          : Uint8Array.from(call.body).buffer

      if (body !== undefined) {
        headers.set('content-type', json ? 'application/json' : 'application/protobuf')
      }

      try {
        const response = await fetch(new URL(call.http.path, endpoint), {
          body,
          headers,
          method: call.http.method,
          signal: call.signal,
        })
        if (!response.ok) {
          throw await httpError(response)
        }

        return json
          ? call.decodeJson(await response.text())
          : new Uint8Array(await response.arrayBuffer())
      }
      catch (error) {
        if (call.signal?.aborted)
          throw abortError(call.signal)

        if (error instanceof AuvRemoteError || error instanceof AuvProtocolError)
          throw error

        throw new AuvTransportError('AUV HTTP transport failed', error)
      }
    },
  }
}

async function httpError(response: Response): Promise<Error> {
  if (response.headers.get('content-type')?.startsWith('application/problem+json')) {
    return auvHttpError(response, await response.json())
  }

  return auvHttpError(response, await response.text())
}

async function openWebSocketCall(socket: WebSocket, call: DuplexCallOptions, route: WebSocketRoute, onClose: () => void): Promise<DuplexCall> {
  const responses = new AsyncQueue<Uint8Array>()
  let ready: () => void
  let rejectReady: (reason: unknown) => void
  let ended = false
  let readyReceived = false
  const opened = new Promise<void>((resolve, reject) => {
    ready = resolve
    rejectReady = reject
  })

  const send = (message: Parameters<typeof create<typeof ClientMessageSchema>>[1]) => {
    socket.send(toBinary(ClientMessageSchema, create(ClientMessageSchema, message)))
  }
  const abort = () => {
    if (socket.readyState === 1) {
      send({ message: { case: 'cancel', value: { reason: String(call.signal!.reason) } } })
    }

    ended = true
    const error = abortError(call.signal!)
    responses.fail(error, true)
    socket.close()

    rejectReady(error)
  }
  const fail = (error: Error) => {
    if (ended)
      return

    ended = true
    call.signal?.removeEventListener('abort', abort)
    rejectReady(error)
    responses.fail(error)
    socket.close()
  }
  socket.addEventListener('open', () => {
    send({
      message: {
        case: 'open',
        value: {
          credential: call.headers.get('authorization')?.replace(/^Bearer /, '') ?? '',
          deviceId: call.headers.get('auv-device-id') ?? undefined,
          method: route.method,
          runId: call.headers.get('auv-run-id') ?? undefined,
          runnerClass: route.runnerClass,
          service: route.service,
        },
      },
    })
  })
  socket.addEventListener('message', (event) => {
    let message

    try {
      message = fromBinary(ServerMessageSchema, new Uint8Array(event.data as ArrayBuffer))
    }
    catch (error) {
      fail(new AuvProtocolError('AUV WebSocket returned an invalid ServerMessage', error))
      return
    }

    switch (message.message.case) {
      case 'end': {
        ended = true

        call.signal?.removeEventListener('abort', abort)

        const end = message.message.value

        const error = end.grpcStatus === 0 ? undefined : new AuvWebSocketError(end.grpcStatus, end.message)
        if (!readyReceived)
          rejectReady(error ?? new AuvProtocolError('AUV WebSocket returned End before Ready'))
        if (error === undefined)
          responses.end()
        else responses.fail(error)

        socket.close()

        break
      }
      case 'output':
        if (!readyReceived) {
          fail(new AuvProtocolError('AUV WebSocket returned Output before Ready'))
          return
        }

        responses.push(message.message.value.payload)

        break
      case 'ready':
        if (readyReceived) {
          fail(new AuvProtocolError('AUV WebSocket returned Ready more than once'))
          return
        }

        readyReceived = true
        ready()

        break
      case undefined:
        fail(new AuvProtocolError('AUV WebSocket returned a ServerMessage without a message'))

        break
    }
  })
  socket.addEventListener('error', () => {
    fail(new AuvTransportError('AUV WebSocket transport failed'))
  })
  socket.addEventListener('close', () => {
    onClose()
    call.signal?.removeEventListener('abort', abort)
    if (!ended) {
      const error = new AuvTransportError('AUV WebSocket closed before the operation ended')
      rejectReady(error)
      responses.fail(error)
    }
  })

  call.signal?.addEventListener('abort', abort, { once: true })
  throwIfAborted(call.signal)

  await opened

  return {
    close({ signal }) {
      throwIfAborted(signal)
      send({ message: { case: 'cancel', value: { reason: 'closed by caller' } } })

      socket.close()
    },
    async halfClose() {
      send({ message: { case: 'halfClose', value: {} } })
    },
    responses,
    async send(body) {
      send({ message: { case: 'input', value: { payload: body } } })
    },
  }
}

function webSocketRoute(call: DuplexCallOptions): WebSocketRoute {
  const segments = call.method.replace(/^\//, '').split('/')
  if (segments.length !== 2 || segments.some(segment => segment.length === 0)) {
    throw new AuvConfigurationError(`invalid RPC method path: ${call.method}`)
  }

  const runnerClass = call.headers.get('auv-runner-class')
  if (runnerClass === null || runnerClass.length === 0) {
    throw new AuvConfigurationError('WebSocket invoke requires auv-runner-class')
  }

  return {
    method: segments[1]!,
    runnerClass,
    service: segments[0]!,
  }
}
