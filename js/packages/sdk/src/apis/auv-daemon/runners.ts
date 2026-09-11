import type {
  V1CreateRunnerRequest,
  V1DeleteRunnerRequest,
  V1GetRunnerClassRequest,
  V1GetRunnerRequest,
  V1ListRunnerClassesRequest,
} from '@auv-js/api-client'

import type { Runner as ProtoRunner, RunnerClass as ProtoRunnerClass } from '../../gen/auv/api/daemon/v1/runner_pb'
import type { AuvConnection, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import {
  runnerClassServiceGetRunnerClass,
  runnerClassServiceListRunnerClasses,
  runnerServiceCreateRunner,
  runnerServiceDeleteRunner,
  runnerServiceGetRunner,
  runnerServiceListRunners,
} from '@auv-js/api-client'

import {
  CreateRunnerRequestSchema,
  CreateRunnerResponseSchema,
  DeleteRunnerRequestSchema,
  DeleteRunnerResponseSchema,
  GetRunnerClassRequestSchema,
  GetRunnerClassResponseSchema,
  GetRunnerRequestSchema,
  GetRunnerResponseSchema,
  ListRunnerClassesRequestSchema,
  ListRunnerClassesResponseSchema,
  ListRunnersRequestSchema,
  ListRunnersResponseSchema,

  RunnerLifecycle as ProtoRunnerLifecycle,
  RunnerPhase as ProtoRunnerPhase,
  RunnerClassService,
  RunnerService,
} from '../../gen/auv/api/daemon/v1/runner_pb'
import { AuvProtocolError } from '../../transport/errors'
import { selectedDevice } from '../auv/routing'
import { duration, durationMilliseconds, timestampDate, unknownEnum } from './wire'

export interface CreateRunnerOptions extends OperationOptions {
  deviceId?: string
  idleTimeoutMs?: number
  labels?: Readonly<Record<string, string>>
  lifecycle: Exclude<RunnerLifecycle, 'unspecified' | number>
  runnerClass: string
}
export interface DeleteRunnerOptions extends GetRunnerOptions {
  force?: boolean
  gracePeriodMs?: number
}

export interface GetRunnerClassOptions extends ListRunnerClassesOptions {
  runnerClass: string
}

export interface GetRunnerOptions extends OperationOptions {
  runnerId: string
}

export interface ListRunnerClassesOptions extends OperationOptions {
  deviceId?: string
}

export interface Runner {
  activeOperations: bigint
  createdAt: Date | undefined
  deviceId: string
  id: string
  idleDeadline: Date | undefined
  idleTimeoutMs: number | undefined
  labels: Readonly<Record<string, string>>
  lifecycle: RunnerLifecycle
  phase: RunnerPhase
  processId: number
  runnerClass: string
}

export interface RunnerClass {
  available: boolean
  deviceId: string | undefined
  displayName: string
  id: string
  supportedLifecycles: readonly RunnerLifecycle[]
}

export type RunnerLifecycle = 'ephemeral' | 'unless_idle' | 'unless_shutdown' | 'unspecified'

export type RunnerPhase = 'draining' | 'failed' | 'ready' | 'starting' | 'stopped' | 'unspecified'

const createRunnerRpc = {
  input: CreateRunnerRequestSchema,
  method: `/${RunnerService.typeName}/${RunnerService.method.createRunner.name}`,
  output: CreateRunnerResponseSchema,
  rest: ({ body, ...options }) => runnerServiceCreateRunner({ body: body as V1CreateRunnerRequest, ...options }),
} satisfies RpcDefinition<typeof CreateRunnerRequestSchema, typeof CreateRunnerResponseSchema>
const listRunnersRpc = {
  input: ListRunnersRequestSchema,
  method: `/${RunnerService.typeName}/${RunnerService.method.listRunners.name}`,
  output: ListRunnersResponseSchema,
  rest: ({ client, headers, signal }) => runnerServiceListRunners({ client, headers, signal }),
} satisfies RpcDefinition<typeof ListRunnersRequestSchema, typeof ListRunnersResponseSchema>
const getRunnerRpc = {
  input: GetRunnerRequestSchema,
  method: `/${RunnerService.typeName}/${RunnerService.method.getRunner.name}`,
  output: GetRunnerResponseSchema,
  rest: ({ body, ...options }) => runnerServiceGetRunner({ body: body as V1GetRunnerRequest, ...options }),
} satisfies RpcDefinition<typeof GetRunnerRequestSchema, typeof GetRunnerResponseSchema>
const deleteRunnerRpc = {
  input: DeleteRunnerRequestSchema,
  method: `/${RunnerService.typeName}/${RunnerService.method.deleteRunner.name}`,
  output: DeleteRunnerResponseSchema,
  rest: ({ body, ...options }) => runnerServiceDeleteRunner({ body: body as V1DeleteRunnerRequest, ...options }),
} satisfies RpcDefinition<typeof DeleteRunnerRequestSchema, typeof DeleteRunnerResponseSchema>
const listRunnerClassesRpc = {
  input: ListRunnerClassesRequestSchema,
  method: `/${RunnerClassService.typeName}/${RunnerClassService.method.listRunnerClasses.name}`,
  output: ListRunnerClassesResponseSchema,
  rest: ({ body, ...options }) => runnerClassServiceListRunnerClasses({ body: body as V1ListRunnerClassesRequest, ...options }),
} satisfies RpcDefinition<typeof ListRunnerClassesRequestSchema, typeof ListRunnerClassesResponseSchema>
const getRunnerClassRpc = {
  input: GetRunnerClassRequestSchema,
  method: `/${RunnerClassService.typeName}/${RunnerClassService.method.getRunnerClass.name}`,
  output: GetRunnerClassResponseSchema,
  rest: ({ body, ...options }) => runnerClassServiceGetRunnerClass({ body: body as V1GetRunnerClassRequest, ...options }),
} satisfies RpcDefinition<typeof GetRunnerClassRequestSchema, typeof GetRunnerClassResponseSchema>

/** Creates a Runner. */
export async function createRunner(connection: AuvConnection, options: CreateRunnerOptions): Promise<Runner> {
  const response = await connection.unary(createRunnerRpc, {
    device: deviceRef(connection, options.deviceId),
    idleTimeout: options.idleTimeoutMs === undefined ? undefined : duration(options.idleTimeoutMs),
    labels: options.labels,
    lifecycle: runnerLifecycleInput(options.lifecycle),
    runnerClass: { runnerClass: options.runnerClass },
  }, options)
  if (response.runner === undefined) {
    throw new AuvProtocolError('AUV response omitted CreateRunnerResponse.runner')
  }
  return runner(response.runner)
}

/** Stops and deletes one Runner. */
export async function deleteRunner(connection: AuvConnection, options: DeleteRunnerOptions): Promise<Runner> {
  const response = await connection.unary(deleteRunnerRpc, {
    force: options.force,
    gracePeriod: options.gracePeriodMs === undefined ? undefined : duration(options.gracePeriodMs),
    runner: { runnerId: options.runnerId },
  }, options)
  if (response.runner === undefined) {
    throw new AuvProtocolError('AUV response omitted DeleteRunnerResponse.runner')
  }
  return runner(response.runner)
}

/** Gets one Runner. */
export async function getRunner(connection: AuvConnection, options: GetRunnerOptions): Promise<Runner> {
  const response = await connection.unary(getRunnerRpc, { runner: { runnerId: options.runnerId } }, options)
  if (response.runner === undefined) {
    throw new AuvProtocolError('AUV response omitted GetRunnerResponse.runner')
  }
  return runner(response.runner)
}

/** Gets one RunnerClass. */
export async function getRunnerClass(connection: AuvConnection, options: GetRunnerClassOptions): Promise<RunnerClass> {
  const response = await connection.unary(getRunnerClassRpc, {
    device: deviceRef(connection, options.deviceId),
    runnerClass: { runnerClass: options.runnerClass },
  }, options)
  if (response.runnerClass === undefined) {
    throw new AuvProtocolError('AUV response omitted GetRunnerClassResponse.runner_class')
  }
  return runnerClass(response.runnerClass)
}

/** Lists RunnerClasses for the selected or implicit local Device. */
export async function listRunnerClasses(connection: AuvConnection, options: ListRunnerClassesOptions = {}): Promise<readonly RunnerClass[]> {
  const response = await connection.unary(listRunnerClassesRpc, {
    device: deviceRef(connection, options.deviceId),
  }, options)
  return response.runnerClasses.map(runnerClass)
}

/** Lists Runner instances. */
export async function listRunners(connection: AuvConnection, options: OperationOptions = {}): Promise<readonly Runner[]> {
  const response = await connection.unary(listRunnersRpc, {}, options)
  return response.runners.map(runner)
}

function deviceRef(connection: AuvConnection, deviceId: string | undefined): undefined | { deviceId: string } {
  const selected = selectedDevice(connection.local, deviceId)
  return selected === undefined ? undefined : { deviceId: selected }
}

function runner(value: ProtoRunner): Runner {
  const deviceId = value.device?.deviceId
  if (deviceId === undefined || deviceId.length === 0) {
    throw new AuvProtocolError('AUV response omitted Runner.device.device_id')
  }
  const id = value.ref?.runnerId
  if (id === undefined || id.length === 0) {
    throw new AuvProtocolError('AUV response omitted Runner.ref.runner_id')
  }
  const runnerClass = value.runnerClass?.runnerClass
  if (runnerClass === undefined || runnerClass.length === 0) {
    throw new AuvProtocolError('AUV response omitted Runner.runner_class.runner_class')
  }
  return {
    activeOperations: value.activeOperations,
    createdAt: timestampDate(value.createdAt),
    deviceId,
    id,
    idleDeadline: timestampDate(value.idleDeadline),
    idleTimeoutMs: durationMilliseconds(value.idleTimeout),
    labels: value.labels,
    lifecycle: runnerLifecycle(value.lifecycle),
    phase: runnerPhase(value.phase),
    processId: value.processId,
    runnerClass,
  }
}

function runnerClass(value: ProtoRunnerClass): RunnerClass {
  const id = value.ref?.runnerClass
  if (id === undefined || id.length === 0) {
    throw new AuvProtocolError('AUV response omitted RunnerClass.ref.runner_class')
  }
  return {
    available: value.available,
    deviceId: value.device?.deviceId,
    displayName: value.displayName,
    id,
    supportedLifecycles: value.supportedLifecycles.map(runnerLifecycle),
  }
}

function runnerLifecycle(value: ProtoRunnerLifecycle): RunnerLifecycle {
  switch (value) {
    case ProtoRunnerLifecycle.EPHEMERAL: return 'ephemeral'
    case ProtoRunnerLifecycle.UNLESS_IDLE: return 'unless_idle'
    case ProtoRunnerLifecycle.UNLESS_SHUTDOWN: return 'unless_shutdown'
    case ProtoRunnerLifecycle.UNSPECIFIED: return 'unspecified'
    default: return unknownEnum('Runner.lifecycle', value)
  }
}

function runnerLifecycleInput(value: CreateRunnerOptions['lifecycle']): ProtoRunnerLifecycle {
  switch (value) {
    case 'ephemeral': return ProtoRunnerLifecycle.EPHEMERAL
    case 'unless_idle': return ProtoRunnerLifecycle.UNLESS_IDLE
    case 'unless_shutdown': return ProtoRunnerLifecycle.UNLESS_SHUTDOWN
  }
}

function runnerPhase(value: ProtoRunnerPhase): RunnerPhase {
  switch (value) {
    case ProtoRunnerPhase.DRAINING: return 'draining'
    case ProtoRunnerPhase.FAILED: return 'failed'
    case ProtoRunnerPhase.READY: return 'ready'
    case ProtoRunnerPhase.STARTING: return 'starting'
    case ProtoRunnerPhase.STOPPED: return 'stopped'
    case ProtoRunnerPhase.UNSPECIFIED: return 'unspecified'
    default: return unknownEnum('Runner.phase', value)
  }
}
