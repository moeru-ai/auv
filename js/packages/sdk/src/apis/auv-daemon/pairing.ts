import type {
  V1CreatePairingTokenRequest,
  V1PairDeviceRequest,
  V1RevokeDeviceCredentialRequest,
  V1SetPairedDeviceEnabledRequest,
  V1UnpairDeviceRequest,
} from '@auv-js/api-client'

import type { AuvConnection, DeviceCredential, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import {
  pairingServiceCreatePairingToken,
  pairingServicePairDevice,
  pairingServiceRevokeDeviceCredential,
  pairingServiceSetPairedDeviceEnabled,
  pairingServiceUnpairDevice,
} from '@auv-js/api-client'

import {
  CreatePairingTokenRequestSchema,
  CreatePairingTokenResponseSchema,
  PairDeviceRequestSchema,
  PairDeviceResponseSchema,
  PairingService,
  RevokeDeviceCredentialRequestSchema,
  RevokeDeviceCredentialResponseSchema,
  SetPairedDeviceEnabledRequestSchema,
  SetPairedDeviceEnabledResponseSchema,
  UnpairDeviceRequestSchema,
  UnpairDeviceResponseSchema,
} from '../../gen/auv/api/daemon/v1/pairing_pb'
import { duration, timestampDate } from './wire'

export interface CreatePairingTokenOptions extends OperationOptions {
  ttlMs?: number
}

export interface PairDeviceOptions extends OperationOptions {
  deviceId?: string
  label: string
  token: PairingToken
}

export interface PairedDeviceOptions extends OperationOptions {
  selector: string
}

export interface PairingEnrollment {
  credential: DeviceCredential
  deviceId: string
}

export interface PairingToken {
  expiresAt: Date | undefined
  value: string
}

export interface SetPairedDeviceEnabledOptions extends OperationOptions {
  enabled: boolean
  selector: string
}

const createPairingTokenRpc = {
  input: CreatePairingTokenRequestSchema,
  method: `/${PairingService.typeName}/${PairingService.method.createPairingToken.name}`,
  output: CreatePairingTokenResponseSchema,
  rest: ({ body, ...options }) => pairingServiceCreatePairingToken({ body: body as V1CreatePairingTokenRequest, ...options }),
} satisfies RpcDefinition<typeof CreatePairingTokenRequestSchema, typeof CreatePairingTokenResponseSchema>

const pairDeviceRpc = {
  input: PairDeviceRequestSchema,
  method: `/${PairingService.typeName}/${PairingService.method.pairDevice.name}`,
  output: PairDeviceResponseSchema,
  rest: ({ body, ...options }) => pairingServicePairDevice({ body: body as V1PairDeviceRequest, ...options }),
} satisfies RpcDefinition<typeof PairDeviceRequestSchema, typeof PairDeviceResponseSchema>

const revokeDeviceCredentialRpc = {
  input: RevokeDeviceCredentialRequestSchema,
  method: `/${PairingService.typeName}/${PairingService.method.revokeDeviceCredential.name}`,
  output: RevokeDeviceCredentialResponseSchema,
  rest: ({ body, ...options }) => pairingServiceRevokeDeviceCredential({ body: body as V1RevokeDeviceCredentialRequest, ...options }),
} satisfies RpcDefinition<typeof RevokeDeviceCredentialRequestSchema, typeof RevokeDeviceCredentialResponseSchema>

const setPairedDeviceEnabledRpc = {
  input: SetPairedDeviceEnabledRequestSchema,
  method: `/${PairingService.typeName}/${PairingService.method.setPairedDeviceEnabled.name}`,
  output: SetPairedDeviceEnabledResponseSchema,
  rest: ({ body, ...options }) => pairingServiceSetPairedDeviceEnabled({ body: body as V1SetPairedDeviceEnabledRequest, ...options }),
} satisfies RpcDefinition<typeof SetPairedDeviceEnabledRequestSchema, typeof SetPairedDeviceEnabledResponseSchema>

const unpairDeviceRpc = {
  input: UnpairDeviceRequestSchema,
  method: `/${PairingService.typeName}/${PairingService.method.unpairDevice.name}`,
  output: UnpairDeviceResponseSchema,
  rest: ({ body, ...options }) => pairingServiceUnpairDevice({ body: body as V1UnpairDeviceRequest, ...options }),
} satisfies RpcDefinition<typeof UnpairDeviceRequestSchema, typeof UnpairDeviceResponseSchema>

/** Creates a one-time Device enrollment token. */
export async function createPairingToken(
  connection: AuvConnection,
  options: CreatePairingTokenOptions = {},
): Promise<PairingToken> {
  const ttl = options.ttlMs === undefined
    ? undefined
    : duration(options.ttlMs)
  const response = await connection.unary(createPairingTokenRpc, { ttl }, options)
  return {
    expiresAt: timestampDate(response.expiresAt),
    value: response.token,
  }
}

/** Consumes a one-time token and enrolls this caller as a paired Device. */
export async function pairDevice(connection: AuvConnection, options: PairDeviceOptions): Promise<PairingEnrollment> {
  const response = await connection.unary(pairDeviceRpc, {
    deviceId: options.deviceId ?? randomDeviceId(),
    label: options.label,
    token: options.token.value,
  }, options)
  return {
    credential: response.deviceCredential,
    deviceId: response.deviceId,
  }
}

/** Revokes every credential issued to a paired Device. */
export async function revokeDeviceCredential(connection: AuvConnection, options: PairedDeviceOptions): Promise<boolean> {
  const response = await connection.unary(revokeDeviceCredentialRpc, { deviceId: options.selector }, options)
  return response.revoked
}

/** Enables or disables a paired Device. */
export async function setPairedDeviceEnabled(connection: AuvConnection, options: SetPairedDeviceEnabledOptions): Promise<boolean> {
  const response = await connection.unary(setPairedDeviceEnabledRpc, {
    deviceSelector: options.selector,
    enabled: options.enabled,
  }, options)
  return response.changed
}

/** Removes a paired Device and its credentials. */
export async function unpairDevice(connection: AuvConnection, options: PairedDeviceOptions): Promise<boolean> {
  const response = await connection.unary(unpairDeviceRpc, { deviceSelector: options.selector }, options)
  return response.removed
}

function randomDeviceId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32))
  return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
}
