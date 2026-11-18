import type { PlaygroundState, Workspace } from '../../store'

import { createContext, use } from 'react'
import { useStore } from 'zustand'

export const WorkspaceContext = createContext<null | Workspace>(null)

export function useWorkspace(): Workspace {
  const workspace = use(WorkspaceContext)
  if (!workspace)
    throw new Error('A workbench component needs a workspace.')
  return workspace
}

export function useWorkspaceState<T>(select: (state: PlaygroundState) => T): T {
  return useStore(useWorkspace().store, select)
}
