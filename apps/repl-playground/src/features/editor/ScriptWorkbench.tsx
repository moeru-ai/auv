import { useEffect } from 'react'

import { PanelFrame } from '../../components/PanelFrame'
import { session } from '../../runtime/session'
import { Workbench } from '../workbench/Workbench'
import { CodeEditor } from './CodeEditor'
import { DebugBar } from './DebugBar'
import { scriptDesktop, scriptWorkspace } from './script-workspace'

export function ScriptWorkbench({ active }: { active: boolean }) {
  useEffect(() => {
    if (!active)
      return
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
  }, [active])

  return (
    <Workbench active={active} desktop={scriptDesktop} onReset={() => void session.reset()} resetHint="Reset: terminate the worker and clear REPL state" workspace={scriptWorkspace}>
      <PanelFrame title="Script">
        <CodeEditor />
        <DebugBar />
      </PanelFrame>
    </Workbench>
  )
}
