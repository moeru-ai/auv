// Static cards over the landing's frosted ambient desk. Share previews (Open
// Graph, GitHub social preview) put the gray mark and the headline on the start
// side, with one crayon stroke curling from the mark into a column of empty
// square tiles (the agent apps AUV plugs into). The README banner is a compact
// lockup: the mark with the headline beside it.
// scripts/export-cards.ts screenshots this route into the Open Graph images,
// the GitHub social preview, and the README banner.
//
//   /card?kind=og|social|readme&theme=light|dark

import type { ThemeName } from '../theme'

import { useEffect } from 'react'

import { GLDesk } from '../gl/GLDesk'
import { ICON_BOX, PART_PATHS } from '../lib/icon'
import { COLORFUL } from '../render/Overlay'
import { ambient } from '../scene/ambient'
import { glowsFor } from '../scene/blobs'
import { FILM } from '../scene/film'
import { THEMES } from '../theme'

export interface CardKind {
  h: number
  /**
   * `lockup` puts the headline beside the mark, compact enough to stay legible
   * at GitHub's README width. `column` adds the crayon stroke into the tile
   * column, which needs the taller share-preview canvas.
   */
  layout: 'column' | 'lockup'
  /** Corner radius; the README banner sits on GitHub's page, so it gets a transparent corner. */
  radius: number
  w: number
}

// NOTICE: 1200x630 is the Open Graph size most crawlers recommend (1.91:1).
// 1280x640 is GitHub's recommended social preview size (Settings -> Social
// preview, under 1 MB). The README banner is shorter than the 16:9 landing so
// it does not push the README text below the fold.
export const CARDS = {
  og: { h: 630, layout: 'column', radius: 0, w: 1200 },
  readme: { h: 480, layout: 'lockup', radius: 44, w: 1280 },
  social: { h: 640, layout: 'column', radius: 0, w: 1280 },
} satisfies Record<string, CardKind>

export type CardName = keyof typeof CARDS

// A frame of the ambient loop where the blurred windows read evenly.
const CLOCK = 6

// Column of empty squares, top to bottom: each overlaps the one above by
// about half, with a small uneven tilt and sideways nudge so it looks set down
// by hand. `y` is in tile heights from the column center.
const COLUMN = [{ r: -5, x: -8, y: -0.78 }, { r: 4, x: 10, y: -0.26 }, { r: -3, x: -6, y: 0.26 }, { r: 6, x: 8, y: 0.78 }]

interface Pt { x: number, y: number }

export function CardPage() {
  const q = new URLSearchParams(location.search)
  const theme = THEMES[(q.get('theme') as ThemeName) ?? 'light'] ?? THEMES.light
  const card: CardKind = CARDS[(q.get('kind') as CardName) ?? 'og'] ?? CARDS.og

  useEffect(() => {
    document.documentElement.dataset.theme = theme.name
    // Transparent page, so the README banner's rounded corners stay see-through.
    document.documentElement.style.background = 'transparent'
    document.body.style.background = 'transparent'
  }, [theme.name])

  // Cover-fit the 1920x1080 desk into the card; the short README banner crops
  // the top and bottom rows of windows, which only show through the veil.
  const k = Math.max(card.w / FILM.width, card.h / FILM.height)
  const fit = { k, x: (card.w - FILM.width * k) / 2, y: (card.h - FILM.height * k) / 2 }
  const veil = {
    amount: 1,
    hole: { h: 0, w: 0, x: FILM.width / 2, y: FILM.height / 2 },
    open: 0,
    tint: theme.name === 'light' ? 'rgba(246,247,251,0.35)' : 'rgba(12,14,19,0.4)',
  }

  // Everything below is laid out for a 630 px tall card and scaled by `u`;
  // the start column anchors left and the tile column anchors right, so wider
  // cards only open up the middle.
  const u = card.h / 630
  const pad = 60 * u
  const markH = 140 * u
  const markW = (ICON_BOX.w / ICON_BOX.h) * markH
  const markTop = card.h * 0.47 - markH / 2 - 20 * u
  const tile = 132 * u
  const stack = { x: card.w - pad - 110 * u, y: card.h * 0.48 }
  // The stroke starts under the mark's end like a signature flourish.
  const flourish = { x: pad + markW * 0.92, y: markTop + markH + 6 * u }
  // NOTICE: the terminal desk window is the one dark slab on the light desk;
  // under the tile column it reads as a smudge, so cards leave it out.
  const desk = ambient(CLOCK, 1, FILM.width, FILM.height)
  desk.windows = desk.windows.filter(w => w.id !== 'a-term')
  desk.glows = glowsFor(desk.windows, CLOCK)
  if (card.layout === 'lockup') {
    const h = 196 * u
    const w = (ICON_BOX.w / ICON_BOX.h) * h
    return (
      <div id="card" style={{ background: theme.bg, borderRadius: card.radius, color: theme.ink, height: card.h, overflow: 'hidden', position: 'relative', width: card.w }}>
        <GLDesk fit={fit} maxPixels={Infinity} state={desk} style={{ height: card.h, inset: 0, position: 'absolute', width: card.w }} theme={theme} veil={veil} />
        <div className="card-lockup" style={{ gap: 68 * u }}>
          <svg aria-hidden="true" height={h} viewBox={`${ICON_BOX.x} ${ICON_BOX.y} ${ICON_BOX.w} ${ICON_BOX.h}`} width={w}>
            {PART_PATHS.map((d, i) => <path d={d} fill={theme.icon[i]} key={d} />)}
          </svg>
          <div className="card-headline-static">
            <p style={{ color: theme.ink2, fontSize: 30 * u, fontWeight: 650, marginBottom: 12 * u }}>Application Use Via ...</p>
            <h1 style={{ fontSize: 63 * u }}>Programmable Computer Use,</h1>
            <p style={{ color: theme.ink2, fontSize: 38 * u }}>more like Playwright for the OS.</p>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div
      id="card"
      style={{
        background: theme.bg,
        borderRadius: card.radius,
        color: theme.ink,
        height: card.h,
        overflow: 'hidden',
        position: 'relative',
        width: card.w,
      }}
    >
      <GLDesk fit={fit} maxPixels={Infinity} state={desk} style={{ height: card.h, inset: 0, position: 'absolute', width: card.w }} theme={theme} veil={veil} />
      <p className="card-kicker" style={{ color: theme.ink2, fontSize: 24 * u, left: pad, top: pad - 6 * u }}>Application Use Via ...</p>
      <svg aria-hidden="true" height={markH} style={{ left: pad, position: 'absolute', top: markTop }} viewBox={`${ICON_BOX.x} ${ICON_BOX.y} ${ICON_BOX.w} ${ICON_BOX.h}`} width={markW}>
        {PART_PATHS.map((d, i) => <path d={d} fill={theme.icon[i]} key={d} />)}
      </svg>
      {/* Agent apps AUV plugs into: empty squares stacked in a loose column. */}
      {COLUMN.map(f => (
        <div
          className="card-tile"
          key={f.r}
          style={{
            backdropFilter: theme.blur,
            background: `color-mix(in srgb, ${theme.ink} ${theme.name === 'dark' ? 7 : 0}%, ${theme.tint})`,
            border: `${2.5 * u}px dashed color-mix(in srgb, ${theme.ink} ${theme.name === 'dark' ? 46 : 34}%, transparent)`,
            borderRadius: 34 * u,
            height: tile,
            left: stack.x - tile / 2 + f.x * u,
            top: stack.y - tile / 2 + f.y * tile,
            transform: `rotate(${f.r}deg)`,
            width: tile,
          }}
        />
      ))}
      <svg aria-hidden="true" className="card-strokes" height={card.h} width={card.w}>
        <defs>
          {/* Wax crayon: wobbly edges, then small gaps where the wax skipped the paper grain. */}
          <filter height="140%" id="crayon" width="140%" x="-20%" y="-20%">
            <feTurbulence baseFrequency="0.7" numOctaves="2" result="wobble" seed="3" type="fractalNoise" />
            <feDisplacementMap in="SourceGraphic" in2="wobble" result="shape" scale={3 * u} xChannelSelector="R" yChannelSelector="G" />
            <feTurbulence baseFrequency="1.6" numOctaves="1" result="grain" seed="11" type="fractalNoise" />
            <feColorMatrix in="grain" result="wax" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 -2.2 2.3" />
            <feComposite in="shape" in2="wax" operator="in" />
          </filter>
        </defs>
        <Crayon color={COLORFUL[2]} from={flourish} to={{ x: stack.x - tile * 0.68, y: stack.y + 4 * u }} u={u} />
      </svg>
      <div className="card-headline" style={{ bottom: pad - 8 * u, left: pad }}>
        <h1 style={{ fontSize: 50 * u }}>Programmable Computer Use,</h1>
        <p style={{ color: theme.ink2, fontSize: 30 * u }}>more like Playwright for the OS.</p>
      </div>
    </div>
  )
}

/**
 * One crayon gesture under the mark: it dips to the lower left, curls once
 * like a signature flourish, then swings right in one long rising arc into the
 * tile column, and ends in a quick two-tick arrowhead.
 *
 * The curve is a chain of cubic Beziers with matching tangents at each joint,
 * so the hand never kinks. It is filled as a ribbon whose width follows pen
 * pressure: a light touch-down, fuller through the curl and the swing, easing
 * off near the end.
 */
function Crayon({ color, from, to, u }: { color: string, from: Pt, to: Pt, u: number }) {
  const p = (x: number, y: number) => ({ x: from.x + x * u, y: from.y + y * u })
  // The last segment continues the curl's exit tangent (40, 26) and arrives
  // rising toward the column.
  const exit = p(30, 66)
  const segs: [Pt, Pt, Pt, Pt][] = [
    [p(0, 0), p(-10, 40), p(-40, 78), p(-78, 70)],
    [p(-78, 70), p(-112, 62), p(-112, 18), p(-78, 10)],
    [p(-78, 10), p(-44, 2), p(-10, 40), exit],
    [exit, { x: exit.x + 100 * u, y: exit.y + 65 * u }, { x: to.x - 170 * u, y: to.y + 70 * u }, to],
  ]
  const pts: Pt[] = []
  segs.forEach(([a, b, c, d], i) => {
    const steps = i === segs.length - 1 ? 90 : 30
    for (let n = i === 0 ? 0 : 1; n <= steps; n++) {
      const t = n / steps
      const m = 1 - t
      pts.push({
        x: m * m * m * a.x + 3 * m * m * t * b.x + 3 * m * t * t * c.x + t * t * t * d.x,
        y: m * m * m * a.y + 3 * m * m * t * b.y + 3 * m * t * t * c.y + t * t * t * d.y,
      })
    }
  })
  const width = (t: number) => {
    const down = Math.min(1, 0.25 + t / 0.05)
    const lift = t > 0.86 ? 1 - (t - 0.86) * 2.4 : 1
    return 8 * u * down * lift * (1 + 0.1 * Math.sin(t * 13))
  }
  const end = segs[segs.length - 1][2]
  const a = Math.atan2(to.y - end.y, to.x - end.x)
  const tick = (turn: number, len: number): Pt[] => [
    { x: to.x - Math.cos(a + turn) * len, y: to.y - Math.sin(a + turn) * len },
    to,
  ]
  // Ticks are drawn tip-outward: heavier at the tip, trailing off.
  const tickWidth = (t: number) => 7.5 * u * (1 - 0.6 * (1 - t))
  return (
    <g fill={color} filter="url(#crayon)">
      <path d={ribbon(pts, width)} />
      <path d={ribbon(tick(0.62, 30 * u), tickWidth, 8)} />
      <path d={ribbon(tick(-0.52, 25 * u), tickWidth, 8)} />
    </g>
  )
}

/**
 * Outline of a variable-width stroke along `pts` (a filled ribbon with round
 * caps). `width(t)` is the full width at arc position t in 0..1. Two-point
 * input is resampled into `steps` points first.
 */
function ribbon(input: Pt[], width: (t: number) => number, steps = 0): string {
  const pts = steps > 0
    ? Array.from({ length: steps + 1 }, (_, n) => ({
        x: input[0].x + (input[1].x - input[0].x) * (n / steps),
        y: input[0].y + (input[1].y - input[0].y) * (n / steps),
      }))
    : input
  const left: string[] = []
  const right: string[] = []
  pts.forEach((pt, n) => {
    const prev = pts[Math.max(0, n - 1)]
    const next = pts[Math.min(pts.length - 1, n + 1)]
    const len = Math.hypot(next.x - prev.x, next.y - prev.y) || 1
    const nx = -(next.y - prev.y) / len
    const ny = (next.x - prev.x) / len
    const w = width(n / (pts.length - 1)) / 2
    left.push(`${(pt.x + nx * w).toFixed(2)} ${(pt.y + ny * w).toFixed(2)}`)
    right.push(`${(pt.x - nx * w).toFixed(2)} ${(pt.y - ny * w).toFixed(2)}`)
  })
  const capEnd = width(1) / 2
  const capStart = width(0) / 2
  return `M${left.join(' L')} A${capEnd} ${capEnd} 0 0 1 ${right.at(-1)} L${right.reverse().join(' L')} A${capStart} ${capStart} 0 0 1 ${left[0]} Z`
}
