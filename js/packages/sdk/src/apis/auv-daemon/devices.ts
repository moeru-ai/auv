import type { V1GetDeviceRequest } from '@auv-js/api-client'

import type { Device as ProtoDevice, UserSession as ProtoUserSession } from '../../gen/auv/api/daemon/v1/device_pb'
import type { AuvConnection, RpcDefinition } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import {
  deviceServiceGetDevice,
  deviceServiceListDevices,
} from '@auv-js/api-client'

import {
  DeviceEntryEffectKind,
  DeviceService,
  EnsureUserSessionUnlockedRequestSchema,
  EnsureUserSessionUnlockedResponseSchema,
  GetDeviceRequestSchema,
  GetDeviceResponseSchema,
  GetUserSessionRequestSchema,
  GetUserSessionResponseSchema,
  ListDevicesRequestSchema,
  ListDevicesResponseSchema,
  ListUserSessionsRequestSchema,
  ListUserSessionsResponseSchema,
  DeviceEntryErrorReason as ProtoDeviceEntryErrorReason,
  DevicePlatform as ProtoDevicePlatform,
  UserSessionConnectionKind as ProtoUserSessionConnectionKind,
  UserSessionLockState as ProtoUserSessionLockState,
} from '../../gen/auv/api/daemon/v1/device_pb'
import { AuvConfigurationError, AuvProtocolError, AuvRemoteError } from '../../transport/errors'
import { unknownEnum } from './wire'

/** One Device visible to the connected AUV caller. */
export interface Device {
  id: string
  labels: Readonly<Record<string, string>>
  local: boolean
  name: string
  platform: DevicePlatform
}

export type DeviceEntryErrorReason
  = | 'ambiguousUser'
    | 'auditUnavailable'
    | 'credentialRejected'
    | 'disabled'
    | 'occupiedDesktop'
    | 'outcomeUnverified'
    | 'serviceUnavailable'
    | 'staleSession'
    | 'suspended'
    | 'unauthorized'
    | 'unenrolled'
    | 'unsupportedOsState'

export type DevicePlatform = 'linux' | 'macos' | 'unspecified' | 'windows'

/** Select exactly one current session or its OS account; no credential crosses this RPC. */
export type EnsureUserSessionUnlockedOptions = OperationOptions & (
  | { sessionSelector: string, user?: never }
  | { sessionSelector?: never, user: string }
)

export interface GetDeviceOptions extends OperationOptions {
  deviceId: string
}
export interface GetUserSessionOptions extends OperationOptions {
  sessionSelector: string
}

/** One current OS login instance on the connected Device. */
export interface UserSession {
  connectionKind: UserSessionConnectionKind
  /** Both predicates are false when the OS could not determine the state. */
  isLocked: boolean
  isUnlocked: boolean
  lockState: UserSessionLockState
  seat: string | undefined
  sessionSelector: string
  user: string
}

export type UserSessionConnectionKind = 'physical' | 'remote' | 'unspecified'

export type UserSessionLockState = 'locked' | 'unknown' | 'usable'

export interface UserSessionUnlockEffect {
  kind: 'alreadyUsable' | 'unlockedExistingSession'
  sessionSelector: string | undefined
  user: string
}

/** A fixed, non-secret Device entry rejection returned by the target. */
export class AuvDeviceEntryError extends AuvRemoteError {
  constructor(readonly reason: DeviceEntryErrorReason) {
    super(`Device entry failed: ${reason}`)
    this.name = 'AuvDeviceEntryError'
  }
}

const listDevicesRpc = {
  input: ListDevicesRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.listDevices.name}`,
  output: ListDevicesResponseSchema,
  rest: ({ client, headers, signal }) => deviceServiceListDevices({ client, headers, signal }),
} satisfies RpcDefinition<typeof ListDevicesRequestSchema, typeof ListDevicesResponseSchema>

const getDeviceRpc = {
  input: GetDeviceRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.getDevice.name}`,
  output: GetDeviceResponseSchema,
  rest: ({ body, ...options }) => deviceServiceGetDevice({ body: body as V1GetDeviceRequest, ...options }),
} satisfies RpcDefinition<typeof GetDeviceRequestSchema, typeof GetDeviceResponseSchema>

// NOTICE(device-entry-http): These session methods have no HTTP binding. Keep
// them on the gRPC transport until the approved REST contract is implemented.
const listUserSessionsRpc = {
  input: ListUserSessionsRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.listUserSessions.name}`,
  output: ListUserSessionsResponseSchema,
} satisfies RpcDefinition<typeof ListUserSessionsRequestSchema, typeof ListUserSessionsResponseSchema>

const getUserSessionRpc = {
  input: GetUserSessionRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.getUserSession.name}`,
  output: GetUserSessionResponseSchema,
} satisfies RpcDefinition<typeof GetUserSessionRequestSchema, typeof GetUserSessionResponseSchema>

const ensureUserSessionUnlockedRpc = {
  input: EnsureUserSessionUnlockedRequestSchema,
  method: `/${DeviceService.typeName}/${DeviceService.method.ensureUserSessionUnlocked.name}`,
  output: EnsureUserSessionUnlockedResponseSchema,
} satisfies RpcDefinition<typeof EnsureUserSessionUnlockedRequestSchema, typeof EnsureUserSessionUnlockedResponseSchema>

// TODO(device-session-lock-js): Add the high-level JavaScript lock projection
// after that SDK surface is owner-approved; this port preserves #201's
// gRPC, Rust, CLI, and MCP scope.

/** Ensures an existing selected OS login session is unlocked. */
export async function ensureUserSessionUnlocked(
  connection: AuvConnection,
  options: EnsureUserSessionUnlockedOptions,
): Promise<UserSessionUnlockEffect> {
  const target = 'user' in options && options.user
    ? { case: 'user' as const, value: options.user }
    : 'sessionSelector' in options && options.sessionSelector
      ? { case: 'sessionSelector' as const, value: options.sessionSelector }
      : undefined
  if (target === undefined || ('user' in options && 'sessionSelector' in options)) {
    throw new AuvConfigurationError('exactly one non-empty user or sessionSelector is required')
  }
  const response = await connection.unary(ensureUserSessionUnlockedRpc, { target }, options)
  switch (response.result.case) {
    case 'effect': {
      const effect = response.result.value
      let kind: UserSessionUnlockEffect['kind']
      switch (effect.kind) {
        case DeviceEntryEffectKind.ALREADY_USABLE:
          kind = 'alreadyUsable'
          break
        case DeviceEntryEffectKind.UNLOCKED_EXISTING_SESSION:
          kind = 'unlockedExistingSession'
          break
        default:
          throw new AuvProtocolError(`AUV response returned unsupported Device entry effect ${effect.kind}`)
      }
      if (!effect.user) {
        throw new AuvProtocolError('AUV response omitted EnsureUserSessionUnlockedEffect.user')
      }
      return { kind, sessionSelector: effect.sessionSelector || undefined, user: effect.user }
    }
    case 'error':
      throw deviceEntryError(response.result.value.reason)
    default:
      throw new AuvProtocolError('AUV response omitted EnsureUserSessionUnlockedResponse.result')
  }
}

/** Gets one Device by canonical identity. */
export async function getDevice(connection: AuvConnection, options: GetDeviceOptions): Promise<Device> {
  const response = await connection.unary(getDeviceRpc, { device: { deviceId: options.deviceId } }, options)
  if (response.device === undefined) {
    throw new AuvProtocolError('AUV response omitted GetDeviceResponse.device')
  }
  return device(response.device)
}

/** Gets one current OS login instance by its opaque selector. */
export async function getUserSession(connection: AuvConnection, options: GetUserSessionOptions): Promise<UserSession> {
  if (!options.sessionSelector) {
    throw new AuvConfigurationError('sessionSelector must be non-empty')
  }
  const response = await connection.unary(getUserSessionRpc, { sessionSelector: options.sessionSelector }, options)
  switch (response.result.case) {
    case 'session':
      return userSession(response.result.value)
    case 'error':
      throw deviceEntryError(response.result.value.reason)
    default:
      throw new AuvProtocolError('AUV response omitted GetUserSessionResponse.result')
  }
}

/** Lists Devices visible to the connected caller. */
export async function listDevices(connection: AuvConnection, options: OperationOptions = {}): Promise<readonly Device[]> {
  const response = await connection.unary(listDevicesRpc, {}, options)
  return response.devices.map(device)
}

/** Lists current OS login instances on the connected Device. */
export async function listUserSessions(connection: AuvConnection, options: OperationOptions = {}): Promise<readonly UserSession[]> {
  const response = await connection.unary(listUserSessionsRpc, {}, options)
  switch (response.result.case) {
    case 'list':
      return response.result.value.sessions.map(userSession)
    case 'error':
      throw deviceEntryError(response.result.value.reason)
    default:
      throw new AuvProtocolError('AUV response omitted ListUserSessionsResponse.result')
  }
}

function device(value: ProtoDevice): Device {
  const id = value.ref?.deviceId
  if (id === undefined || id.length === 0) {
    throw new AuvProtocolError('AUV response omitted Device.ref.device_id')
  }
  return {
    id,
    labels: value.labels,
    local: value.local,
    name: value.name,
    platform: devicePlatform(value.platform),
  }
}

function deviceEntryError(reason: ProtoDeviceEntryErrorReason): AuvDeviceEntryError {
  switch (reason) {
    case ProtoDeviceEntryErrorReason.AMBIGUOUS_USER: return new AuvDeviceEntryError('ambiguousUser')
    case ProtoDeviceEntryErrorReason.AUDIT_UNAVAILABLE: return new AuvDeviceEntryError('auditUnavailable')
    case ProtoDeviceEntryErrorReason.CREDENTIAL_REJECTED: return new AuvDeviceEntryError('credentialRejected')
    case ProtoDeviceEntryErrorReason.DISABLED: return new AuvDeviceEntryError('disabled')
    case ProtoDeviceEntryErrorReason.OCCUPIED_DESKTOP: return new AuvDeviceEntryError('occupiedDesktop')
    case ProtoDeviceEntryErrorReason.OUTCOME_UNVERIFIED: return new AuvDeviceEntryError('outcomeUnverified')
    case ProtoDeviceEntryErrorReason.SERVICE_UNAVAILABLE: return new AuvDeviceEntryError('serviceUnavailable')
    case ProtoDeviceEntryErrorReason.STALE_SESSION: return new AuvDeviceEntryError('staleSession')
    case ProtoDeviceEntryErrorReason.SUSPENDED: return new AuvDeviceEntryError('suspended')
    case ProtoDeviceEntryErrorReason.UNENROLLED: return new AuvDeviceEntryError('unenrolled')
    case ProtoDeviceEntryErrorReason.UNSPECIFIED:
      throw new AuvProtocolError('AUV response omitted DeviceEntryError.reason')
    case ProtoDeviceEntryErrorReason.UNAUTHORIZED: return new AuvDeviceEntryError('unauthorized')
    case ProtoDeviceEntryErrorReason.UNSUPPORTED_OS_STATE: return new AuvDeviceEntryError('unsupportedOsState')
    default: return unknownEnum('DeviceEntryError.reason', reason)
  }
}

function devicePlatform(value: ProtoDevicePlatform): DevicePlatform {
  switch (value) {
    case ProtoDevicePlatform.LINUX:
      return 'linux'
    case ProtoDevicePlatform.MACOS:
      return 'macos'
    case ProtoDevicePlatform.UNSPECIFIED:
      return 'unspecified'
    case ProtoDevicePlatform.WINDOWS:
      return 'windows'
    default:
      return unknownEnum('Device.platform', value)
  }
}

function userSession(value: ProtoUserSession): UserSession {
  if (!value.sessionSelector || !value.user) {
    throw new AuvProtocolError('AUV response omitted UserSession.session_selector or user')
  }
  const lockState = userSessionLockState(value.lockState)
  return {
    connectionKind: userSessionConnectionKind(value.connectionKind),
    isLocked: lockState === 'locked',
    isUnlocked: lockState === 'usable',
    lockState,
    seat: value.seat || undefined,
    sessionSelector: value.sessionSelector,
    user: value.user,
  }
}

function userSessionConnectionKind(value: ProtoUserSessionConnectionKind): UserSessionConnectionKind {
  switch (value) {
    case ProtoUserSessionConnectionKind.PHYSICAL:
      return 'physical'
    case ProtoUserSessionConnectionKind.REMOTE:
      return 'remote'
    case ProtoUserSessionConnectionKind.UNSPECIFIED:
      return 'unspecified'
    default:
      return unknownEnum('UserSession.connection_kind', value)
  }
}

function userSessionLockState(value: ProtoUserSessionLockState): UserSessionLockState {
  switch (value) {
    case ProtoUserSessionLockState.LOCKED:
      return 'locked'
    case ProtoUserSessionLockState.UNSPECIFIED:
      throw new AuvProtocolError('AUV response omitted UserSession.lock_state')
    case ProtoUserSessionLockState.UNKNOWN:
      return 'unknown'
    case ProtoUserSessionLockState.USABLE:
      return 'usable'
    default:
      return unknownEnum('UserSession.lock_state', value)
  }
}
