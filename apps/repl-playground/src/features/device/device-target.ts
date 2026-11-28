import type { Backend, RunBackend, RunOutcomeKind } from '../../backend/types'

import { create } from 'zustand'

export const useDeviceTarget = create<{ backend: Backend | null, locked: boolean }>(() => ({ backend: null, locked: false }))

export function reserveDevice(): (() => void) | undefined {
  if (useDeviceTarget.getState().locked)
    return undefined
  useDeviceTarget.setState({ locked: true })
  return () => useDeviceTarget.setState({ locked: false })
}

export async function withDeviceRun<T>(signal: AbortSignal, execute: (run: null | RunBackend) => Promise<{ outcome: RunOutcomeKind, value: T }>): Promise<T> {
  const run = await useDeviceTarget.getState().backend?.beginRun() ?? null
  let outcome: RunOutcomeKind | undefined
  try {
    const result = await execute(run)
    outcome = result.outcome
    return result.value
  }
  finally {
    await run?.endRun(outcome ?? (signal.aborted ? 'canceled' : 'failed'))
  }
}
