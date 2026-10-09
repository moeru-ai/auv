// Tap or click anywhere during the intro to skip to the end state: the page
// fades to its background, the caller jumps the film to the handoff, and the
// cover fades back out over the hero's own entrance. Links and buttons (nav)
// keep working and don't count as a skip.

import { useEffect, useRef, useState } from 'react'

/** Fade-to-cover time; keep in sync with `.skip-cover.on` in styles.css. */
const COVER_MS = 220

/** Returns whether the cover is up; render `<div className={`skip-cover ${on ? 'on' : ''}`} />`. */
export function useTapToSkip(intro: boolean, skip: () => void) {
  const [cover, setCover] = useState(false)
  const skipRef = useRef(skip)
  skipRef.current = skip

  useEffect(() => {
    if (!intro)
      return
    let timer = 0
    const onTap = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element | null)?.closest('a, button'))
        return
      removeEventListener('pointerup', onTap)
      setCover(true)
      timer = window.setTimeout(() => {
        skipRef.current()
        setCover(false)
      }, COVER_MS)
    }
    addEventListener('pointerup', onTap)
    return () => {
      removeEventListener('pointerup', onTap)
      clearTimeout(timer)
    }
  }, [intro])

  return cover
}
