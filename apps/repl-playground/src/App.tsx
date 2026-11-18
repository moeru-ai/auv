import { Tabs } from '@base-ui/react/tabs'
import { Tooltip } from '@base-ui/react/tooltip'
import { useEffect, useState } from 'react'

import { TabBar } from './components/TabBar'
import { AgentWorkbench } from './features/agent/AgentWorkbench'
import { restoreDevices } from './features/device/devices'
import { ScriptWorkbench } from './features/editor/ScriptWorkbench'

export function App() {
  const [workspace, setWorkspace] = useState('agent')

  useEffect(() => {
    void restoreDevices()
  }, [])

  return (
    <Tooltip.Provider delay={300}>
      <Tabs.Root className="bg-surface-1 flex flex-col h-full" onValueChange={setWorkspace} value={workspace}>
        <TabBar tabs={[
          { icon: 'i-ph-chat-circle', label: 'Agent', value: 'agent' },
          { icon: 'i-ph-code', label: 'Script', value: 'script' },
        ]}
        />
        <Tabs.Panel className="flex-1 min-h-0 min-w-0 relative data-[hidden]:hidden" keepMounted value="agent">
          <AgentWorkbench />
        </Tabs.Panel>
        <Tabs.Panel className="flex-1 min-h-0 min-w-0 relative data-[hidden]:hidden" keepMounted value="script">
          <ScriptWorkbench active={workspace === 'script'} />
        </Tabs.Panel>
      </Tabs.Root>
    </Tooltip.Provider>
  )
}
