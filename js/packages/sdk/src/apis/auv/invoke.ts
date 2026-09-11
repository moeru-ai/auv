import type { DescMessage, MessageInitShape, MessageShape } from '@bufbuild/protobuf'

import type { AuvConnection, RpcDefinition, TypedDuplexCall } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import { AuvConfigurationError } from '../../transport/errors'
import { selectedDevice } from './routing'

export interface InvokeDuplexOptions<I extends DescMessage, O extends DescMessage> extends OperationOptions {
  deviceId?: string
  input: I
  method: string
  output: O
  runId?: string
  runnerClass: string
  service: string
}

export type InvokeServerStreamOptions<I extends DescMessage, O extends DescMessage> = InvokeUnaryOptions<I, O>

export interface InvokeUnaryOptions<I extends DescMessage, O extends DescMessage> extends OperationOptions {
  deviceId?: string
  input: I
  method: string
  output: O
  request: MessageInitShape<I>
  runId?: string
  runnerClass: string
  service: string
}

/** Opens one generated bidirectional capability operation. */
export async function invokeDuplex<I extends DescMessage, O extends DescMessage>(
  connection: AuvConnection,
  options: InvokeDuplexOptions<I, O>,
): Promise<TypedDuplexCall<I, O>> {
  return await connection.duplex(definition(options), {
    headers: routeHeaders(connection, options),
    signal: options.signal,
  })
}

/** Invokes one generated server-streaming capability operation. */
export async function invokeServerStream<I extends DescMessage, O extends DescMessage>(
  connection: AuvConnection,
  options: InvokeServerStreamOptions<I, O>,
): Promise<AsyncIterable<MessageShape<O>>> {
  const stream = await invokeDuplex(connection, options)
  await stream.send(options.request)
  await stream.halfClose()
  return stream.responses
}

/** Invokes one generated unary capability operation through a routed Runner. */
export async function invokeUnary<I extends DescMessage, O extends DescMessage>(
  connection: AuvConnection,
  options: InvokeUnaryOptions<I, O>,
): Promise<MessageShape<O>> {
  return await connection.unary(definition(options), options.request, {
    headers: routeHeaders(connection, options),
    signal: options.signal,
  })
}

function definition<I extends DescMessage, O extends DescMessage>(
  options: InvokeDuplexOptions<I, O>,
): RpcDefinition<I, O> {
  return {
    http: () => ({
      method: 'POST',
      path: `/apis/auv/runtime/v1/invoke/${encodeURIComponent(options.service)}/${encodeURIComponent(options.method)}`,
    }),
    input: options.input,
    method: `/${options.service}/${options.method}`,
    output: options.output,
  }
}

function inheritedRouteValue(kind: 'Device' | 'Run', explicit?: string, inherited?: string): string | undefined {
  if (explicit !== undefined && inherited !== undefined && explicit !== inherited) {
    throw new AuvConfigurationError(
      `explicit ${kind} selection conflicts with the connection's inherited ${kind} context`,
    )
  }
  return explicit ?? inherited
}

function routeHeaders(connection: AuvConnection, options: { deviceId?: string, runId?: string, runnerClass: string }): Headers {
  const headers = new Headers({ 'auv-runner-class': options.runnerClass })
  const deviceId = selectedDevice(connection.local, inheritedRouteValue(
    'Device',
    options.deviceId,
    connection.route.deviceId,
  ))
  const runId = inheritedRouteValue('Run', options.runId, connection.route.runId)

  if (deviceId !== undefined)
    headers.set('auv-device-id', deviceId)
  if (runId !== undefined)
    headers.set('auv-run-id', runId)

  return headers
}
