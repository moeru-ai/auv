import { Tooltip } from '@base-ui/react/tooltip'
import { useEffect } from 'react'
import { Group, Panel } from 'react-resizable-panels'

import { PanelFrame } from './components/PanelFrame'
import { ResizeHandle } from './components/ResizeHandle'
import { DesktopCanvas } from './features/desktop/DesktopCanvas'
import { restoreDevices } from './features/device/devices'
import { CodeEditor } from './features/editor/CodeEditor'
import { DebugBar } from './features/editor/DebugBar'
import { Inspector } from './features/inspect/Inspector'
import { PinLayer } from './features/inspect/PinLayer'
import { BottomPanel } from './features/run/BottomPanel'
import { TimeStrip } from './features/run/TimeStrip'
import { ActivityBar } from './features/workbench/ActivityBar'
import { StatusBar } from './features/workbench/StatusBar'
import { session } from './runtime/session'

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
    void restoreDevices()
  }, [])

  return (
    <Tooltip.Provider delay={300}>
      <div className="flex flex-col h-full">
        <div className="flex flex-1 min-h-0">
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
                  <BottomPanel />
                </Panel>
              </Group>
            </div>
            <TimeStrip />
          </div>
        </div>
        <StatusBar />
      </div>
      <PinLayer />
    </Tooltip.Provider>
  )
}
