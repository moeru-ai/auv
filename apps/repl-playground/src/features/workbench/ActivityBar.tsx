import { useEffect, useState } from 'react'

import { IconButton } from '../../components/IconButton'
import { session } from '../../runtime/session'
import { actions, usePlayground } from '../../store'
import { resetMockDesktop } from '../device/devices'

const THEME_KEY = 'auv-playground:theme'

/** Left rail for global actions: live view, pins, REPL reset, theme. The Device lives in the status bar. */
export function ActivityBar() {
  const live = usePlayground(state => state.live)
  const connected = usePlayground(state => state.backend !== null)
  const pins = usePlayground(state => state.pins.length)
  const [theme, setTheme] = useState<'dark' | 'light' | 'system'>(() => {
    try {
      return (localStorage.getItem(THEME_KEY) as 'dark' | 'light' | null) ?? 'system'
    }
    catch {
      return 'system'
    }
  })

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
        onClick={() => session.setLive(!live)}
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
        hint="Reset: terminate the worker, clear REPL state, and restore the mock desktop"
        icon="i-ph-arrow-counter-clockwise"
        onClick={() => {
          session.reset()
          // The mock desktop keeps state across runs like a real one; reset restores it.
          void resetMockDesktop()
        }}
        side="right"
      />
      <IconButton
        hint={`Theme: ${theme}`}
        icon={theme === 'dark' ? 'i-ph-moon' : theme === 'light' ? 'i-ph-sun' : 'i-ph-desktop'}
        onClick={() => setTheme(theme === 'system' ? 'dark' : theme === 'dark' ? 'light' : 'system')}
        side="right"
      />
    </nav>
  )
}
