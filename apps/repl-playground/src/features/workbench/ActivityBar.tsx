import { useEffect } from 'react'
import { create } from 'zustand'

import { IconButton } from '../../components/IconButton'
import { useWorkspace, useWorkspaceState } from './workspace-context'

const THEME_KEY = 'auv-playground:theme'

const useTheme = create<{ theme: 'dark' | 'light' | 'system' }>(() => {
  try {
    const theme = localStorage.getItem(THEME_KEY)
    return { theme: theme === 'dark' || theme === 'light' ? theme : 'system' }
  }
  catch {
    return { theme: 'system' }
  }
})

/** Left rail for global actions: live view, pins, REPL reset, theme. The Device lives in the status bar. */
export function ActivityBar({ onReset, resetHint, setLive }: { onReset: () => void, resetHint: string, setLive: (live: boolean) => void }) {
  const { actions } = useWorkspace()
  const live = useWorkspaceState(state => state.live)
  const connected = useWorkspaceState(state => state.backend !== null)
  const pins = useWorkspaceState(state => state.pins.length)
  const theme = useTheme(state => state.theme)

  useEffect(() => {
    if (theme === 'system')
      delete document.documentElement.dataset.theme
    else
      document.documentElement.dataset.theme = theme
    try {
      localStorage.setItem(THEME_KEY, theme)
    }
    catch {}
  }, [theme])

  return (
    <nav className="py-2.5 border-r border-line bg-surface-1 flex shrink-0 flex-col gap-1.5 w-12 items-center">
      <div className="mb-2 flex size-9 items-center justify-center" title="AUV Playground">
        <span className="i-ph-crosshair-simple text-[20px] text-accent" />
      </div>
      <IconButton
        active={live}
        disabled={!connected}
        hint={live ? 'Stop live desktop view' : 'Live desktop view'}
        icon="i-ph-broadcast"
        onClick={() => setLive(!live)}
        side="right"
      />
      <IconButton
        disabled={pins === 0}
        hint={pins === 0 ? 'No pinned cards' : `Close ${pins} pinned card${pins === 1 ? '' : 's'}`}
        icon="i-ph-broom"
        onClick={actions.clearPins}
        side="right"
      />
      <div className="flex-1" />
      <IconButton
        hint={resetHint}
        icon="i-ph-arrow-counter-clockwise"
        onClick={onReset}
        side="right"
      />
      <IconButton
        hint={`Theme: ${theme}`}
        icon={theme === 'dark' ? 'i-ph-moon' : theme === 'light' ? 'i-ph-sun' : 'i-ph-desktop'}
        onClick={() => useTheme.setState({ theme: theme === 'system' ? 'dark' : theme === 'dark' ? 'light' : 'system' })}
        side="right"
      />
    </nav>
  )
}
