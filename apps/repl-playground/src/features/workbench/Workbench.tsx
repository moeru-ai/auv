import type { ReactNode } from 'react'

import type { Workspace } from '../../store'
import type { createWorkspaceDesktop } from '../desktop/workspace-desktop'

import { useEffect } from 'react'
import { Group, Panel } from 'react-resizable-panels'

import { PanelFrame } from '../../components/PanelFrame'
import { ResizeHandle } from '../../components/ResizeHandle'
import { DesktopCanvas } from '../desktop/DesktopCanvas'
import { Inspector } from '../inspect/Inspector'
import { PinLayer } from '../inspect/PinLayer'
import { BottomPanel } from '../run/BottomPanel'
import { TimeStrip } from '../run/TimeStrip'
import { ActivityBar } from './ActivityBar'
import { StatusBar } from './StatusBar'
import { WorkspaceContext } from './workspace-context'

export function Workbench({ active, children, desktop, onReset, resetHint, workspace }: {
  active: boolean
  children: ReactNode
  desktop: ReturnType<typeof createWorkspaceDesktop>
  onReset: () => void
  resetHint: string
  workspace: Workspace
}) {
  useEffect(() => active ? desktop.watchDevice() : undefined, [active, desktop])

  return (
    <WorkspaceContext value={workspace}>
      <div className="flex flex-col h-full">
        <div className="flex flex-1 min-h-0">
          <ActivityBar onReset={onReset} resetHint={resetHint} setLive={desktop.setLive} />
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
                    <Panel defaultSize="40%" minSize="20%">{children}</Panel>
                    <ResizeHandle orientation="horizontal" />
                    <Panel defaultSize="22%" minSize="12%">
                      <Inspector refreshAxTree={desktop.refreshAxTree} />
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
    </WorkspaceContext>
  )
}
