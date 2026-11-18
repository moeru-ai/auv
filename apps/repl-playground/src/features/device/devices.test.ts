import { expect, it, vi } from 'vitest'

import { reserveDevice, useDeviceTarget } from './device-target'

vi.mock('../../backend/auv', () => ({ connectBackend: async () => ({ backend: {} }) }))
vi.mock('../../backend/mock-runner', () => ({ mockRunner: () => ({}) }))

it('allows device initialization to be retried after a running script prevented it', async () => {
  // ROOT CAUSE: the failed initialization Promise was cached for the entire
  // page lifetime, so stopping the script could never make a retry succeed.
  const { restoreDevices, useDevices } = await import('./devices')
  const release = reserveDevice()!
  try {
    await expect(restoreDevices()).rejects.toThrow('Stop the current run')
  }
  finally {
    release()
  }
  await restoreDevices()
  expect(useDeviceTarget.getState().backend).not.toBeNull()
  expect(useDevices.getState().active).toEqual({ connectionId: 'mock', deviceId: 'mock' })
})
