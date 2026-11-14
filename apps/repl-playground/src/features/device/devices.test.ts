import { expect, it, vi } from 'vitest'

const runtime = vi.hoisted(() => ({ activateBackend: vi.fn(), session: { running: true } }))
vi.mock('../../runtime/session', () => runtime)
vi.mock('../../backend/auv', () => ({ connectBackend: async () => ({ backend: {} }) }))
vi.mock('../../backend/mock-runner', () => ({ mockRunner: () => ({}) }))

it('allows device initialization to be retried after a running script prevented it', async () => {
  // ROOT CAUSE: the failed initialization Promise was cached for the entire
  // page lifetime, so stopping the script could never make a retry succeed.
  const { restoreDevices, useDevices } = await import('./devices')
  await expect(restoreDevices()).rejects.toThrow('Stop the current run')
  runtime.session.running = false
  await restoreDevices()
  expect(runtime.activateBackend).toHaveBeenCalledOnce()
  expect(useDevices.getState().active).toEqual({ connectionId: 'mock', deviceId: 'mock' })
})
