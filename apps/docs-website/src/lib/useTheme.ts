import type { ThemeName } from '../theme'

import { useEffect, useState } from 'react'

import { currentTheme, THEMES } from '../theme'

/** requestAnimationFrame clock in seconds; `playing=false` freezes it. */
export function useClock(playing: boolean, onTick: (dt: number) => void) {
  useEffect(() => {
    if (!playing)
      return
    let last = performance.now()
    let raf = 0
    const loop = (now: number) => {
      onTick(Math.min(0.1, (now - last) / 1000))
      last = now
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [playing])
}

/** Follows ?theme= or the system appearance; returns a setter for manual toggles. */
export function useTheme() {
  const [name, setName] = useState<ThemeName>(currentTheme)
  useEffect(() => {
    const mq = matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => setName(currentTheme())
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])
  useEffect(() => {
    document.documentElement.dataset.theme = name
  }, [name])
  return [THEMES[name], setName] as const
}
