// "Texts reveal" from transitions.dev (texts-reveal.md in the transitions-dev
// package): lines rise into view with a staggered blur, and leave with one
// quiet fade so a swap never replays the reveal backwards.
//
// Every slide sits in the same grid cell; only `active` is shown. Switching
// fades the old slide out (200 ms) while the new one plays its entrance. The
// CSS lives in styles.css under `.t-stagger`.

import type { ReactNode } from 'react'

import { useEffect, useRef, useState } from 'react'

export interface RevealSlide {
  key: string
  lines: ReactNode[]
}

export function TextsReveal({ active, className = '', slides }: { active: null | string, className?: string, slides: RevealSlide[] }) {
  const [hiding, setHiding] = useState<null | string>(null)
  const last = useRef(active)
  useEffect(() => {
    if (last.current === active)
      return
    const prev = last.current
    last.current = active
    setHiding(prev)
    const id = setTimeout(() => setHiding(h => (h === prev ? null : h)), 200)
    return () => clearTimeout(id)
  }, [active])

  return (
    <div className={`t-reveal ${className}`}>
      {slides.map(s => (
        <Slide key={s.key} lines={s.lines} state={s.key === active ? 'shown' : s.key === hiding ? 'hiding' : 'idle'} />
      ))}
    </div>
  )
}

function Slide({ lines, state }: { lines: ReactNode[], state: 'hiding' | 'idle' | 'shown' }) {
  // The entrance needs one painted frame in the resting (lowered, blurred)
  // state before `.is-shown`, the React form of the snippet's forced reflow.
  const [on, setOn] = useState(false)
  // Lines remount on every entrance so their own animations (the strike) replay.
  const [round, setRound] = useState(0)
  useEffect(() => {
    if (state !== 'shown') {
      setOn(false)
      return
    }
    setRound(r => r + 1)
    let r2 = 0
    const r1 = requestAnimationFrame(() => {
      r2 = requestAnimationFrame(() => setOn(true))
    })
    return () => {
      cancelAnimationFrame(r1)
      cancelAnimationFrame(r2)
    }
  }, [state])
  const cls = state === 'shown' && on ? 'is-shown' : state === 'hiding' ? 'is-hiding' : ''
  return (
    <div aria-hidden={state !== 'shown'} className={`t-stagger ${cls}`}>
      {lines.map((line, i) => (
        <div className={`t-stagger-line t-stagger-line--${i + 1}`} key={`${round}-${i}`}>{line}</div>
      ))}
    </div>
  )
}
