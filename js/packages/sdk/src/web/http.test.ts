import type { AuvAbortError, AuvHttpError, AuvProtocolError, AuvWebSocketError } from './index'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { ClientMessageSchema, ServerMessageSchema } from '../gen/auv/api/transport/websocket/v1/websocket_pb'
import { connect, createHttpTransport, createRun, listDevices } from './index'

describe('hTTP transport', () => {
  it('sends authenticated ProtoJSON requests to the canonical resource route', async () => {
    const signal = new AbortController().signal
    const fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      expect(input.toString()).toBe('http://auv.example:9847/apis/auv/runtime/v1/runs')
      expect(init?.method).toBe('POST')
      expect(init?.signal).toBe(signal)
      const headers = new Headers(init?.headers)
      expect(headers.get('authorization')).toBe('Bearer paired-secret')
      expect(headers.get('content-type')).toBe('application/json')
      expect(JSON.parse(init?.body as string)).toEqual({})
      return new Response(JSON.stringify({
        run: { ref: { runId: 'run-http' } },
      }), {
        headers: { 'content-type': 'application/json' },
      })
    }
    const connection = await connect({
      credential: 'paired-secret',
      signal,
      transport: createHttpTransport({ endpoint: 'http://auv.example:9847', fetch }),
    })

    await expect(createRun(connection, { signal })).resolves.toMatchObject({ id: 'run-http' })
  })

  it('does not send a body for GET operations', async () => {
    const fetch = async (_input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      expect(init?.body).toBeUndefined()
      return new Response('{}')
    }
    const connection = await connect({ transport: createHttpTransport({ fetch }) })

    await expect(listDevices(connection)).resolves.toEqual([])
  })

  it('exposes the daemon problem response without replacing its detail', async () => {
    const fetch = async (): Promise<Response> => new Response(JSON.stringify({
      detail: 'paired Device bearer required',
      status: 401,
      title: 'unauthenticated',
      type: 'urn:auv:error:unauthenticated',
    }), {
      headers: { 'content-type': 'application/problem+json' },
      status: 401,
    })
    const connection = await connect({ transport: createHttpTransport({ fetch }) })

    await expect(listDevices(connection)).rejects.toEqual(expect.objectContaining<AuvHttpError>({
      message: 'paired Device bearer required',
      name: 'AuvHttpError',
      status: 401,
      type: 'urn:auv:error:unauthenticated',
    }))
  })

  it('normalizes abort while consuming an HTTP response body', async () => {
    const controller = new AbortController()
    const reason = new DOMException('view closed', 'AbortError')
    const fetch = async (): Promise<Response> => {
      const response = new Response()
      response.text = async () => {
        controller.abort(reason)
        throw reason
      }
      return response
    }
    const connection = await connect({ transport: createHttpTransport({ fetch }) })

    await expect(listDevices(connection, { signal: controller.signal })).rejects.toEqual(
      expect.objectContaining<AuvAbortError>({
        cause: reason,
        message: 'AUV operation was aborted',
        name: 'AuvAbortError',
      }),
    )
  })

  it('opens one authenticated WebSocket lane and forwards stream frames', async () => {
    const socket = new FakeWebSocket()
    const transport = createHttpTransport({
      endpoint: 'http://auv.test:9847',
      fetch: async () => new Response(),
      webSocket: class {
        constructor(url: string | URL) {
          expect(String(url)).toBe('ws://auv.test:9847/apis/auv/runtime/v1/invoke')
          queueMicrotask(() => socket.open())
          return socket
        }
      } as unknown as typeof WebSocket,
    })
    const stream = await transport.duplex({
      headers: new Headers({
        'authorization': 'Bearer device-secret',
        'auv-runner-class': 'example.runner.v1',
      }),
      method: '/example.Service/Watch',
    })
    const open = fromBinary(ClientMessageSchema, socket.sent[0]!)
    expect(open.message).toEqual({
      case: 'open',
      value: expect.objectContaining({
        credential: 'device-secret',
        method: 'Watch',
        runnerClass: 'example.runner.v1',
        service: 'example.Service',
      }),
    })

    await stream.send(Uint8Array.of(1, 2))
    await stream.halfClose()
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'output', value: { payload: Uint8Array.of(3, 4) } },
    })))
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'end', value: { grpcStatus: 0, message: '' } },
    })))

    const output: Uint8Array[] = []
    for await (const value of stream.responses) output.push(value)
    expect(output).toEqual([Uint8Array.of(3, 4)])
    expect(fromBinary(ClientMessageSchema, socket.sent[1]!).message.case).toBe('input')
    expect(fromBinary(ClientMessageSchema, socket.sent[2]!).message.case).toBe('halfClose')
  })

  it('delivers buffered outputs before a terminal RPC error', async () => {
    const socket = new FakeWebSocket()
    const transport = createHttpTransport({
      webSocket: class {
        constructor() {
          queueMicrotask(() => socket.open())
          return socket
        }
      } as unknown as typeof WebSocket,
    })
    const stream = await transport.duplex({
      headers: new Headers({ 'auv-runner-class': 'example.runner.v1' }),
      method: '/example.Service/Watch',
    })
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'output', value: { payload: Uint8Array.of(7) } },
    })))
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'end', value: { grpcStatus: 13, message: 'runner failed' } },
    })))
    const iterator = stream.responses[Symbol.asyncIterator]()

    await expect(iterator.next()).resolves.toEqual({ done: false, value: Uint8Array.of(7) })
    await expect(iterator.next()).rejects.toEqual(expect.objectContaining<AuvWebSocketError>({
      grpcStatus: 13,
      message: 'runner failed',
      name: 'AuvWebSocketError',
      rpcCode: 13,
    }))
  })

  it('sends stream cancellation and rejects iteration with a normalized abort error', async () => {
    const socket = new FakeWebSocket()
    const transport = createHttpTransport({
      webSocket: class {
        constructor() {
          queueMicrotask(() => socket.open())
          return socket
        }
      } as unknown as typeof WebSocket,
    })
    const controller = new AbortController()
    const stream = await transport.duplex({
      headers: new Headers({ 'auv-runner-class': 'example.runner.v1' }),
      method: '/example.Service/Watch',
      signal: controller.signal,
    })
    const reason = new DOMException('page closed', 'AbortError')
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'output', value: { payload: Uint8Array.of(9) } },
    })))

    controller.abort(reason)
    const next = stream.responses[Symbol.asyncIterator]().next()

    await expect(next).rejects.toEqual(expect.objectContaining<AuvAbortError>({
      cause: reason,
      message: 'AUV operation was aborted',
      name: 'AuvAbortError',
    }))
    expect(fromBinary(ClientMessageSchema, socket.sent.at(-1)!).message.case).toBe('cancel')
  })

  it('rejects stream establishment when the daemon ends before Ready', async () => {
    const socket = new FakeWebSocket()
    const transport = createHttpTransport({
      webSocket: class {
        constructor() {
          queueMicrotask(() => socket.openWithoutReady())
          return socket
        }
      } as unknown as typeof WebSocket,
    })
    const pending = transport.duplex({
      headers: new Headers({ 'auv-runner-class': 'missing.runner' }),
      method: '/example.Service/Watch',
    })
    await Promise.resolve()
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'end', value: { grpcStatus: 5, message: 'runner not found' } },
    })))

    await expect(pending).rejects.toEqual(expect.objectContaining<AuvWebSocketError>({
      grpcStatus: 5,
      message: 'runner not found',
      name: 'AuvWebSocketError',
      rpcCode: 5,
    }))
  })

  it('rejects an empty ServerMessage as a protocol error', async () => {
    const socket = new FakeWebSocket()
    const transport = createHttpTransport({
      webSocket: class {
        constructor() {
          queueMicrotask(() => socket.openWithoutReady())
          return socket
        }
      } as unknown as typeof WebSocket,
    })
    const pending = transport.duplex({
      headers: new Headers({ 'auv-runner-class': 'example.runner.v1' }),
      method: '/example.Service/Watch',
    })
    await Promise.resolve()
    socket.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema)))

    await expect(pending).rejects.toEqual(expect.objectContaining<AuvProtocolError>({
      message: 'AUV WebSocket returned a ServerMessage without a message',
      name: 'AuvProtocolError',
    }))
  })
})

class FakeWebSocket {
  binaryType = ''
  readyState = 0
  sent: Uint8Array[] = []
  private listeners = new Map<string, Array<(event: Event | MessageEvent) => void>>()

  addEventListener(name: string, listener: (event: Event | MessageEvent) => void) {
    const listeners = this.listeners.get(name) ?? []
    listeners.push(listener)
    this.listeners.set(name, listeners)
  }

  close() {
    this.readyState = 3
    this.emit('close', new Event('close'))
  }

  open() {
    this.openWithoutReady()
    this.receive(toBinary(ServerMessageSchema, create(ServerMessageSchema, {
      message: { case: 'ready', value: {} },
    })))
  }

  openWithoutReady() {
    this.readyState = 1
    this.emit('open', new Event('open'))
  }

  receive(data: Uint8Array) {
    this.emit('message', new MessageEvent('message', { data: data.buffer }))
  }

  send(data: Uint8Array) {
    this.sent.push(data)
  }

  private emit(name: string, event: Event | MessageEvent) {
    for (const listener of this.listeners.get(name) ?? []) listener(event)
  }
}
