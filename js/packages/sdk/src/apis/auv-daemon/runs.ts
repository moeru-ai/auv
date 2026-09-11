import type { V1CreateRunRequest, V1GetRunRequest, V1StopRunRequest } from '@auv-js/api-client'

import type { Run as ProtoRun } from '../../gen/auv/api/daemon/v1/run_pb'
import type { AuvConnection, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import {
  runServiceCreateRun,
  runServiceGetRun,
  runServiceListRuns,
  runServiceStopRun,
} from '@auv-js/api-client'

import {
  CreateRunRequestSchema,
  CreateRunResponseSchema,
  GetRunRequestSchema,
  GetRunResponseSchema,
  ListRunsRequestSchema,
  ListRunsResponseSchema,

  RunOutcome as ProtoRunOutcome,
  RunPhase as ProtoRunPhase,
  RunService,
  StopRunRequestSchema,
  StopRunResponseSchema,
} from '../../gen/auv/api/daemon/v1/run_pb'
import { AuvProtocolError } from '../../transport/errors'
import { selectedDevices } from '../auv/routing'
import { timestampDate, unknownEnum } from './wire'

export interface CreateRunOptions extends OperationOptions {
  deviceIds?: readonly string[]
  labels?: Readonly<Record<string, string>>
}
export interface GetRunOptions extends OperationOptions {
  runId: string
}

export interface Run {
  completedAt: Date | undefined
  createdAt: Date | undefined
  deviceIds: readonly string[]
  id: string
  labels: Readonly<Record<string, string>>
  phase: RunPhase
}

export type RunOutcome = 'canceled' | 'failed' | 'succeeded'

export type RunPhase = 'canceled' | 'failed' | 'pending' | 'running' | 'succeeded' | 'unspecified'

export interface StopRunOptions extends GetRunOptions {
  outcome: RunOutcome
}

const createRunRpc = {
  input: CreateRunRequestSchema,
  method: `/${RunService.typeName}/${RunService.method.createRun.name}`,
  output: CreateRunResponseSchema,
  rest: ({ body, ...options }) => runServiceCreateRun({ body: body as V1CreateRunRequest, ...options }),
} satisfies RpcDefinition<typeof CreateRunRequestSchema, typeof CreateRunResponseSchema>
const listRunsRpc = {
  input: ListRunsRequestSchema,
  method: `/${RunService.typeName}/${RunService.method.listRuns.name}`,
  output: ListRunsResponseSchema,
  rest: ({ client, headers, signal }) => runServiceListRuns({ client, headers, signal }),
} satisfies RpcDefinition<typeof ListRunsRequestSchema, typeof ListRunsResponseSchema>
const getRunRpc = {
  input: GetRunRequestSchema,
  method: `/${RunService.typeName}/${RunService.method.getRun.name}`,
  output: GetRunResponseSchema,
  rest: ({ body, ...options }) => runServiceGetRun({ body: body as V1GetRunRequest, ...options }),
} satisfies RpcDefinition<typeof GetRunRequestSchema, typeof GetRunResponseSchema>
const stopRunRpc = {
  input: StopRunRequestSchema,
  method: `/${RunService.typeName}/${RunService.method.stopRun.name}`,
  output: StopRunResponseSchema,
  rest: ({ body, ...options }) => runServiceStopRun({ body: body as V1StopRunRequest, ...options }),
} satisfies RpcDefinition<typeof StopRunRequestSchema, typeof StopRunResponseSchema>

/** Creates a Run on the selected Devices. */
export async function createRun(connection: AuvConnection, options: CreateRunOptions = {}): Promise<Run> {
  const response = await connection.unary(createRunRpc, {
    devices: selectedDevices(connection.local, options.deviceIds)?.map(deviceId => ({ deviceId })),
    labels: options.labels,
  }, options)
  if (response.run === undefined) {
    throw new AuvProtocolError('AUV response omitted CreateRunResponse.run')
  }
  return run(response.run)
}

/** Gets one Run by canonical identity. */
export async function getRun(connection: AuvConnection, options: GetRunOptions): Promise<Run> {
  const response = await connection.unary(getRunRpc, { run: { runId: options.runId } }, options)
  if (response.run === undefined) {
    throw new AuvProtocolError('AUV response omitted GetRunResponse.run')
  }
  return run(response.run)
}

/** Lists Runs visible to the connected caller. */
export async function listRuns(connection: AuvConnection, options: OperationOptions = {}): Promise<readonly Run[]> {
  const response = await connection.unary(listRunsRpc, {}, options)
  return response.runs.map(run)
}

/** Stops a Run with an explicit terminal outcome. */
export async function stopRun(connection: AuvConnection, options: StopRunOptions): Promise<Run> {
  const response = await connection.unary(stopRunRpc, {
    outcome: runOutcome(options.outcome),
    run: { runId: options.runId },
  }, options)
  if (response.run === undefined) {
    throw new AuvProtocolError('AUV response omitted StopRunResponse.run')
  }
  return run(response.run)
}

function run(value: ProtoRun): Run {
  const id = value.ref?.runId
  if (id === undefined || id.length === 0) {
    throw new AuvProtocolError('AUV response omitted Run.ref.run_id')
  }
  const deviceIds = value.devices.map((device) => {
    if (device.deviceId.length === 0) {
      throw new AuvProtocolError('AUV response omitted Run.devices.device_id')
    }
    return device.deviceId
  })
  return {
    completedAt: timestampDate(value.completedAt),
    createdAt: timestampDate(value.createdAt),
    deviceIds,
    id,
    labels: value.labels,
    phase: runPhase(value.phase),
  }
}

function runOutcome(value: RunOutcome): ProtoRunOutcome {
  switch (value) {
    case 'canceled': return ProtoRunOutcome.CANCELED
    case 'failed': return ProtoRunOutcome.FAILED
    case 'succeeded': return ProtoRunOutcome.SUCCEEDED
  }
}

function runPhase(value: ProtoRunPhase): RunPhase {
  switch (value) {
    case ProtoRunPhase.CANCELED: return 'canceled'
    case ProtoRunPhase.FAILED: return 'failed'
    case ProtoRunPhase.PENDING: return 'pending'
    case ProtoRunPhase.RUNNING: return 'running'
    case ProtoRunPhase.SUCCEEDED: return 'succeeded'
    case ProtoRunPhase.UNSPECIFIED: return 'unspecified'
    default: return unknownEnum('Run.phase', value)
  }
}
