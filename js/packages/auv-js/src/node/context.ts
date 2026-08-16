import type { AuvConnection, ConnectOptions, DeviceCredential } from '../transport/connection'
import type { OperationOptions } from '../transport/types'

import process from 'node:process'

import { AuvConfigurationError } from '../transport/errors'
import { connect } from './connect'

/** Non-secret execution context inherited through `AUV_CONTEXT`. */
export interface AuvContext {
  readonly configProfile?: string
  readonly daemonEndpoint?: string
  readonly deviceId?: string
  readonly deviceName?: string
  readonly invocationId?: string
  readonly runId?: string
}

export type AuvEnvironment = Readonly<Record<string, string | undefined>>

export interface ConnectFromContextOptions extends OperationOptions {
  /** Application-owned credential for a paired Device profile. */
  credential?: DeviceCredential
  /** Explicit transport adapter; otherwise inferred from `daemonEndpoint`. */
  transport?: ConnectOptions['transport']
}

/** Connects to the daemon selected by an already resolved `AuvContext`. */
export async function connectFromContext(
  context: AuvContext,
  options: ConnectFromContextOptions = {},
): Promise<AuvConnection> {
  if (context.daemonEndpoint === undefined) {
    throw new AuvConfigurationError(
      'AuvContext does not contain a resolved daemon_endpoint',
    )
  }

  if (context.configProfile !== undefined && options.credential === undefined) {
    // TODO(js-context-profile): shared config_profile credential lookup is
    // deferred because credential persistence remains application-owned. Reopen
    // when an approved JavaScript plugin consumer requires Rust profile parity.
    throw new AuvConfigurationError(
      'AuvContext config_profile requires an application-owned credential',
    )
  }

  return connect({
    credential: options.credential,
    endpoint: context.daemonEndpoint,
    route: {
      deviceId: context.deviceId,
      runId: context.runId,
    },
    signal: options.signal,
    transport: options.transport ?? inferredTransport(context.daemonEndpoint),
  })
}

/** Parses the additive, non-secret `AUV_CONTEXT` process contract. */
export function contextFromEnv(
  env: AuvEnvironment = process.env,
): Readonly<AuvContext> {
  const encoded = env.AUV_CONTEXT
  if (encoded === undefined)
    throw new AuvConfigurationError('AUV_CONTEXT is unavailable')

  let decoded: unknown
  try {
    decoded = JSON.parse(encoded)
  }
  catch {
    throw new AuvConfigurationError('AUV_CONTEXT is not valid JSON')
  }

  if (typeof decoded !== 'object' || decoded === null || Array.isArray(decoded))
    throw new AuvConfigurationError('AUV_CONTEXT must be a JSON object')

  const source = decoded as Readonly<Record<string, unknown>>
  return Object.freeze({
    configProfile: optionalString(source, 'config_profile'),
    daemonEndpoint: optionalString(source, 'daemon_endpoint'),
    deviceId: optionalString(source, 'device_id'),
    deviceName: optionalString(source, 'device_name'),
    invocationId: optionalString(source, 'invocation_id'),
    runId: optionalString(source, 'run_id'),
  })
}

function inferredTransport(endpoint: string): 'grpc' | 'npipe' | 'unix' {
  if (endpoint.startsWith('unix:'))
    return 'unix'
  return endpoint.startsWith('npipe:') ? 'npipe' : 'grpc'
}

function optionalString(source: Readonly<Record<string, unknown>>, field: string): string | undefined {
  const value = source[field]
  if (value === undefined)
    return undefined
  if (typeof value !== 'string')
    throw new AuvConfigurationError(`AUV_CONTEXT.${field} must be a string`)
  return value
}
