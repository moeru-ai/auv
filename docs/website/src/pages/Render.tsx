import type { ThemeName } from '../theme'

import { useEffect, useState } from 'react'
import { flushSync } from 'react-dom'

import { GLDesk } from '../gl/GLDesk'
import { Overlay } from '../render/Overlay'
import { film, FILM, LANDSCAPE, PORTRAIT } from '../scene/film'
import { THEMES } from '../theme'

declare global {
  interface Window {
    __auvDuration?: number
    /** Render film time `t`; resolves once the frame is painted. Used by scripts/export-video.ts. */
    __auvSetT?: (t: number) => Promise<void>
  }
}

const FIT = { k: 1, x: 0, y: 0 }

/**
 * Frame-addressable render target for the exporter: a bare 1920x1080 stage,
 * no UI. `/render?t=7.5&theme=dark` shows one frame. The WebGL frame is drawn
 * during React's commit, so `flushSync` returning means the frame is ready.
 */
export function RenderPage() {
  const q = new URLSearchParams(location.search)
  const theme = THEMES[(q.get('theme') as ThemeName) ?? 'light'] ?? THEMES.light
  const [t, setT] = useState(Number(q.get('t') ?? 0))
  const layout = q.get('layout') === 'portrait' ? PORTRAIT : LANDSCAPE
  const W = layout.width
  const H = layout.height

  useEffect(() => {
    document.documentElement.dataset.theme = theme.name
    window.__auvDuration = FILM.duration
    window.__auvSetT = t => new Promise((resolve) => {
      // NOTICE: the exporter screenshots right after this resolves, so the
      // frame for `t` must be committed synchronously, not batched.
      // eslint-disable-next-line react-dom/no-flush-sync
      flushSync(() => setT(t))
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
    })
  }, [theme.name])

  const state = film(t, theme, layout)
  return (
    <div style={{ height: H, overflow: 'hidden', position: 'relative', width: W }}>
      {/* The exporter sets deviceScaleFactor; let the buffer follow it all the way to 4K and beyond. */}
      <GLDesk fit={FIT} maxPixels={Infinity} stage={{ h: H, w: W }} state={state} style={{ height: H, inset: 0, position: 'absolute', width: W }} theme={theme} />
      <div style={{ inset: 0, position: 'absolute' }}>
        <Overlay cursors={state.cursors.filter(c => c.outline)} height={H} icon={state.icon} id="render" ripples={[]} theme={theme} width={W} />
      </div>
    </div>
  )
}
