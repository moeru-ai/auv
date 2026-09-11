import { isWindows } from 'std-env'
import { describe, expect, it } from 'vitest'

import { setupPairedAuvDaemon } from '../tutils/auv-daemon'
import { listDevices } from './index'

describe.skipIf(isWindows)('public Device operations against an AUV daemon', () => {
  it('lists Devices through an authenticated HTTP connection', async () => {
    const daemon = await setupPairedAuvDaemon('listed-device')
    try {
      await expect(listDevices(daemon.connection)).resolves.toContainEqual(expect.objectContaining({
        id: daemon.localDeviceId,
        local: true,
      }))
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)
})
