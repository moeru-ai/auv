import type { ClientDuplexStream, ClientUnaryCall, ServiceError } from '@grpc/grpc-js'

import type { DuplexCall, DuplexCallOptions, OperationOptions, Transport, UnaryCall } from '../transport/types'

import { Buffer } from 'node:buffer'

import { ChannelCredentials, Client, Metadata } from '@grpc/grpc-js'

import { AsyncQueue } from '../transport/async-queue'
import { abortError, AuvRpcError, AuvTransportError, throwIfAborted } from '../transport/errors'

export interface GrpcTransportOptions {
  endpoint?: string | URL
}

export interface NamedPipeTransportOptions {
  path: string | URL
}

export interface UnixSocketTransportOptions {
  path: string | URL
}

/** Creates a Node.js gRPC transport for an AUV TCP listener. */
export function createGrpcTransport(options: GrpcTransportOptions = {}): Transport {
  const endpoint = grpcEndpoint(options.endpoint ?? 'http://127.0.0.1:9847')
  return grpcTransport(endpoint.target, endpoint.credentials)
}

/** Creates a Node.js gRPC transport over an AUV Windows named pipe. */
export function createNamedPipeTransport(options: NamedPipeTransportOptions): Transport {
  const value = options.path.toString()
  const path = value.startsWith('npipe://') ? namedPipePath(new URL(value)) : value
  return grpcTransport(`unix:${path}`, ChannelCredentials.createInsecure())
}

/** Creates a Node.js gRPC transport over an AUV Unix socket. */
export function createUnixSocketTransport(options: UnixSocketTransportOptions): Transport {
  const path = options.path instanceof URL
    ? options.path.pathname
    : options.path.startsWith('unix://')
      ? new URL(options.path).pathname
      : options.path

  return grpcTransport(`unix:${path}`, ChannelCredentials.createInsecure())
}

async function grpcDuplex(client: Client, call: DuplexCallOptions): Promise<DuplexCall> {
  throwIfAborted(call.signal)

  const metadata = new Metadata()
  call.headers.forEach((value, name) => metadata.set(name, value))

  const responses = new AsyncQueue<Uint8Array>()
  const request: ClientDuplexStream<Uint8Array, Uint8Array> = client.makeBidiStreamRequest(
    call.method,
    value => Buffer.from(value),
    value => value,
    metadata,
  )

  let settled = false
  const abort = () => {
    if (settled)
      return
    settled = true
    responses.fail(abortError(call.signal!), true)
    request.cancel()
  }

  const cleanup = () => call.signal?.removeEventListener('abort', abort)
  request.on('data', value => responses.push(value))
  request.on('end', () => {
    if (settled)
      return

    settled = true
    cleanup()
    responses.end()
  })
  request.on('error', (error) => {
    if (settled)
      return
    settled = true
    cleanup()
    responses.fail(grpcError(error))
  })

  call.signal?.addEventListener('abort', abort, { once: true })

  return {
    close() {
      cleanup()
      request.cancel()
    },
    halfClose() {
      request.end()
      return Promise.resolve()
    },
    responses,
    send(body) {
      return new Promise<void>((resolve, reject) => request.write(body, (error?: Error | null) => {
        if (error)
          reject(new AuvTransportError('AUV gRPC stream write failed', error))

        else resolve()
      }))
    },
  }
}

function grpcEndpoint(endpoint: string | URL): { credentials: ChannelCredentials, target: string } {
  const url = new URL(endpoint)
  switch (url.protocol) {
    case 'http:':
      return { credentials: ChannelCredentials.createInsecure(), target: url.host }
    case 'https:':
      return { credentials: ChannelCredentials.createSsl(), target: url.host }
    default:
      throw new TypeError(`unsupported gRPC endpoint protocol: ${url.protocol}`)
  }
}

function grpcError(error: ServiceError): AuvRpcError {
  return new AuvRpcError(error.code, error.details, error)
}

function grpcTransport(target: string, credentials: ChannelCredentials): Transport {
  const client = new Client(target, credentials)
  return {
    close() {
      client.close()
    },
    connect(options) {
      return waitForReady(client, options)
    },
    duplex(call) {
      return grpcDuplex(client, call)
    },
    unary(call) {
      return grpcUnary(client, call)
    },
  }
}

function grpcUnary(client: Client, call: UnaryCall): Promise<Uint8Array> {
  throwIfAborted(call.signal)

  const metadata = new Metadata()
  call.headers.forEach((value, name) => metadata.set(name, value))

  return new Promise((resolve, reject) => {
    let request: ClientUnaryCall
    let settled = false
    let onAbort: () => void

    const finish = (result: () => void) => {
      if (settled)
        return

      settled = true
      call.signal?.removeEventListener('abort', onAbort)

      result()
    }

    onAbort = () => {
      finish(() => {
        request.cancel()
        reject(abortError(call.signal!))
      })
    }

    request = client.makeUnaryRequest<Uint8Array, Uint8Array>(
      call.method,
      value => Buffer.from(value),
      value => value,
      call.body,
      metadata,
      (error, response) => {
        if (error)
          finish(() => reject(grpcError(error)))

        else finish(() => resolve(response!))
      },
    )

    call.signal?.addEventListener('abort', onAbort, { once: true })
    if (call.signal?.aborted)
      onAbort()
  })
}

function namedPipePath(url: URL): string {
  const prefix = '/pipe/'
  const name = url.pathname.startsWith(prefix) ? url.pathname.slice(prefix.length) : ''
  if (url.protocol !== 'npipe:' || url.hostname !== '.' || !/^[\w.-]+$/.test(name))
    throw new TypeError(`invalid AUV named-pipe endpoint: ${url.toString()}`)
  return `\\\\.\\pipe\\${name}`
}

function waitForReady(client: Client, options: OperationOptions): Promise<void> {
  throwIfAborted(options.signal)

  return new Promise((resolve, reject) => {
    let settled = false
    let onAbort: () => void

    const finish = (result: () => void) => {
      if (settled)
        return
      settled = true
      options.signal?.removeEventListener('abort', onAbort)
      result()
    }

    onAbort = () => {
      finish(() => {
        client.close()
        reject(abortError(options.signal!))
      })
    }

    options.signal?.addEventListener('abort', onAbort, { once: true })
    client.waitForReady(Infinity, (error) => {
      if (error)
        finish(() => reject(new AuvTransportError('AUV gRPC connection failed', error)))

      else finish(resolve)
    })

    if (options.signal?.aborted)
      onAbort()
  })
}
