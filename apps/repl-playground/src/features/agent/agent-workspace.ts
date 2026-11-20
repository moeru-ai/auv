import { createWorkspace } from '../../store'
import { createWorkspaceDesktop } from '../desktop/workspace-desktop'

export const agentWorkspace = createWorkspace()
export const agentDesktop = createWorkspaceDesktop(agentWorkspace)
