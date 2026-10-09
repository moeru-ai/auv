import { useMemo, useState } from 'react'

import { GLDesk } from '../gl/GLDesk'
import { useClock, useTheme } from '../lib/useTheme'
import { Overlay } from '../render/Overlay'
import { film, FILM, LANDSCAPE, PORTRAIT } from '../scene/film'
import { FitStage, useFit } from '../ui/FitStage'

/** Review player: scrub, loop, and flip themes. `?t=7.5` opens paused at a frame. */
export function FilmPage() {
  const [theme, setTheme] = useTheme()
  const q = new URLSearchParams(location.search)
  const [t, setT] = useState(() => Number(q.get('t') ?? 0))
  const [playing, setPlaying] = useState(!q.has('t'))
  const [loop, setLoop] = useState(true)
  const [layout, setLayout] = useState(() => (q.get('layout') === 'portrait' ? PORTRAIT : LANDSCAPE))
  const fit = useFit(layout.width, layout.height, 'meet')

  useClock(playing, dt => setT((prev) => {
    const next = prev + dt
    if (next < FILM.duration)
      return next
    if (!loop)
      setPlaying(false)
    return loop ? 0 : FILM.duration
  }))

  const state = useMemo(() => film(t, theme, layout), [t, theme, layout])

  return (
    <>
      <GLDesk fit={fit} stage={{ h: layout.height, w: layout.width }} state={state} theme={theme} />
      <FitStage height={layout.height} mode="meet" width={layout.width}>
        <Overlay cursors={state.cursors.filter(c => c.outline)} height={layout.height} icon={state.icon} id="film" ripples={[]} theme={theme} width={layout.width} />
      </FitStage>
      <div className="toolbar">
        <button onClick={() => setPlaying(p => !p)}>{playing ? 'Pause' : 'Play'}</button>
        <input
          max={FILM.duration}
          min={0}
          onChange={(e) => {
            setPlaying(false)
            setT(Number(e.target.value))
          }}
          step={1 / 60}
          type="range"
          value={t}
        />
        <span className="time">{`${t.toFixed(2)}s`}</span>
        <button onClick={() => setLoop(l => !l)}>{loop ? 'Loop on' : 'Loop off'}</button>
        <button onClick={() => setLayout(l => (l === PORTRAIT ? LANDSCAPE : PORTRAIT))}>{layout === PORTRAIT ? '16:9' : '9:16'}</button>
        <button onClick={() => setTheme(theme.name === 'light' ? 'dark' : 'light')}>{theme.name === 'light' ? 'Dark' : 'Light'}</button>
        <a href="/export">Export</a>
      </div>
    </>
  )
}
