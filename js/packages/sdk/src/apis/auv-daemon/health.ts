import type { AuvConnection, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import { healthServiceCheck } from '@auv-js/api-client'

import {
  CheckRequestSchema,
  CheckResponseSchema,
  HealthService,
  HealthStatus as ProtoHealthStatus,
} from '../../gen/auv/api/daemon/v1/health_pb'
import { AuvProtocolError } from '../../transport/errors'

/** Identity and readiness shared by every listener of one daemon instance. */
export interface Health {
  id: string
  status: HealthStatus
}

/** Readiness state reported by one AUV daemon listener. */
export type HealthStatus = 'serving'

const checkHealthRpc = {
  input: CheckRequestSchema,
  method: `/${HealthService.typeName}/${HealthService.method.check.name}`,
  output: CheckResponseSchema,
  rest: ({ client, headers, signal }) => healthServiceCheck({ client, headers, signal }),
} satisfies RpcDefinition<typeof CheckRequestSchema, typeof CheckResponseSchema>

/**
 * Checks a daemon listener and returns its instance identity and readiness.
 * checkHealth -> connection.unary -> HealthService.Check / GET /health.
 */
export async function checkHealth(connection: AuvConnection, options: OperationOptions = {}): Promise<Health> {
  const response = await connection.unary(checkHealthRpc, {}, options)
  if (response.status !== ProtoHealthStatus.SERVING)
    throw new AuvProtocolError(`AUV daemon reported unknown health status ${String(response.status)}`)

  if (!response.id)
    throw new AuvProtocolError('AUV daemon health response omitted its instance id')

  return { id: response.id, status: 'serving' }
}
