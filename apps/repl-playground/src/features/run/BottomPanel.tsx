import { Tabs } from '@base-ui/react/tabs'

import { TabBar } from '../../components/TabBar'
import { usePlayground } from '../../store'
import { VariablesPanel } from '../inspect/Inspector'
import { CallList } from './CallList'
import { ConsolePanel } from './ConsolePanel'

/** Run output under the editor: binding calls, console and variables. */
export function BottomPanel() {
  const calls = usePlayground(state => state.calls.length)
  const logs = usePlayground(state => state.logs.length)
  return (
    <Tabs.Root className="bg-surface-1 flex flex-col h-full" defaultValue="calls">
      <TabBar
        tabs={[
          { count: calls, icon: 'i-ph-list-checks', label: 'Calls', value: 'calls' },
          { count: logs, icon: 'i-ph-terminal-window', label: 'Console', value: 'console' },
          { icon: 'i-ph-brackets-square', label: 'Variables', value: 'variables' },
        ]}
      />
      <Tabs.Panel className="flex-1 min-h-0" value="calls"><CallList /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="console"><ConsolePanel /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="variables"><VariablesPanel /></Tabs.Panel>
    </Tabs.Root>
  )
}
