import { isWindows } from 'std-env'
import { describe, expect, it } from 'vitest'

import {
  connect,
  createPairingToken,
  listDevices,
  pairDevice,
} from '../../node/index'
import { setupAuvDaemon } from '../../tutils/auv-daemon'

describe.skipIf(isWindows)('pairing operations against an AUV daemon', () => {
  it('creates a one-time token with an explicit TTL', async () => {
    const daemon = await setupAuvDaemon()

    try {
      const owner = await connect({ endpoint: daemon.ownerSocket, local: true, transport: 'unix' })
      const issuedAfter = Date.now()
      const token = await createPairingToken(owner, { ttlMs: 60_000 })
      await owner.close()

      expect(token.value).toMatch(/^[0-9a-f]{32}$/u)
      expect(token.expiresAt?.getTime()).toBeGreaterThanOrEqual(issuedAfter + 59_000)
      expect(token.expiresAt?.getTime()).toBeLessThanOrEqual(Date.now() + 60_000)
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)

  it('enrolls and authenticates a caller-supplied Device identity', async () => {
    const daemon = await setupAuvDaemon()

    try {
      const owner = await connect({ endpoint: daemon.ownerSocket, local: true, transport: 'unix' })
      const token = await createPairingToken(owner)
      await owner.close()

      const bootstrap = await connect({ endpoint: daemon.remoteEndpoint, transport: 'http' })
      const enrollment = await pairDevice(bootstrap, { deviceId: 'browser-device', label: 'Browser controller', token })
      await bootstrap.close()

      expect(enrollment.deviceId).toBe('browser-device')
      expect(enrollment.credential).toMatch(/^[0-9a-f]{64}$/u)

      const paired = await connect({ credential: enrollment.credential, endpoint: daemon.remoteEndpoint, transport: 'http' })
      const devices = await listDevices(paired)
      await paired.close()

      expect(devices).toContainEqual(expect.objectContaining({ local: true }))
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)
})
