import { Workbench } from '../workbench/Workbench'
import { resetAgentExecution } from './agent-execution'
import { agentDesktop, agentWorkspace } from './agent-workspace'
import { AgentTab } from './AgentTab'

export function AgentWorkbench() {
  return (
    <Workbench desktop={agentDesktop} onReset={resetAgentExecution} resetHint="Cancel execution and clear Agent run history" workspace={agentWorkspace}>
      <AgentTab />
    </Workbench>
  )
}
