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

it('rejects connection changes and mock reset while execution holds the device', async () => {
  const { activateDevice, addConnection, disconnect, forgetConnection, MOCK_ID, resetMockDesktop, useDevices } = await import('./devices')
  await activateDevice(MOCK_ID, MOCK_ID)
  const backend = useDeviceTarget.getState().backend
  const devices = useDevices.getState()
  const release = reserveDevice()!
  try {
    await expect(addConnection({ endpoint: 'http://localhost:9847' })).rejects.toThrow('Stop the current run')
    await expect(disconnect('saved-daemon')).rejects.toThrow('Stop the current run')
    await expect(forgetConnection('saved-daemon')).rejects.toThrow('Stop the current run')
    await expect(resetMockDesktop()).rejects.toThrow('Stop the current run')
    expect(useDeviceTarget.getState().backend).toBe(backend)
    expect(useDevices.getState()).toBe(devices)
  }
  finally {
    release()
  }
})
