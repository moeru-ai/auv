// Landing: intro film, then the interactive mark over a frosted ambient desk.
//
// One WebGL canvas draws the film during the intro and the ambient desk after
// it, including the frosted veil and the hover hole (a shader uniform, so it
// costs nothing to animate). The mark, copy, and info cards are DOM in a
// 1920x1080 stage scaled to cover the viewport, on top of the canvas.
//
// After the intro the page scrolls: an opaque install section slides up over
// the fixed hero. The gray mark flies up with it and docks above the install
// heading, the copy fades, and the hero stops reacting to the pointer.

import type { DeskInfo } from '../scene/ambient'
import type { Theme } from '../theme'

import { clamp } from 'es-toolkit'
import { useEffect, useMemo, useRef, useState } from 'react'

import { GLDesk } from '../gl/GLDesk'
import { HeroIcon } from '../hero/HeroIcon'
import { INSTALL_ID, INSTALL_MARK_ID, InstallSection } from '../install/InstallSection'
import { cubicBezier, lerp, prog } from '../lib/ease'
import { ICON_CENTER, PART_PATHS } from '../lib/icon'
import { useTapToSkip } from '../lib/useTapToSkip'
import { useClock, useTheme } from '../lib/useTheme'
import { glassStyle } from '../render/glass'
import { COLORFUL, Overlay } from '../render/Overlay'
import { MONO } from '../render/WindowContent'
import { ambient, deskRect, DESKS } from '../scene/ambient'
import { film, FILM, FILM_HANDOFF, FILM_ICON } from '../scene/film'
import { Island, IslandButton, NavBar } from '../ui/Chrome'
import { CORE, Struck } from '../ui/Copy'
import { useFit, useViewport } from '../ui/FitStage'
import { TextsReveal } from '../ui/TextsReveal'

const W = FILM.width
const H = FILM.height
// Film time at which the mark has turned gray; the hero takes over here with
// an identical frame.
const HANDOFF = FILM_HANDOFF
const SETTLE = cubicBezier(0.3, 0, 0.1, 1)

interface Hole { h: number, open: number, w: number, x: number, y: number }

export function Landing() {
  const [theme, setTheme] = useTheme()
  const skip = new URLSearchParams(location.search).has('skip')
  const [filmT, setFilmT] = useState(skip ? HANDOFF : 0)
  const [clock, setClock] = useState(0)
  const [part, setPart] = useState<null | number>(null)
  const [pointer, setPointer] = useState<null | { x: number, y: number }>(null)
  const [scrollY, setScrollY] = useState(0)
  const hole = useRef<Hole>({ h: 0, open: 0, w: 0, x: W / 2, y: H / 2 })
  const deskHoverRef = useRef<null | string>(null)
  const fit = useFit(W, H, 'cover')
  const { h: viewH, w: viewW } = useViewport()
  // Stage px visible across the screen; below 1920 the sides are cropped.
  const visibleW = Math.min(W, viewW / fit.k)
  const visibleWRef = useRef(visibleW)
  visibleWRef.current = visibleW

  useClock(true, (dt) => {
    setClock(c => c + dt)
    setFilmT(t => Math.min(HANDOFF + 3, t + dt))
    // Ease the veil's hole toward the hovered desk window; snap when settled.
    const desk = DESKS.find(d => d.id === deskHoverRef.current)
    const target = desk ? deskRect(desk, visibleWRef.current) : undefined
    const k = 1 - Math.exp(-dt * 12)
    const h = hole.current
    if (target) {
      // Extra room right and below for the ghost's label, which pokes out of the window.
      h.x += (target.x - h.x) * k
      h.y += (target.y - h.y) * k
      h.w += (target.w + 150 - h.w) * k
      h.h += (target.h + 60 - h.h) * k
    }
    h.open += ((target ? 1 : 0) - h.open) * k
    if (!target && h.open < 0.002)
      h.open = 0
  })

  useEffect(() => {
    const onMove = (e: PointerEvent) => setPointer({ x: (e.clientX - fit.x) / fit.k, y: (e.clientY - fit.y) / fit.k })
    // Touch has no hover: a tap sets the pointer, and lifting the finger must
    // not clear it, or every tap would open and instantly close.
    const onOut = (e: PointerEvent) => {
      if (!e.relatedTarget && e.pointerType !== 'touch')
        setPointer(null)
    }
    addEventListener('pointermove', onMove)
    addEventListener('pointerdown', onMove)
    addEventListener('pointerout', onOut)
    return () => {
      removeEventListener('pointermove', onMove)
      removeEventListener('pointerdown', onMove)
      removeEventListener('pointerout', onOut)
    }
  }, [fit.k, fit.x, fit.y])

  const intro = filmT < HANDOFF
  useEffect(() => {
    // The intro owns the screen; the page scrolls only after it.
    document.documentElement.style.overflow = intro ? 'hidden' : ''
    if (intro)
      scrollTo({ top: 0 })
    const onScroll = () => setScrollY(window.scrollY)
    onScroll()
    addEventListener('scroll', onScroll, { passive: true })
    return () => {
      document.documentElement.style.overflow = ''
      removeEventListener('scroll', onScroll)
    }
  }, [intro])
  const skipCover = useTapToSkip(intro, () => setFilmT(HANDOFF))
  // Once the install section starts covering the hero, the hero ignores the pointer.
  const heroPointer = scrollY > 24 ? null : pointer
  // Mark flight into the install section's slot: 0 at the top, 1 once docked.
  // Past 1 the section's own mark takes over and scrolls with the page.
  const dock = intro ? 0 : SETTLE(clamp(scrollY / (viewH * 0.8), 0, 1))
  const appear = prog(filmT, HANDOFF - 0.2, HANDOFF + 1.4)
  // Desk hover: only outside the mark, only once the desk is up.
  // NOTICE: derived during render, not synced by an effect. `appear` changes
  // every frame while the desk comes up, so an effect calling setState on each
  // change tripped React's nested passive update limit ("Maximum update depth
  // exceeded") whenever the per-frame clock update was still pending.
  const deskHover = useMemo(() => {
    const p = heroPointer
    if (!p || part !== null || appear < 0.9)
      return null
    const hit = DESKS.find((d) => {
      const r = deskRect(d, visibleW)
      return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
    })
    return hit?.id ?? null
  }, [heroPointer, part, appear, visibleW])
  deskHoverRef.current = deskHover
  const scene = useMemo(
    () => (intro ? film(filmT, theme) : ambient(clock, appear, W, H, deskHover, visibleW)),
    [intro, filmT, theme, clock, appear, deskHover, visibleW],
  )
  const copyIn = prog(filmT, HANDOFF - 0.1, HANDOFF + 0.8)

  // Keep the last revealed window's card while the hole closes, so both fade together.
  const lastDesk = useRef<null | string>(null)
  if (deskHover)
    lastDesk.current = deskHover
  const deskFocus = hole.current.open
  const hovered = deskFocus > 0.01 ? DESKS.find(d => d.id === lastDesk.current) : undefined
  const veil = intro
    ? undefined
    : {
        amount: appear,
        hole: hole.current,
        open: hole.current.open,
        tint: theme.name === 'light' ? 'rgba(246,247,251,0.35)' : 'rgba(12,14,19,0.4)',
      }

  return (
    <>
      <GLDesk fit={fit} focus={intro ? undefined : deskHover} state={scene} theme={theme} veil={veil} />
      <div className="fit">
        <div style={{ height: H, left: 0, position: 'absolute', top: 0, transform: `translate(${fit.x}px, ${fit.y}px) scale(${fit.k})`, transformOrigin: '0 0', width: W }}>
          {intro && <Overlay cursors={scene.cursors.filter(c => c.outline)} height={H} icon={scene.icon} id="intro" ripples={[]} theme={theme} width={W} />}
          {/* While a desk window is revealed, the mark shrinks and the copy fades so the card has room. */}
          {!intro && dock === 0 && <HeroIcon dim={deskFocus} onHover={setPart} pointer={heroPointer} scale={FILM_ICON.s * (1 - 0.3 * deskFocus)} theme={theme} />}
          <div className={`hero-copy ${part !== null ? 'away' : ''}`} style={{ opacity: (1 - 0.85 * deskFocus) * (1 - clamp(dock * 1.6, 0, 1)), transform: `translateY(${-40 * deskFocus - 120 * dock}px)` }}>
            {/* Same tagline and capability lines as the mobile landing; a
                hovered part swaps the text in place. */}
            <TextsReveal
              active={filmT < HANDOFF - 0.1 ? null : part === null ? 'tagline' : `part-${part}`}
              slides={[
                {
                  key: 'tagline',
                  lines: [
                    <h1 key="h">Application Use Via</h1>,
                    <p key="p">
                      programmable
                      {' '}
                      <Struck play />
                    </p>,
                  ],
                },
                ...CORE.map(({ body, title }, i) => ({
                  key: `part-${i}`,
                  lines: [<h1 key="h" style={{ color: COLORFUL[i] }}>{title}</h1>, <p key="p">{body}</p>],
                })),
              ]}
            />
          </div>
          {hovered && <InfoCard info={hovered.info} open={hole.current.open} rect={deskRect(hovered, visibleW)} theme={theme} />}
        </div>
      </div>
      {!intro && (
        <div className="install-flow">
          <div style={{ height: '100vh' }} />
          <InstallSection markHidden={dock < 1} theme={theme} />
        </div>
      )}
      {dock > 0 && dock < 1 && <FlyingMark from={{ s: FILM_ICON.s * fit.k, x: fit.x + FILM_ICON.cx * fit.k, y: fit.y + FILM_ICON.cy * fit.k }} p={dock} theme={theme} />}
      <div className={`skip-cover ${skipCover ? 'on' : ''}`} />
      <NavBar brand={intro ? 0 : copyIn} onToggleTheme={() => setTheme(theme.name === 'light' ? 'dark' : 'light')} theme={theme} />
      {/* Review controls, dev only (production skips by tapping the intro). */}
      {import.meta.env.DEV && (
        <Island>
          {intro
            ? <IslandButton icon="arrow-counter-clockwise" label="Skip intro" onClick={() => setFilmT(HANDOFF)} />
            : <IslandButton icon="arrow-counter-clockwise" label="Replay" onClick={() => setFilmT(0)} />}
          {!intro && <IslandButton icon="download-simple" label="Install" onClick={() => document.getElementById(INSTALL_ID)?.scrollIntoView({ behavior: 'smooth' })} />}
          <IslandButton href="/film" icon="film-strip" label="Film" />
        </Island>
      )}
    </>
  )
}

/** The gray mark between the hero and the install slot (read live, as it scrolls). */
function FlyingMark({ from, p, theme }: { from: { s: number, x: number, y: number }, p: number, theme: Theme }) {
  const slot = document.getElementById(INSTALL_MARK_ID)?.getBoundingClientRect()
  if (!slot)
    return null
  const s = lerp(from.s, slot.height / 92.4, p)
  const x = lerp(from.x, slot.left + slot.width / 2, p)
  const y = lerp(from.y, slot.top + slot.height / 2, p)
  return (
    <svg aria-hidden="true" className="flying-mark">
      <g transform={`translate(${x - ICON_CENTER.x * s} ${y - ICON_CENTER.y * s}) scale(${s})`}>
        {PART_PATHS.map((d, i) => <path d={d} fill={theme.icon[i]} key={i} />)}
      </g>
    </svg>
  )
}

function InfoCard({ info, open, rect, theme }: { info: DeskInfo, open: number, rect: { h: number, w: number, x: number, y: number }, theme: Theme }) {
  const right = rect.x + rect.w / 2 < W / 2
  const width = 380
  const x = right ? rect.x + rect.w + 28 : rect.x - width - 28
  const y = clamp(rect.y + 10, 20, H - 230)
  return (
    <div
      className="info-card"
      style={{
        left: x,
        top: y,
        width,
        ...glassStyle(theme, false),
        boxShadow: `inset 0 0 0 1px ${theme.edge}, 0 20px 50px ${theme.shadow}`,
        opacity: open,
        transform: `translateX(${(1 - open) * (right ? -16 : 16)}px) scale(${0.96 + 0.04 * open})`,
      }}
    >
      <h3>{info.title}</h3>
      <p>{info.body}</p>
      <code style={{ fontFamily: MONO }}>{info.code}</code>
    </div>
  )
}
