// Portrait landing (phones, iPad portrait).
//
//   intro    the 9:16 film (WebGL)
//   screen 1 the desk's task windows sit frosted in the background, and the
//            gray mark rests in the lower third. The picked part's colored copy
//            flies up out of the mark and unfolds like the desktop hero:
//            A -> ghost cursor, U -> app windows, V -> line pointer + REPL.
//            Tap a part to switch; it cycles on its own until the first tap.
//   scroll   the gray mark flies into the nav bar; the showcase fades; the
//            background windows come out of the frost and line up in a row.
//   gallery  scrolling sweeps the row, one window per page.
//            Every window keeps its own ghost cursor, as on desktop, with a
//            short title and description underneath.
//   install  an opaque install section slides up over the gallery.
//
// Everything after the intro is scrubbed by scroll position, so scrolling
// back plays it in reverse. Between the hero, each gallery window, and the
// install section the scroll pages like a carousel (useSnapPaging).
// NOTICE: copy is provisional, like the desktop explainers.

import type { Desk } from '../scene/ambient'
import type { Rect, SceneState } from '../scene/types'

import { clamp } from 'es-toolkit'
import { useEffect, useMemo, useRef, useState } from 'react'

import { GLDesk } from '../gl/GLDesk'
import { MobileHero } from '../hero/MobileHero'
import { INSTALL_ID, InstallSection } from '../install/InstallSection'
import { mixOklab } from '../lib/color'
import { cubicBezier, lerp, prog } from '../lib/ease'
import { ICON_CENTER, PART_PATHS } from '../lib/icon'
import { useSafeArea } from '../lib/useSafeArea'
import { useSnapPaging } from '../lib/useSnapPaging'
import { useTapToSkip } from '../lib/useTapToSkip'
import { useClock, useTheme } from '../lib/useTheme'
import { COLORFUL } from '../render/Overlay'
import { ambient, DESKS } from '../scene/ambient'
import { film, FILM_HANDOFF, filmIcon, PORTRAIT } from '../scene/film'
import { Island, IslandButton, NAV_H, NAV_MARK_H, NavBar, navMarkCenter } from '../ui/Chrome'
import { CORE, Struck } from '../ui/Copy'
import { useFit, useViewport } from '../ui/FitStage'
import { TextsReveal } from '../ui/TextsReveal'

/** Film time at which the mark has turned gray (same as desktop). */
const HANDOFF = FILM_HANDOFF
const SETTLE = cubicBezier(0.3, 0, 0.1, 1)
const seg = (p: number, a: number, b: number) => clamp((p - a) / (b - a), 0, 1)
/** Auto-advance period, and how long a tapped part holds before cycling on. */
const CYCLE = 4.5
const TAP_HOLD = 9

/** Gallery order, and where each window sits in the frosted portrait collage (stage px). */
const ORDER = ['a-browser', 'a-scan', 'a-music', 'a-notes', 'a-trace', 'a-term']
const COLLAGE: Record<string, { x: number, y: number }> = {
  'a-browser': { x: 40, y: 150 },
  'a-music': { x: 610, y: 440 },
  'a-notes': { x: 620, y: 1380 },
  'a-scan': { x: 20, y: 540 },
  'a-term': { x: 40, y: 1440 },
  'a-trace': { x: 560, y: 110 },
}

export function MobileLanding() {
  const [theme, setTheme] = useTheme()
  const skip = new URLSearchParams(location.search).has('skip')
  const [filmT, setFilmT] = useState(skip ? HANDOFF : 0)
  const [clock, setClock] = useState(0)
  const [scrollY, setScrollY] = useState(0)
  const [sel, setSel] = useState(0)
  const view = useViewport()
  // A just-opened or resizing window can briefly report 0; keep the math finite.
  const vw = Math.max(view.w, 1)
  const vh = Math.max(view.h, 1)
  const filmFit = useFit(PORTRAIT.width, PORTRAIT.height, 'meet')
  const deskFit = useFit(PORTRAIT.width, PORTRAIT.height, 'cover')
  const safe = useSafeArea()

  const intro = filmT < HANDOFF
  const skipCover = useTapToSkip(intro, () => setFilmT(HANDOFF))
  useEffect(() => {
    // The intro owns the screen; the page scrolls only after it.
    document.documentElement.style.overflow = intro ? 'hidden' : ''
    if (intro)
      scrollTo({ top: 0 })
    return () => {
      document.documentElement.style.overflow = ''
    }
  }, [intro])
  useEffect(() => {
    const onScroll = () => setScrollY(window.scrollY)
    addEventListener('scroll', onScroll, { passive: true })
    return () => removeEventListener('scroll', onScroll)
  }, [])

  // Part springs (A, U, V) and the auto-advance.
  const springs = useRef([0, 0, 0].map(() => ({ v: 0, vel: 0 })))
  const auto = useRef({ hold: CYCLE, idle: 0 })
  const selRef = useRef(sel)
  selRef.current = sel
  const t0 = filmT - HANDOFF
  const t0Ref = useRef(t0)
  t0Ref.current = t0
  const gRef = useRef(0)

  useClock(true, (dt) => {
    setClock(c => c + dt)
    setFilmT(t => Math.min(HANDOFF + 6, t + dt))
    const ready = t0Ref.current > 0.5
    springs.current.forEach((s, i) => {
      const target = ready && i === selRef.current && gRef.current < 0.5 ? 1 : 0
      const acc = 150 * (target - s.v) - 18 * s.vel
      s.vel += acc * dt
      s.v += s.vel * dt
    })
    const a = auto.current
    // The cycle starts counting once the showcase is up, not during the intro.
    a.idle = ready ? a.idle + dt : 0
    if (ready && a.idle > a.hold && gRef.current < 0.05) {
      a.idle = 0
      a.hold = CYCLE
      setSel(s => (s + 1) % 3)
    }
  })

  const pick = (i: number) => {
    auto.current.idle = 0
    auto.current.hold = TAP_HOLD
    setSel(i)
  }

  // --- Scroll ranges ---------------------------------------------------------
  const enter = vh * 0.9
  const span = vh * 0.55 * (ORDER.length - 1)
  const g = clamp(scrollY / enter, 0, 1)
  gRef.current = g
  const gp = clamp((scrollY - enter) / span, 0, 1)
  // Linear, so every window takes the same drag; the paging does the easing.
  const pos = (ORDER.length - 1) * gp
  // Pages: the hero, each window dead center, then the install section's top
  // (its scroll-margin-top in styles.css).
  const installSnap = vh + enter + span - 48
  const snaps = useMemo(() => [0, ...ORDER.map((_, k) => enter + (k / (ORDER.length - 1)) * span), installSnap], [enter, span, installSnap])
  useSnapPaging(snaps, !intro)

  // --- Screen 1 geometry (CSS px) ------------------------------------------
  const settle = SETTLE(clamp(t0 / 0.9, 0, 1))
  const markH = clamp(vw * 0.15, 50, 80)
  const icon = filmIcon(PORTRAIT)
  const filmMark = { s: icon.s * filmFit.k, x: filmFit.x + icon.cx * filmFit.k, y: filmFit.y + icon.cy * filmFit.k }
  const rest = { s: markH / 92.4, x: vw / 2, y: vh * 0.74 }
  const nav = navMarkCenter(safe.top)
  const toNav = SETTLE(seg(g, 0, 0.8))
  const mark = {
    s: lerp(lerp(filmMark.s, rest.s, settle), NAV_MARK_H / 92.4, toNav),
    x: lerp(lerp(filmMark.x, rest.x, settle), nav.x, toNav),
    y: lerp(lerp(filmMark.y, rest.y, settle), nav.y, toNav),
  }
  const areaTop = safe.top + NAV_H + 8
  // The showcase block (visual + capability subtitle) is centered between the
  // nav bar and the mark; the visuals size themselves from the area width.
  const freeH = rest.y - markH / 2 - 36 - areaTop
  const visualH = 0.64 * Math.min(vw - 32, 460)
  const blockTop = areaTop + Math.max(0, (freeH - visualH - 24 - 64) / 2)
  const area = { h: visualH, w: vw - 32, x: 16, y: blockTop }
  const heroFade = SETTLE(seg(g, 0, 0.45))
  const h = springs.current.map(s => s.v) as [number, number, number]

  // --- Gallery geometry -----------------------------------------------------
  const cardW = Math.min(vw * 0.8, 520)
  const cardCy = vh * 0.43
  const pitchS = cardW * 1.1
  const galleryIn = SETTLE(seg(g, 0.6, 1))

  // --- GL scene: the desk's windows, frosted collage -> gallery row ----------
  const k = deskFit.k
  const row = useMemo(() => ({
    c: { x: (vw / 2 - deskFit.x) / k, y: (cardCy - deskFit.y) / k },
    wStage: cardW / k,
  }), [vw, cardCy, cardW, k, deskFit.x, deskFit.y])
  const deskScene = useMemo((): null | SceneState => {
    if (intro)
      return null
    const line = SETTLE(seg(g, 0.2, 1))
    const at = (d: Desk) => {
      const i = ORDER.indexOf(d.id)
      const from = COLLAGE[d.id]
      const fc = { x: from.x + d.rect.w / 2, y: from.y + d.rect.h / 2 }
      const tc = { x: row.c.x + (i - pos) * row.wStage * 1.1, y: row.c.y }
      return { cx: lerp(fc.x, tc.x, line), cy: lerp(fc.y, tc.y, line), s: lerp(1, row.wStage / d.rect.w, line) }
    }
    const placedAt = new Map(DESKS.map(d => [d.id, at(d)]))
    const rectOf = (d: Desk): Rect => {
      const a = placedAt.get(d.id)!
      return { h: d.rect.h, w: d.rect.w, x: a.cx - d.rect.w / 2, y: a.cy - d.rect.h / 2 }
    }
    const s = ambient(clock, prog(t0, 0, 1.2), PORTRAIT.width, PORTRAIT.height, null, PORTRAIT.width, rectOf)
    // Windows scale about their centers in the row; their own ghosts, labels,
    // and ripples scale with them so every window keeps its cursor, as on desktop.
    const grow = (id: string, x: number, y: number) => {
      const a = placedAt.get(id)!
      return { x: a.cx + (x - a.cx) * a.s, y: a.cy + (y - a.cy) * a.s }
    }
    s.windows = s.windows.map(w => ({ ...w, scale: placedAt.get(w.id)!.s }))
    s.cursors = s.cursors.map((c) => {
      const id = c.id.replace(/-ghost$/, '')
      return { ...c, ...grow(id, c.x, c.y), scale: c.scale * Math.max(1, placedAt.get(id)!.s * 0.85) }
    })
    s.ripples = s.ripples.map((r) => {
      const d = DESKS.find((d) => {
        const rr = rectOf(d)
        return r.x >= rr.x && r.x <= rr.x + rr.w && r.y >= rr.y && r.y <= rr.y + rr.h
      })
      return d ? { ...r, ...grow(d.id, r.x, r.y) } : r
    })
    return s
  }, [intro, clock, g, pos, row, t0])

  const filmState = useMemo(() => (intro ? film(filmT, theme, PORTRAIT) : null), [intro, filmT, theme])
  const glState = filmState ?? deskScene!
  const veil = intro
    ? undefined
    : { amount: prog(t0, 0, 0.8) * (1 - SETTLE(seg(g, 0.3, 0.9))), hole: { h: 0, w: 0, x: 0, y: 0 }, open: 0, tint: theme.name === 'light' ? 'rgba(246,247,251,0.35)' : 'rgba(12,14,19,0.4)' }

  return (
    <div className="m2" style={{ background: theme.bg }}>
      <GLDesk fit={intro ? filmFit : deskFit} focus={intro || g > 0.5 ? undefined : null} stage={{ h: PORTRAIT.height, w: PORTRAIT.width }} state={glState} theme={theme} veil={veil} />

      {intro && filmState && (
        <div className="fit">
          <svg className="m2-mark" height="100%" width="100%">
            {filmState.icon && (
              <g transform={`translate(${filmFit.x + (filmState.icon.cx - ICON_CENTER.x * filmState.icon.s) * filmFit.k} ${filmFit.y + (filmState.icon.cy - ICON_CENTER.y * filmState.icon.s) * filmFit.k}) scale(${filmState.icon.s * filmFit.k})`}>
                {PART_PATHS.map((d, i) => <path d={d} fill={mixOklab(theme.icon[i], COLORFUL[i], typeof filmState.icon!.color === 'number' ? filmState.icon!.color : filmState.icon!.color[i])} key={i} />)}
              </g>
            )}
            {filmState.cursors.filter(c => c.outline).map(c => (
              <path d={c.outline} fill={c.outlineFill} key={c.id} opacity={c.opacity} transform={`translate(${filmFit.x} ${filmFit.y}) scale(${filmFit.k})`} />
            ))}
          </svg>
        </div>
      )}

      {!intro && (
        <>
          <section className="m2-scroller" style={{ height: vh + enter + span }} />
          {/* Opaque, above the fixed layers: it slides up over the end of the gallery. */}
          <InstallSection bottomPad={safe.bottom} compact theme={theme} />

          <div className="m2-layer">
            {heroFade < 1 && <MobileHero area={area} fade={heroFade} h={h} mark={rest} t={clock} theme={theme} />}

            <svg className="m2-mark" height="100%" width="100%">
              {/* The gray mark until it lands in the nav bar. */}
              {g < 0.99 && (
                <g transform={`translate(${mark.x - ICON_CENTER.x * mark.s} ${mark.y - ICON_CENTER.y * mark.s}) scale(${mark.s})`}>
                  {PART_PATHS.map((d, i) => <path d={d} fill={theme.icon[i]} key={i} />)}
                </g>
              )}
              {/* Tap targets per part on screen 1. */}
              {g < 0.1 && t0 > 0.5 && [0, 1, 2].map((i) => {
                const x0 = rest.x + ([20, 104, 178][i] - ICON_CENTER.x) * rest.s
                const w = [84, 74, 86][i] * rest.s
                return <rect fill="transparent" height={140 * rest.s} key={i} onClick={() => pick(i)} style={{ cursor: 'pointer', pointerEvents: 'all' }} width={w} x={x0} y={rest.y - 70 * rest.s} />
              })}
            </svg>

            <div className="m2-sub" style={{ opacity: 1 - heroFade, top: blockTop + visualH + 24 }}>
              <TextsReveal
                active={t0 > 0.6 ? `part-${sel}` : null}
                slides={CORE.map(({ body, title }, i) => ({
                  key: `part-${i}`,
                  lines: [<h2 key="h" style={{ color: COLORFUL[i] }}>{title}</h2>, <p key="p">{body}</p>],
                }))}
              />
            </div>

            <p className="m2-tagline" style={{ opacity: seg(t0, 0.3, 0.9) * (1 - seg(g, 0, 0.25)), top: rest.y + markH / 2 + 18 }}>
              <strong>Application Use Via</strong>
              <span>
                programmable
                {' '}
                <Struck play={t0 > 0.6} />
              </span>
            </p>

            {/* Gallery: a title and a line or two in the lower start corner, no box. */}
            {galleryIn > 0.01 && ORDER.map((id, i) => {
              const d = DESKS.find(x => x.id === id)!
              const off = i - pos
              if (Math.abs(off) > 1.2)
                return null
              return (
                <div
                  className="m2-gal-text"
                  key={id}
                  style={{
                    // Anchored to the lower start corner (left in LTR, right in RTL),
                    // drifting a little with the row so neighbours cross-fade.
                    bottom: safe.bottom + 96,
                    opacity: galleryIn * clamp(1 - Math.abs(off) * 1.6, 0, 1),
                    transform: `translateX(${off * pitchS * 0.3}px)`,
                    width: Math.min(vw - 48, 420),
                  }}
                >
                  <h3>{d.info.title}</h3>
                  <p>{d.info.body}</p>
                </div>
              )
            })}
          </div>
        </>
      )}

      <div className={`skip-cover ${skipCover ? 'on' : ''}`} />
      <NavBar brand={!intro && g >= 0.99 ? 1 : 0} onToggleTheme={() => setTheme(theme.name === 'light' ? 'dark' : 'light')} safeTop={safe.top} theme={theme} />
      {/* Review controls, dev only (production skips by tapping the intro). */}
      {import.meta.env.DEV && (
        <Island bottom={safe.bottom}>
          {intro
            ? <IslandButton icon="arrow-counter-clockwise" label="Skip" onClick={() => setFilmT(HANDOFF)} />
            : (
                <IslandButton
                  icon="arrow-counter-clockwise"
                  label="Replay"
                  onClick={() => {
                    scrollTo({ top: 0 })
                    setFilmT(0)
                  }}
                />
              )}
          {!intro && <IslandButton icon="download-simple" label="Install" onClick={() => document.getElementById(INSTALL_ID)?.scrollIntoView({ behavior: 'smooth' })} />}
        </Island>
      )}
    </div>
  )
}
