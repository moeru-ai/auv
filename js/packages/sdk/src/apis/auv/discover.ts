import type { DescMethod, FileRegistry, JsonObject, JsonValue } from '@bufbuild/protobuf'

import type { AuvConnection } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import { create, createFileRegistry, fromBinary, fromJson, getOption, hasOption, toJson } from '@bufbuild/protobuf'
import { FileDescriptorProtoSchema, FileDescriptorSetSchema } from '@bufbuild/protobuf/wkt'

import { discoverable, effect, MethodEffect } from '../../gen/auv/api/annotations/v1/annotations_pb'
import { ServerReflection, ServerReflectionRequestSchema, ServerReflectionResponseSchema } from '../../gen/grpc/reflection/v1/reflection_pb'
import { AuvProtocolError } from '../../transport/errors'
import { invokeDuplex, invokeServerStream, invokeUnary } from './invoke'
import { protobufJsonSchema } from './json'

export type DiscoveredMethodEffect = 'administration' | 'input' | 'mutation' | 'read_only' | 'unspecified'

// TODO(discovered-tool-presentation): The annotation contract has no title or
// description fields. Add them here after the owning protobuf contract defines
// their source and override rules.
export interface DiscoveredRpcMethod {
  readonly effect: DiscoveredMethodEffect
  readonly id: string
  readonly inputSchema: Readonly<Record<string, unknown>>
  readonly method: string
  readonly methodKind: DescMethod['methodKind']
  readonly outputSchema: Readonly<Record<string, unknown>>
  readonly service: string
}

/** Discovered methods and dynamic ProtoJSON invocation for one RunnerClass. */
export interface DiscoveredRunner {
  readonly apis: readonly DiscoveredRpcMethod[]
  invokeServerStreamJson: (options: InvokeDiscoveredOptions) => Promise<AsyncIterable<JsonValue>>
  invokeUnaryJson: (options: InvokeDiscoveredOptions) => Promise<JsonValue>
  readonly runnerClass: string
}

export interface DiscoverRunnerOptions extends OperationOptions {
  deviceId?: string
  runId?: string
  runnerClass: string
}

export interface InvokeDiscoveredOptions extends OperationOptions {
  input?: JsonObject
  method: DiscoveredRpcMethod | string
}

type ReflectedFileDescriptor = ReturnType<typeof fromBinary<typeof FileDescriptorProtoSchema>>

// TODO(discovered-streaming-tools): client-streaming and bidi business-method
// projection is deferred until a concrete tool host defines incremental input
// and cancellation UX. Callers can still use the typed invoke.duplex surface.

/** Discovers one registered RunnerClass through AUV's routed gRPC proxy. */
export async function discoverRunner(connection: AuvConnection, options: DiscoverRunnerOptions): Promise<DiscoveredRunner> {
  const reflectionMethod = ServerReflection.method.serverReflectionInfo
  const stream = await invokeDuplex(connection, {
    deviceId: options.deviceId,
    input: ServerReflectionRequestSchema,
    method: reflectionMethod.name,
    output: ServerReflectionResponseSchema,
    runId: options.runId,
    runnerClass: options.runnerClass,
    service: reflectionMethod.parent.typeName,
    signal: options.signal,
  })
  const responses = stream.responses[Symbol.asyncIterator]()
  let completed = false

  try {
    const servicesResponse = await reflectionRequest(stream, responses, {
      host: '',
      messageRequest: { case: 'listServices', value: '' },
    })
    if (servicesResponse.messageResponse.case !== 'listServicesResponse') {
      throw new AuvProtocolError(
        `gRPC Reflection returned ${servicesResponse.messageResponse.case ?? 'an empty response'} for list_services`,
      )
    }

    const files = new Map<string, ReflectedFileDescriptor>()
    const serviceNames = servicesResponse.messageResponse.value.service
      .map(service => service.name)
      .filter(name => name.length > 0)

    for (const service of serviceNames) {
      const response = await reflectionRequest(stream, responses, {
        host: '',
        messageRequest: { case: 'fileContainingSymbol', value: service },
      })
      if (response.messageResponse.case !== 'fileDescriptorResponse') {
        throw new AuvProtocolError(
          `gRPC Reflection returned ${response.messageResponse.case ?? 'an empty response'} for file_containing_symbol ${service}`,
        )
      }
      collectFileDescriptors(files, response.messageResponse.value.fileDescriptorProto)
    }

    const requestedDependencies = new Set<string>()
    while (true) {
      const dependency = [...files.values()].flatMap(file => file.dependency).find(name => !files.has(name))
      if (dependency === undefined)
        break
      if (requestedDependencies.has(dependency))
        throw new AuvProtocolError(`gRPC Reflection did not return requested dependency ${dependency}`)
      requestedDependencies.add(dependency)
      const response = await reflectionRequest(stream, responses, {
        host: '',
        messageRequest: { case: 'fileByFilename', value: dependency },
      })
      if (response.messageResponse.case !== 'fileDescriptorResponse') {
        throw new AuvProtocolError(
          `gRPC Reflection returned ${response.messageResponse.case ?? 'an empty response'} for file_by_filename ${dependency}`,
        )
      }
      collectFileDescriptors(files, response.messageResponse.value.fileDescriptorProto)
    }

    await stream.halfClose()
    const end = await responses.next()
    if (!end.done)
      throw new AuvProtocolError('gRPC Reflection returned an unsolicited response after client half-close')
    completed = true

    const registry = createFileRegistry(create(FileDescriptorSetSchema, { file: dependencyOrderedFiles(files) }))
    return discoveredRunner(connection, registry, serviceNames, options)
  }
  finally {
    if (!completed)
      await stream.close().catch(() => {})
  }
}

function collectFileDescriptors(files: Map<string, ReflectedFileDescriptor>, encoded: readonly Uint8Array[]): void {
  for (const bytes of encoded) {
    const file = fromBinary(FileDescriptorProtoSchema, bytes)
    if (file.name.length === 0)
      throw new AuvProtocolError('gRPC Reflection returned a FileDescriptorProto without a name')
    files.set(file.name, file)
  }
}

function dependencyOrderedFiles(files: ReadonlyMap<string, ReflectedFileDescriptor>): ReflectedFileDescriptor[] {
  const ordered: ReflectedFileDescriptor[] = []
  const visiting = new Set<string>()
  const visited = new Set<string>()

  const visit = (file: ReflectedFileDescriptor) => {
    if (visited.has(file.name))
      return
    if (visiting.has(file.name))
      throw new AuvProtocolError(`gRPC Reflection returned a descriptor dependency cycle at ${file.name}`)
    visiting.add(file.name)
    for (const dependency of file.dependency) {
      const descriptor = files.get(dependency)
      if (descriptor === undefined)
        throw new AuvProtocolError(`gRPC Reflection omitted dependency ${dependency}, imported by ${file.name}`)
      visit(descriptor)
    }
    visiting.delete(file.name)
    visited.add(file.name)
    ordered.push(file)
  }

  for (const file of files.values()) visit(file)
  return ordered
}

function discoveredEffect(value: MethodEffect): DiscoveredMethodEffect {
  switch (value) {
    case MethodEffect.ADMINISTRATION: return 'administration'
    case MethodEffect.INPUT: return 'input'
    case MethodEffect.MUTATION: return 'mutation'
    case MethodEffect.READ_ONLY: return 'read_only'
    case MethodEffect.UNSPECIFIED: return 'unspecified'
  }
}

function discoveredMethod(method: DescMethod): DiscoveredRpcMethod {
  return {
    effect: discoveredEffect(hasOption(method, effect) ? getOption(method, effect) : MethodEffect.UNSPECIFIED),
    id: `/${method.parent.typeName}/${method.name}`,
    inputSchema: protobufJsonSchema(method.input),
    method: method.name,
    methodKind: method.methodKind,
    outputSchema: protobufJsonSchema(method.output),
    service: method.parent.typeName,
  }
}

function discoveredRunner(
  connection: AuvConnection,
  registry: FileRegistry,
  serviceNames: readonly string[],
  route: DiscoverRunnerOptions,
): DiscoveredRunner {
  const apis = serviceNames.flatMap((serviceName) => {
    const service = registry.getService(serviceName)
    if (service === undefined)
      throw new AuvProtocolError(`gRPC Reflection omitted the descriptor for service ${serviceName}`)
    return service.methods
      .filter(method => hasOption(method, discoverable) && getOption(method, discoverable))
      .map(method => discoveredMethod(method))
  })
  const byId = new Map(apis.map(method => [method.id, method]))

  const resolve = (value: DiscoveredRpcMethod | string): { descriptor: DescMethod, method: DiscoveredRpcMethod } => {
    const id = typeof value === 'string'
      ? (value.startsWith('/') ? value : `/${value}`)
      : value.id
    const method = byId.get(id)
    if (method === undefined)
      throw new AuvProtocolError(`discovered Runner ${route.runnerClass} does not expose ${id}`)
    const [serviceName, methodName] = methodPartsFrom(id)
    const descriptor = registry.getService(serviceName)?.methods.find(candidate => candidate.name === methodName)
    if (descriptor === undefined)
      throw new AuvProtocolError(`reflected descriptor registry does not contain ${id}`)
    return { descriptor, method }
  }

  return {
    apis,
    async invokeServerStreamJson(options) {
      const { descriptor, method } = resolve(options.method)
      if (descriptor.methodKind !== 'server_streaming')
        throw new AuvProtocolError(`${method.id} is ${descriptor.methodKind}, not server_streaming`)
      const request = fromJson(descriptor.input, options.input ?? {}, { registry })
      const responses = await invokeServerStream(connection, {
        deviceId: route.deviceId,
        input: descriptor.input,
        method: descriptor.name,
        output: descriptor.output,
        request,
        runId: route.runId,
        runnerClass: route.runnerClass,
        service: descriptor.parent.typeName,
        signal: options.signal ?? route.signal,
      })
      return encodeJsonResponses(descriptor, responses, registry)
    },
    async invokeUnaryJson(options) {
      const { descriptor, method } = resolve(options.method)
      if (descriptor.methodKind !== 'unary')
        throw new AuvProtocolError(`${method.id} is ${descriptor.methodKind}, not unary`)
      const request = fromJson(descriptor.input, options.input ?? {}, { registry })
      const response = await invokeUnary(connection, {
        deviceId: route.deviceId,
        input: descriptor.input,
        method: descriptor.name,
        output: descriptor.output,
        request,
        runId: route.runId,
        runnerClass: route.runnerClass,
        service: descriptor.parent.typeName,
        signal: options.signal ?? route.signal,
      })
      return toJson(descriptor.output, response, { registry })
    },
    runnerClass: route.runnerClass,
  }
}

async function* encodeJsonResponses(
  method: DescMethod,
  responses: AsyncIterable<unknown>,
  registry: FileRegistry,
): AsyncIterable<JsonValue> {
  for await (const response of responses)
    yield toJson(method.output, response as never, { registry })
}

function methodPartsFrom(id: string): [string, string] {
  const parts = id.slice(1).split('/')
  if (parts.length !== 2 || parts.some(part => part.length === 0))
    throw new AuvProtocolError(`invalid discovered gRPC method ID: ${id}`)
  return [parts[0], parts[1]]
}

async function reflectionRequest(
  stream: Awaited<ReturnType<typeof invokeDuplex<typeof ServerReflectionRequestSchema, typeof ServerReflectionResponseSchema>>>,
  responses: AsyncIterator<ReturnType<typeof create<typeof ServerReflectionResponseSchema>>>,
  request: Parameters<typeof stream.send>[0],
) {
  await stream.send(request)
  const response = await responses.next()
  if (response.done)
    throw new AuvProtocolError('gRPC Reflection ended before answering a request')
  if (response.value.messageResponse.case === 'errorResponse') {
    const error = response.value.messageResponse.value
    throw new AuvProtocolError(`gRPC Reflection failed with status ${error.errorCode}: ${error.errorMessage}`)
  }
  return response.value
}
