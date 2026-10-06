import { Tabs } from '@base-ui/react/tabs'
import { Tooltip } from '@base-ui/react/tooltip'
import { useEffect } from 'react'
import { Group, Panel, Separator } from 'react-resizable-panels'

import { ActivityBar } from './components/ActivityBar'
import { CodeEditor } from './components/CodeEditor'
import { ConsolePanel } from './components/ConsolePanel'
import { DebugBar } from './components/DebugBar'
import { DesktopCanvas } from './components/DesktopCanvas'
import { Inspector, VariablesPanel } from './components/Inspector'
import { PinLayer } from './components/PinLayer'
import { CallList } from './components/Timeline'
import { TimeStrip } from './components/TimeStrip'
import { connectMockDesktop, connectSaved } from './connection'
import { session } from './runtime/session'
import { usePlayground } from './store'

export function App() {
  useEffect(() => {
    // Debugger keys work regardless of focus (the editor may not be focused
    // after clicking a toolbar button).
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'F8')
        session.resume('continue')
      else if (event.key === 'F10')
        session.resume('step')
      else if (event.key === 'F5' && event.shiftKey)
        session.stop()
      else
        return
      event.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    // First visit: start on the mock desktop so the playground works immediately.
    void connectSaved().then(async (connected) => {
      if (!connected)
        await connectMockDesktop()
    })
  }, [])

  return (
    <Tooltip.Provider delay={300}>
      <div className="flex h-full">
        <ActivityBar />
        <div className="flex flex-1 flex-col min-w-0">
          <div className="flex-1 min-h-0">
            <Group orientation="vertical">
              <Panel defaultSize="70%" minSize="30%">
                <Group orientation="horizontal">
                  <Panel defaultSize="38%" minSize="15%">
                    <PanelFrame title="Desktop">
                      <DesktopCanvas />
                    </PanelFrame>
                  </Panel>
                  <ResizeHandle orientation="horizontal" />
                  <Panel defaultSize="40%" minSize="20%">
                    <PanelFrame title="Script">
                      <CodeEditor />
                      <DebugBar />
                    </PanelFrame>
                  </Panel>
                  <ResizeHandle orientation="horizontal" />
                  <Panel defaultSize="22%" minSize="12%">
                    <Inspector />
                  </Panel>
                </Group>
              </Panel>
              <ResizeHandle orientation="vertical" />
              <Panel defaultSize="30%" minSize="10%">
                <BottomTabs />
              </Panel>
            </Group>
          </div>
          <TimeStrip />
        </div>
      </div>
      <PinLayer />
    </Tooltip.Provider>
  )
}

function BottomTabs() {
  const calls = usePlayground(state => state.calls.length)
  const logs = usePlayground(state => state.logs.length)
  return (
    <Tabs.Root className="bg-surface-1 flex flex-col h-full" defaultValue="calls">
      <Tabs.List className="px-2 border-b border-line flex shrink-0 gap-1 h-9 items-center">
        {([
          ['calls', 'Calls', calls],
          ['console', 'Console', logs],
          ['variables', 'Variables', undefined],
        ] as const).map(([value, label, count]) => (
          <Tabs.Tab
            className="text-[12.5px] text-fg-muted font-medium px-2.5 rounded-md flex gap-1.5 h-7 cursor-pointer items-center data-[active]:(text-fg bg-surface-2) hover:text-fg"
            key={value}
            value={value}
          >
            {label}
            {Boolean(count) && <span className="text-[10.5px] text-fg-subtle font-mono px-1 rounded bg-surface-0">{count}</span>}
          </Tabs.Tab>
        ))}
      </Tabs.List>
      <Tabs.Panel className="flex-1 min-h-0" value="calls"><CallList /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="console"><ConsolePanel /></Tabs.Panel>
      <Tabs.Panel className="flex-1 min-h-0" value="variables"><VariablesPanel /></Tabs.Panel>
    </Tabs.Root>
  )
}

function PanelFrame({ children, title }: { children: React.ReactNode, title: string }) {
  return (
    <section className="bg-surface-1 flex flex-col h-full">
      <div className="px-3 border-b border-line flex shrink-0 h-9 items-center">
        <span className="panel-title">{title}</span>
      </div>
      <div className="flex-1 min-h-0 relative">{children}</div>
    </section>
  )
}

function ResizeHandle({ orientation }: { orientation: 'horizontal' | 'vertical' }) {
  return (
    <Separator
      className={`bg-line transition-colors data-[separator=active]:bg-accent hover:bg-accent ${orientation === 'horizontal' ? 'w-px' : 'h-px'}`}
    />
  )
}
