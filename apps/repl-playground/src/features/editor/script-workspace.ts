import { createWorkspace } from '../../store'
import { createWorkspaceDesktop } from '../desktop/workspace-desktop'

export const scriptWorkspace = createWorkspace()
export const { actions, store: usePlayground } = scriptWorkspace

export const scriptDesktop = createWorkspaceDesktop(scriptWorkspace)
