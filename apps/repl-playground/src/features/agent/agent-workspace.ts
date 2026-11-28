import { createWorkspace } from '../../store'
import { createWorkspaceDesktop } from '../desktop/workspace-desktop'

export const agentWorkspace = createWorkspace('append')
export const agentDesktop = createWorkspaceDesktop(agentWorkspace)
