import type { Backend } from '../../backend/types'

import { create } from 'zustand'

export const useDeviceTarget = create<{ backend: Backend | null, locked: boolean }>(() => ({ backend: null, locked: false }))

export function reserveDevice(): (() => void) | undefined {
  if (useDeviceTarget.getState().locked)
    return undefined
  useDeviceTarget.setState({ locked: true })
  return () => useDeviceTarget.setState({ locked: false })
}
