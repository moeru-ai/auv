import type { Transport } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import {
  DeviceEntryEffectKind,
  DeviceEntryErrorReason,
  EnsureUserSessionUnlockedRequestSchema,
  EnsureUserSessionUnlockedResponseSchema,
  GetUserSessionResponseSchema,
  ListUserSessionsResponseSchema,
  UserSessionConnectionKind,
  UserSessionLockState,
} from '../../gen/auv/api/daemon/v1/device_pb'
import { AuvConnection } from '../../transport/connection'
import { AuvProtocolError } from '../../transport/errors'
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

describe('device user sessions', () => {
  it('derives both lock predicates from the three-state OS observation', async () => {
    const response = toBinary(ListUserSessionsResponseSchema, create(ListUserSessionsResponseSchema, {
      result: {
        case: 'list',
        value: {
          sessions: [
            { connectionKind: UserSessionConnectionKind.PHYSICAL, lockState: UserSessionLockState.LOCKED, sessionSelector: 'a', user: 'neko' },
            { connectionKind: UserSessionConnectionKind.REMOTE, lockState: UserSessionLockState.USABLE, sessionSelector: 'b', user: 'neko' },
            { lockState: UserSessionLockState.UNKNOWN, sessionSelector: 'c', user: 'guest' },
          ],
        },
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
      result: { case: 'list', value: { sessions: [{ sessionSelector: 'a', user: 'neko' }] } },
    }))

    await expect(listUserSessions(connectionFor(response))).rejects.toEqual(
      new AuvProtocolError('AUV response omitted UserSession.lock_state'),
    )
  })

  it('exposes getUserSession through the bound client', async () => {
    const response = toBinary(GetUserSessionResponseSchema, create(GetUserSessionResponseSchema, {
      result: { case: 'session', value: { lockState: UserSessionLockState.LOCKED, seat: 'console', sessionSelector: 'wts:1', user: 'neko' } },
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
      result: { case: 'effect', value: { kind: DeviceEntryEffectKind.UNLOCKED_EXISTING_SESSION, sessionSelector: 'wts:1', user: 'neko' } },
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
    const response = toBinary(EnsureUserSessionUnlockedResponseSchema, create(EnsureUserSessionUnlockedResponseSchema, {
      result: { case: 'error', value: { reason: DeviceEntryErrorReason.UNENROLLED } },
    }))
    const connection = connectionFor(response, () => {
      calls += 1
    })

    await expect(ensureUserSessionUnlocked(connection, { user: '' })).rejects.toMatchObject({ name: 'AuvConfigurationError' })
    expect(calls).toBe(0)
    await expect(ensureUserSessionUnlocked(connection, { user: 'neko' })).rejects.toEqual(new AuvDeviceEntryError('unenrolled'))
    expect(calls).toBe(1)
  })

  it('rejects the reserved signed-out effect from this locked-session API', async () => {
    const response = toBinary(EnsureUserSessionUnlockedResponseSchema, create(EnsureUserSessionUnlockedResponseSchema, {
      result: { case: 'effect', value: { kind: DeviceEntryEffectKind.SIGNED_IN_NEW_SESSION, user: 'neko' } },
    }))
    const connection = connectionFor(response)

    await expect(ensureUserSessionUnlocked(connection, { user: 'neko' })).rejects.toBeInstanceOf(AuvProtocolError)
  })
})
