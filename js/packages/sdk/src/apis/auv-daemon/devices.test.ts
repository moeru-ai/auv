import type { Transport } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { BinaryWriter, WireType } from '@bufbuild/protobuf/wire'
import { anyPack, AnySchema } from '@bufbuild/protobuf/wkt'
import { describe, expect, it } from 'vitest'

import {
  DeviceEntryEffectKind,
  DeviceEntryErrorReason,
  DeviceEntryErrorSchema,
  EnsureUserSessionUnlockedRequestSchema,
  EnsureUserSessionUnlockedResponseSchema,
  GetUserSessionResponseSchema,
  ListUserSessionsResponseSchema,
  UserSessionConnectionKind,
  UserSessionLockState,
} from '../../gen/auv/api/daemon/v1/device_pb'
import { AuvConnection } from '../../transport/connection'
import { AuvProtocolError, AuvRpcError } from '../../transport/errors'
import { createAuv } from '../auv/client'
import { AuvDeviceEntryError, ensureUserSessionUnlocked, listUserSessions } from './devices'

function connectionFor(response: Uint8Array, onCall?: (call: Parameters<Transport['unary']>[0]) => void): AuvConnection {
  return new AuvConnection({
    close() {},
    async connect() {},
    async duplex() { throw new Error('unexpected duplex call') },
    async unary(call) {
      onCall?.(call)
      return response
    },
  })
}

/** A daemon's failing status for one reason: `google.rpc.Status` details with one `DeviceEntryError`. */
function entryStatus(reason: DeviceEntryErrorReason): AuvRpcError {
  const detail = anyPack(DeviceEntryErrorSchema, create(DeviceEntryErrorSchema, { reason }))
  const details = new BinaryWriter().tag(3, WireType.LengthDelimited).bytes(toBinary(AnySchema, detail)).finish()
  return new AuvRpcError(9, `Device entry failed: ${DeviceEntryErrorReason[reason]}`, undefined, details)
}

function failingConnection(error: unknown, onCall?: () => void): AuvConnection {
  return new AuvConnection({
    close() {},
    async connect() {},
    async duplex() { throw new Error('unexpected duplex call') },
    async unary() {
      onCall?.()
      throw error
    },
  })
}

describe('device user sessions', () => {
  it('derives both lock predicates from the three-state OS observation', async () => {
    const response = toBinary(ListUserSessionsResponseSchema, create(ListUserSessionsResponseSchema, {
      list: {
        sessions: [
          { connectionKind: UserSessionConnectionKind.PHYSICAL, lockState: UserSessionLockState.LOCKED, sessionSelector: 'a', user: 'neko' },
          { connectionKind: UserSessionConnectionKind.REMOTE, lockState: UserSessionLockState.USABLE, sessionSelector: 'b', user: 'neko' },
          { lockState: UserSessionLockState.UNKNOWN, sessionSelector: 'c', user: 'guest' },
        ],
      },
    }))
    const connection = connectionFor(response, call => expect(call.method).toBe('/auv.api.daemon.v1.DeviceService/ListUserSessions'))

    await expect(listUserSessions(connection)).resolves.toEqual([
      { connectionKind: 'physical', isLocked: true, isUnlocked: false, lockState: 'locked', seat: undefined, sessionSelector: 'a', user: 'neko' },
      { connectionKind: 'remote', isLocked: false, isUnlocked: true, lockState: 'usable', seat: undefined, sessionSelector: 'b', user: 'neko' },
      { connectionKind: 'unspecified', isLocked: false, isUnlocked: false, lockState: 'unknown', seat: undefined, sessionSelector: 'c', user: 'guest' },
    ])
  })

  it('rejects an omitted lock observation instead of treating it as unknown', async () => {
    const response = toBinary(ListUserSessionsResponseSchema, create(ListUserSessionsResponseSchema, {
      list: { sessions: [{ sessionSelector: 'a', user: 'neko' }] },
    }))

    await expect(listUserSessions(connectionFor(response))).rejects.toEqual(
      new AuvProtocolError('AUV response omitted UserSession.lock_state'),
    )
  })

  it('exposes getUserSession through the bound client', async () => {
    const response = toBinary(GetUserSessionResponseSchema, create(GetUserSessionResponseSchema, {
      session: { lockState: UserSessionLockState.LOCKED, seat: 'console', sessionSelector: 'wts:1', user: 'neko' },
    }))
    const connection = connectionFor(response, call => expect(call.method).toBe('/auv.api.daemon.v1.DeviceService/GetUserSession'))

    await expect(createAuv(connection).devices.getUserSession({ sessionSelector: 'wts:1' })).resolves.toMatchObject({
      isLocked: true,
      seat: 'console',
      sessionSelector: 'wts:1',
    })
  })

  it('submits exactly one selector and returns the verified existing-session effect', async () => {
    const response = toBinary(EnsureUserSessionUnlockedResponseSchema, create(EnsureUserSessionUnlockedResponseSchema, {
      effect: { kind: DeviceEntryEffectKind.UNLOCKED_EXISTING_SESSION, sessionSelector: 'wts:1', user: 'neko' },
    }))
    const connection = connectionFor(response, (call) => {
      expect(call.method).toBe('/auv.api.daemon.v1.DeviceService/EnsureUserSessionUnlocked')
      expect(fromBinary(EnsureUserSessionUnlockedRequestSchema, call.body).target).toEqual({ case: 'sessionSelector', value: 'wts:1' })
    })

    await expect(ensureUserSessionUnlocked(connection, { sessionSelector: 'wts:1' })).resolves.toEqual({
      kind: 'unlockedExistingSession',
      sessionSelector: 'wts:1',
      user: 'neko',
    })
  })

  it('rejects invalid selectors before dispatch and exposes fixed target errors', async () => {
    let calls = 0
    const connection = failingConnection(entryStatus(DeviceEntryErrorReason.UNENROLLED), () => {
      calls += 1
    })

    await expect(ensureUserSessionUnlocked(connection, { user: '' })).rejects.toMatchObject({ name: 'AuvConfigurationError' })
    expect(calls).toBe(0)
    await expect(ensureUserSessionUnlocked(connection, { user: 'neko' })).rejects.toEqual(new AuvDeviceEntryError('unenrolled'))
    expect(calls).toBe(1)
  })

  // Bytes of `auv_api_proto::device_entry::status(Unenrolled).details()`;
  // the Rust test `unenrolled_status_matches_the_shared_fixture` pins them.
  const unenrolledStatusDetails = Uint8Array.from(
    '0809123944657669636520656e747279206661696c65643a204445564943455f454e5452595f4552524f525f524541534f4e5f554e454e524f4c4c45441a3c0a36747970652e676f6f676c65617069732e636f6d2f6175762e6170692e6461656d6f6e2e76312e446576696365456e7472794572726f7212020802'.match(/../g)!.map(byte => Number.parseInt(byte, 16)),
  )

  it('decodes a typed reason from the status details', async () => {
    const status = new AuvRpcError(9, 'Device entry failed: DEVICE_ENTRY_ERROR_REASON_UNENROLLED', undefined, unenrolledStatusDetails)

    await expect(ensureUserSessionUnlocked(failingConnection(status), { user: 'neko' })).rejects.toEqual(new AuvDeviceEntryError('unenrolled'))
    await expect(listUserSessions(failingConnection(status))).rejects.toEqual(new AuvDeviceEntryError('unenrolled'))
    await expect(createAuv(failingConnection(status)).devices.getUserSession({ sessionSelector: 'wts:1' })).rejects.toEqual(new AuvDeviceEntryError('unenrolled'))
  })

  it('passes other RPC failures through unchanged', async () => {
    const plain = new AuvRpcError(3, 'exactly one non-empty user or session_selector is required')
    const foreign = new AuvRpcError(13, 'internal', undefined, Uint8Array.of(0xFF, 0xFF))

    await expect(ensureUserSessionUnlocked(failingConnection(plain), { user: 'neko' })).rejects.toBe(plain)
    await expect(ensureUserSessionUnlocked(failingConnection(foreign), { user: 'neko' })).rejects.toBe(foreign)
  })

  it('exposes an incompatible target host as a typed entry error', async () => {
    const status = entryStatus(DeviceEntryErrorReason.HOST_INCOMPATIBLE)

    await expect(ensureUserSessionUnlocked(failingConnection(status), { user: 'neko' })).rejects.toEqual(new AuvDeviceEntryError('hostIncompatible'))
  })

  it('rejects the reserved signed-out effect from this locked-session API', async () => {
    const response = toBinary(EnsureUserSessionUnlockedResponseSchema, create(EnsureUserSessionUnlockedResponseSchema, {
      effect: { kind: DeviceEntryEffectKind.SIGNED_IN_NEW_SESSION, user: 'neko' },
    }))
    const connection = connectionFor(response)

    await expect(ensureUserSessionUnlocked(connection, { user: 'neko' })).rejects.toBeInstanceOf(AuvProtocolError)
  })
})
