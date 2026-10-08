import type { DescFile, DescMessage, DescMethod, DescService, MessageInitShape, MessageShape } from '@bufbuild/protobuf'

import type { DuplexCall, Transport } from '../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { FileDescriptorProtoSchema } from '@bufbuild/protobuf/wkt'

import { ServerReflection } from '../gen/grpc/reflection/v1/reflection_pb'
import { AsyncQueue } from '../transport/async-queue'
import { AuvRpcError } from '../transport/errors'

// An in-memory Runner: the SDK talks to it through an ordinary `Transport`,
// with the same encoded messages a real Runner exchanges, so clients, call
// recording and replay cannot tell it from a device. The registration style
// follows Connect-ES's `createRouterTransport`
// (connectrpc/connect-es `packages/connect/src/router-transport.ts`).
// See docs/ai/references/session-api/2026-10-08-mock-runner-design.md.

/** What a mock method sees of its call. */
export interface MockCall {
  readonly headers: Headers
  /** gRPC path, `/package.Service/Method`. */
  readonly method: string
  readonly signal?: AbortSignal
}

/** A mock implementation of one method, typed from its generated descriptor. */
export type MockMethod<M>
  = M extends { input: infer I extends DescMessage, methodKind: 'unary', output: infer O extends DescMessage }
    ? (request: MessageShape<I>, call: MockCall) => MaybePromise<MessageInitShape<O>>
    : M extends { input: infer I extends DescMessage, methodKind: 'server_streaming', output: infer O extends DescMessage }
      ? (request: MessageShape<I>, call: MockCall) => AsyncIterable<MessageInitShape<O>>
      : M extends { input: infer I extends DescMessage, methodKind: 'bidi_streaming', output: infer O extends DescMessage }
        ? (requests: AsyncIterable<MessageShape<I>>, call: MockCall) => AsyncIterable<MessageInitShape<O>>
        : never

export interface MockRouter {
  service: <S extends DescService>(service: S, implementation: MockService<S>) => void
}

/**
 * A mock implementation of a generated service: any subset of its methods,
 * by their JavaScript names. Methods left out answer `UNIMPLEMENTED`, like a
 * Runner that does not serve them.
 */
export type MockService<S extends DescService> = { [K in keyof S['method']]?: MockMethod<S['method'][K]> }

type MaybePromise<T> = Promise<T> | T

interface Route {
  handler: (...args: never[]) => unknown
  method: DescMethod
}

// gRPC status codes.
const NOT_FOUND = 5
const UNIMPLEMENTED = 12

/**
 * A `Transport` answered by in-memory service implementations:
 *
 * ```ts
 * const transport = createMockTransport(({ service }) => {
 *   service(WindowService, { listWindows: () => ({ windows }) })
 * })
 * ```
 *
 * Handlers report failures by throwing `AuvRpcError` with a gRPC status code.
 * gRPC Reflection is answered for the registered services, so discovery and
 * method presentation work as on a device.
 */
export function createMockTransport(define: (router: MockRouter) => void): Transport {
  const routes = new Map<string, Route>()
  const services: DescService[] = []
  define({
    service(service, implementation) {
      services.push(service)
      for (const method of service.methods) {
        const handler = (implementation as Record<string, Route['handler'] | undefined>)[method.localName]
        if (handler)
          routes.set(`/${service.typeName}/${method.name}`, { handler, method })
      }
    },
  })
  const reflection = reflectionOf(services)
  routes.set(`/${ServerReflection.typeName}/${ServerReflection.method.serverReflectionInfo.name}`, {
    handler: reflection as Route['handler'],
    method: ServerReflection.method.serverReflectionInfo,
  })

  const route = (method: string): Route => {
    const found = routes.get(method)
    if (!found)
      throw new AuvRpcError(UNIMPLEMENTED, `the mock Runner does not implement ${method}`)
    return found
  }

  return {
    close() {},
    async connect() {},
    async duplex(options): Promise<DuplexCall> {
      const { handler, method } = route(options.method)
      const call: MockCall = { headers: options.headers, method: options.method, signal: options.signal }
      const sent = new AsyncQueue<Uint8Array>()
      const requests = decodeAll(method.input, sent)
      const outputs = method.methodKind === 'server_streaming'
        ? (async function* () {
            const first = await requests[Symbol.asyncIterator]().next()
            if (first.done)
              return
            yield* (handler as (request: unknown, call: MockCall) => AsyncIterable<unknown>)(first.value, call)
          })()
        : (handler as (requests: AsyncIterable<unknown>, call: MockCall) => AsyncIterable<unknown>)(requests, call)
      return {
        close: () => sent.end(),
        halfClose: async () => sent.end(),
        responses: (async function* () {
          for await (const output of outputs)
            yield toBinary(method.output, create(method.output, output as MessageInitShape<DescMessage>))
        })(),
        send: async body => sent.push(body),
      }
    },
    async unary(options) {
      const { handler, method } = route(options.method)
      if (method.methodKind !== 'unary')
        throw new AuvRpcError(UNIMPLEMENTED, `${options.method} is ${method.methodKind}, not unary`)
      const request = fromBinary(method.input, options.body)
      const output = await (handler as (request: unknown, call: MockCall) => unknown)(request, {
        headers: options.headers,
        method: options.method,
        signal: options.signal,
      })
      return toBinary(method.output, create(method.output, output as MessageInitShape<DescMessage>))
    },
  }
}

async function* decodeAll(schema: DescMessage, bodies: AsyncIterable<Uint8Array>): AsyncIterable<MessageShape<DescMessage>> {
  for await (const body of bodies)
    yield fromBinary(schema, body)
}

/**
 * gRPC Reflection v1 over the registered services' generated descriptors:
 * the service list, the file of a symbol, and files by name for
 * dependencies, which is what `discoverRunner` asks.
 */
function reflectionOf(services: readonly DescService[]) {
  const files = new Map<string, DescFile>()
  const pending = services.map(service => service.file)
  for (let file = pending.pop(); file; file = pending.pop()) {
    if (files.has(file.proto.name))
      continue
    files.set(file.proto.name, file)
    pending.push(...file.dependencies)
  }
  const encode = (file: DescFile) => toBinary(FileDescriptorProtoSchema, file.proto)
  // Like the Runner's reflection, the reflection service does not list itself.
  const names = services.map(service => service.typeName)

  return async function* (requests: AsyncIterable<MessageShape<typeof ServerReflection.method.serverReflectionInfo.input>>) {
    for await (const request of requests) {
      const asked = request.messageRequest
      const answer = (messageResponse: MessageInitShape<typeof ServerReflection.method.serverReflectionInfo.output>['messageResponse']) =>
        ({ messageResponse, originalRequest: request, validHost: request.host })
      const notFound = (what: string) => answer({ case: 'errorResponse', value: { errorCode: NOT_FOUND, errorMessage: `${what} is not known to the mock Runner` } })
      switch (asked.case) {
        case 'fileByFilename': {
          const file = files.get(asked.value)
          yield file
            ? answer({ case: 'fileDescriptorResponse', value: { fileDescriptorProto: [encode(file)] } })
            : notFound(`file ${asked.value}`)
          break
        }
        case 'fileContainingSymbol': {
          const service = services.find(candidate => asked.value === candidate.typeName || asked.value.startsWith(`${candidate.typeName}.`))
          yield service
            ? answer({ case: 'fileDescriptorResponse', value: { fileDescriptorProto: [encode(service.file)] } })
            : notFound(`symbol ${asked.value}`)
          break
        }
        case 'listServices':
          yield answer({ case: 'listServicesResponse', value: { service: names.map(name => ({ name })) } })
          break
        default:
          yield answer({ case: 'errorResponse', value: { errorCode: UNIMPLEMENTED, errorMessage: `reflection ${asked.case ?? 'request'} is not implemented by the mock Runner` } })
      }
    }
  }
}
