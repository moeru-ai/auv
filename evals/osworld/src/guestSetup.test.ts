import { afterEach, describe, expect, it, vi } from 'vitest'

import { installRuntimeLibraries } from './guestSetup'

function result(overrides: Partial<{
  error: string
  output: string
  returncode: number
  status: string
}> = {}) {
  return {
    error: '',
    output: '',
    returncode: 0,
    status: 'success',
    ...overrides,
  }
}

describe('installRuntimeLibraries', () => {
  const context = { caseId: 'case-1', podName: 'pod-1' }

  afterEach(() => {
    vi.useRealTimers()
  })

  it('retries while packagekit temporarily owns an apt lock', async () => {
    // ROOT CAUSE:
    //
    // If packagekitd still owns the apt lists lock during VM startup, the
    // one-shot apt command failed even though the lock was released shortly
    // afterward. The retry keeps transient lock contention inside setup.
    vi.useFakeTimers()
    const locked = result({
      error: 'E: Could not get lock /var/lib/apt/lists/lock. It is held by process 1213 (packagekitd)',
      returncode: 100,
      status: 'error',
    })
    const installed = result({ output: 'installed' })
    const execute = vi.fn()
      .mockResolvedValueOnce(locked)
      .mockResolvedValueOnce(installed)

    const installation = installRuntimeLibraries(execute, new AbortController().signal, context)
    await vi.advanceTimersByTimeAsync(2_000)

    await expect(installation).resolves.toEqual(installed)
    expect(execute).toHaveBeenCalledTimes(2)
  })

  it('does not retry unrelated apt failures', async () => {
    const failure = result({
      error: 'E: Unable to locate package libdoes-not-exist',
      returncode: 100,
      status: 'error',
    })
    const execute = vi.fn().mockResolvedValue(failure)

    await expect(installRuntimeLibraries(execute, new AbortController().signal, context)).resolves.toEqual(failure)
    expect(execute).toHaveBeenCalledOnce()
  })
})
