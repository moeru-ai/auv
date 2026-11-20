import { Workbench } from '../workbench/Workbench'
import { resetAgentExecution } from './agent-execution'
import { agentDesktop, agentWorkspace } from './agent-workspace'
import { AgentTab } from './AgentTab'

export function AgentWorkbench({ active }: { active: boolean }) {
  return (
    <Workbench active={active} desktop={agentDesktop} onReset={resetAgentExecution} resetHint="Cancel execution and clear Agent run history" workspace={agentWorkspace}>
      <AgentTab />
    </Workbench>
  )
}
