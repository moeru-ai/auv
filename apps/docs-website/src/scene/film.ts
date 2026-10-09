// The intro film, as a pure function of time. It loops: the first and last
// frames are the same gray AUV mark.
//
//   0.0 - 0.45  the mark holds
//   0.45 - 1.85 the camera turns out to a high orthographic 3/4 view over a
//               line-up of faint display cards (half desk size, one per desktop
//               or device) drifting from bottom right to top left; far cards are
//               defocused. A, U, V glide up the line onto three separate
//               displays and change shape only as they settle: A and V into
//               ghost cursors, the U (the ending's springs, backwards) into a
//               plain window block; all three fade to a docked gray
//   1.75 - 5.0  the line-up runs: it speeds up, streams dozens of displays past
//               with motion trails (the docked ones fly off with it), then
//               brakes so a fresh display slides in from behind and stops. The
//               camera turns into a standard perspective and yaws toward the
//               line, landing on the front view while the line is still
//               slightly oblique. That display lights up and opens to full
//               size, the cards in front of it fade left to right, and every
//               window blooms in it
//   4.25 - 4.85 the user's pointer arrives while the camera lands on the desk
//   4.9 - 6.0   cyan, pink, and violet ghost cursors spring out of it in turn
//               and spread out (Notes, Music, the REPL), so the desk is busy
//               as soon as the camera settles
//   6.0 - 9.3   the cyan ghost drags Notes and clicks "New"; a new window
//               springs out of that click; pink moves on to it
//   9.3 - 12.85 everyone works in parallel (checklist, REPL, music, browsing)
//   12.55 - 13.55 the last two ghost clicks; the user's pointer fades out
//   12.85 - 14.65 windows leave; the browser shrinks, leans, and rounds into the
//               "U", the cyan ghost becomes the "A", the pink ghost the "V"
//   15.1 - 16.0 the color leaves A, U, V in turn (OKLab), then hold until 16.9
//
// After the intro, choreography is written in "desk time" `tt = t - DESK_START`.

import type { Key } from '../lib/ease'
import type { Pt } from '../lib/shape'
import type { GhostColor, Theme } from '../theme'
import type { Camera, CursorNode, DisplayNode, LabelState, Rect, Ripple, SceneState, Vec3, WindowKind, WindowNode } from './types'

import { clamp } from 'es-toolkit'

import { mix } from '../lib/color'
import { bump, cubicBezier, easeInCubic, easeInOutCubic, easeInOutSine, easeOutBack, easeOutCubic, lerp, prog, track, vtrack } from '../lib/ease'
import { ghostTemplate, partTemplates, placedGhost, placedPart } from '../lib/icon'
import { bbox, morph, morphPts } from '../lib/shape'
import { partSlab, rectSlab, slabBox, windowToU } from '../lib/slab'
import { spring } from '../lib/spring'
import { GHOST_SCALE } from '../render/Overlay'
import { GHOSTS } from '../theme'
import { glowsFor } from './blobs'

/** Film time at which the desk choreography (desk time 0) starts. */
const DESK_START = 2.1

export const FILM = { duration: DESK_START + 14.8, height: 1080, width: 1920 }
/** Film time at which the mark has turned gray; the landing hero takes over here with an identical frame. */
export const FILM_HANDOFF = DESK_START + 13.95

// NOTICE: film() is synchronous and points these at the requested layout on
// entry, so the choreography below stays written against plain values.
let CX = FILM.width / 2
let CY = FILM.height / 2
export const ICON_SCALE = 1.7

// ---------------------------------------------------------------------------
// Cursor choreography

const U_KEYS: Key[] = [
  { t: 2.15, x: 1700, y: 980 },
  { arc: 0.06, ease: easeOutCubic, t: 2.75, x: 1010, y: 430 },
  { t: 3.5, x: 1000, y: 420 },
  { t: 4.5, x: 1004, y: 424 },
  { t: 5.3, x: 1430, y: 570 },
  { t: 6.2, x: 1420, y: 590 },
  { t: 7.0, x: 1120, y: 470 },
  { t: 7.3, x: 1110, y: 450 },
  { t: 8.6, x: 1112, y: 452 },
  { t: 9.6, x: 1050, y: 560 },
  { t: 10.4, x: 990, y: 480 },
  { t: 11.3, x: 995, y: 488 },
  { t: 12.0, x: 1040, y: 560 },
]
const U_CLICKS = [3.6, 5.42]

const G1_KEYS: Key[] = [
  { t: 2.8, x: 1012, y: 432 },
  { ease: easeOutCubic, t: 3.55, x: 1080, y: 500 },
  { t: 3.95, x: 1080, y: 500 },
  { t: 4.65, x: 450, y: 272 },
  { t: 4.85, x: 450, y: 272 },
  { arc: 0.04, t: 5.55, x: 290, y: 220 },
  { t: 5.7, x: 290, y: 220 },
  { t: 6.15, x: 615, y: 219 },
  { t: 6.5, x: 625, y: 232 },
  { t: 7.05, x: 1380, y: 322 },
  { t: 7.15, x: 1380, y: 322 },
  { arc: 0.04, t: 7.95, x: 1470, y: 190 },
  { t: 8.05, x: 1470, y: 190 },
  { t: 8.7, x: 980, y: 330 },
  { t: 9.4, x: 940, y: 380 },
  { t: 10.1, x: 770, y: 520 },
  { t: 10.85, x: 760, y: 530 },
  { t: 11.6, x: 735, y: 545 },
]
const G1_CLICKS = [6.22, 10.95]
const G1_DRAGS: [number, number][] = [[4.85, 5.55], [7.15, 7.95]]

const G2_KEYS: Key[] = [
  { t: 2.95, x: 1012, y: 432 },
  { ease: easeOutCubic, t: 3.7, x: 1330, y: 420 },
  { t: 4.6, x: 1345, y: 432 },
  { t: 6.4, x: 1338, y: 424 },
  { t: 7.2, x: 1470, y: 790 },
  { t: 7.6, x: 1290, y: 714 },
  { t: 7.95, x: 1290, y: 758 },
  { t: 8.3, x: 1290, y: 802 },
  { t: 9.0, x: 1420, y: 600 },
  { t: 9.8, x: 1250, y: 470 },
  { t: 10.6, x: 1200, y: 520 },
  { t: 11.1, x: 1190, y: 525 },
  { t: 11.6, x: 1215, y: 520 },
]
const G2_CLICKS = [7.68, 8.02, 8.38, 11.22]

const G3_KEYS: Key[] = [
  { t: 3.1, x: 1012, y: 432 },
  { ease: easeOutCubic, t: 3.85, x: 820, y: 850 },
  { t: 5.6, x: 812, y: 842 },
  { t: 7.35, x: 820, y: 850 },
  { t: 7.8, x: 700, y: 780 },
  { t: 9.3, x: 712, y: 786 },
  { t: 9.75, x: 1055, y: 620 },
  { t: 10.5, x: 980, y: 760 },
]
const G3_CLICKS = [9.85]

const G4_KEYS: Key[] = [
  { t: 10.7, x: 380, y: 760 },
  { t: 11.3, x: 300, y: 700 },
]

type KeyName = 'g1' | 'g2' | 'g3' | 'g4' | 'u'
/** Landscape-authored cursor tracks. */
const LANDSCAPE_KEYS: Record<KeyName, Key[]> = { g1: G1_KEYS, g2: G2_KEYS, g3: G3_KEYS, g4: G4_KEYS, u: U_KEYS }
/** Tracks for the active layout (set by film()). */
let K = LANDSCAPE_KEYS

interface LabelSpec { t0: number, t1: number, text: string }

const LABELS: Record<string, LabelSpec[]> = {
  g1: [
    { t0: 3.0, t1: 4.45, text: 'Hi! Let me help' },
    { t0: 4.45, t1: 5.7, text: 'Moving Notes aside' },
    { t0: 5.7, t1: 6.6, text: 'Clicking “New”' },
    { t0: 6.9, t1: 8.1, text: 'Making room' },
    { t0: 8.4, t1: 10.2, text: 'Reading the page' },
    { t0: 10.2, t1: 11.45, text: 'Wrapping up' },
  ],
  g2: [
    { t0: 3.4, t1: 6.3, text: 'Picking a playlist' },
    { t0: 7.35, t1: 8.7, text: 'Checking off tasks' },
    { t0: 9.0, t1: 10.3, text: 'Opening details' },
    { t0: 10.3, t1: 11.5, text: 'Almost done' },
  ],
  g3: [
    { t0: 3.55, t1: 7.5, text: 'Opening the REPL' },
    { t0: 7.5, t1: 9.7, text: 'Running a REPL script' },
    { t0: 9.7, t1: 11.25, text: 'Recording the run' },
  ],
  g4: [
    { t0: 10.85, t1: 11.5, text: 'Reading the chart' },
  ],
}

interface WinSpec {
  accent: GhostColor
  id: string
  /** Part of the opening: blooms open in the focused display, staggered by `order`. */
  intro?: { order: number }
  kind: WindowKind
  /** Desk time at which the window leaves (browser handled separately). */
  leave?: number
  /** Front-view desktop rect. */
  rect: Rect
  /** Spring-out from a click: desk time and point. */
  spawn?: { tt: number, x: number, y: number }
  title: string
}

export function labelAt(specs: LabelSpec[], t: number): LabelState | undefined {
  for (let i = 0; i < specs.length; i++) {
    const s = specs[i]
    if (t < s.t0 || t >= s.t1)
      continue
    const chained = i > 0 && Math.abs(specs[i - 1].t1 - s.t0) < 0.01
    const next = specs[i + 1]
    const continues = next && Math.abs(next.t0 - s.t1) < 0.01
    const popIn = chained ? 1 : easeOutBack(prog(t, s.t0, s.t0 + 0.42), 2.2)
    const popOut = continues ? 1 : 1 - prog(t, s.t1 - 0.22, s.t1, easeInOutCubic)
    const chars = Math.min(s.text.length, Math.floor((t - s.t0) / 0.028) + (chained ? 0 : 1))
    return { chars, pop: popIn * popOut, text: s.text }
  }
  return undefined
}

function dragOffset(keys: Key[], t: number, d0: number, d1: number) {
  if (t <= d0)
    return { x: 0, y: 0 }
  const a = track(keys, d0)
  const b = track(keys, Math.min(t, d1))
  return { x: b.x - a.x, y: b.y - a.y }
}

// ---------------------------------------------------------------------------
// Windows

function pressScale(t: number, clicks: number[], drags: [number, number][] = []) {
  let s = 1
  for (const c of clicks)
    s -= 0.2 * bump(t, c)
  for (const [d0, d1] of drags)
    s -= 0.12 * prog(t, d0 - 0.1, d0) * (1 - prog(t, d1, d1 + 0.1))
  return s
}

const SPECS: WinSpec[] = [
  { accent: 'amber', id: 'notes', intro: { order: 1 }, kind: 'notes', leave: 11.25, rect: { h: 400, w: 520, x: 300, y: 250 }, title: 'Notes' },
  { accent: 'cyan', id: 'browser', intro: { order: 0 }, kind: 'browser', rect: { h: 500, w: 680, x: 640, y: 170 }, title: 'Browser' },
  { accent: 'pink', id: 'music', intro: { order: 2 }, kind: 'music', leave: 11.32, rect: { h: 340, w: 460, x: 1200, y: 300 }, title: 'Music' },
  { accent: 'violet', id: 'term', intro: { order: 3 }, kind: 'term', leave: 11.39, rect: { h: 330, w: 600, x: 500, y: 600 }, title: 'auv repl' },
  { accent: 'pink', id: 'todo', kind: 'todo', leave: 11.46, rect: { h: 290, w: 380, x: 1260, y: 640 }, spawn: { tt: 6.24, x: 615, y: 219 }, title: 'Checklist' },
  { accent: 'lime', id: 'chart', kind: 'chart', leave: 11.53, rect: { h: 290, w: 400, x: 150, y: 560 }, spawn: { tt: 9.88, x: 1055, y: 620 }, title: 'Run stats' },
]

/** Opening beats, film time. */
const OPEN = {
  /** Yaw and pitch ease to the front view while the camera turns into a standard perspective. */
  align: [2.7, 5.0] as const,
  bloom: 4.15,
  cardsIn: [0.6, 1.5] as const,
  chrome: [4.25, 4.8] as const,
  /** The rest of the line-up and the lit card dissolve into the desk. */
  clear: [4.3, 4.95] as const,
  flatten: 0.7,
  hold: 0.45,
  /** The desk display lights up and opens to full size; the cards in front of it fade left to right. */
  lightUp: [3.3, 3.9] as const,
  open: [3.7, 4.7] as const,
  persp: [2.5, 3.6] as const,
  /** The line-up speeds off; see RUN for the speed curve. */
  run: 1.75,
  settle: 5.0,
  size: 1.25,
  sweep: 3.4,
  /**
   * The parts glide up the line onto their displays and only change shape as
   * they settle onto them. The "U" replays the ending backwards: the bottom
   * round flattens, the lean straightens, then it sizes into a window block.
   */
  travel: [0.55, 1.85] as const,
  /** Camera turns out from the front view to a high orthographic 3/4 view and pulls back. */
  turnOut: [0.45, 1.8] as const,
  unlean: 1.0,
  zoom: [3.5, 5.0] as const,
}
const BLOCK = { h: 70, r: 14, w: 96 }

const TURN_EASE = cubicBezier(0.3, 0, 0.12, 1)
const ALIGN_EASE = cubicBezier(0.45, 0, 0.2, 1)
const ZOOM_EASE = cubicBezier(0.45, 0, 0.08, 1)
const TRAVEL_EASE = cubicBezier(0.4, 0, 0.25, 1)

// ---------------------------------------------------------------------------
// Opening: the display line-up
//
// World space (Vec3): stage px, x right, y up, z toward the viewer, origin at
// the center of the display the camera finally settles on ("the desk
// display", index 0), which is exactly the desk once the camera is front on
// and the display is open. Positions along the line-up are in gaps along AXIS.
//
// The parts dock into displays far up the line (around index DOCKED) and fly
// off with it; the desk display is a fresh one that streams in from behind.
// The mark stays put at the absolute origin while the line-up runs, so in
// desk-display space it sits at DOCKED - lineTravel(t).

const v3 = {
  add: (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
  lerp: (a: Vec3, b: Vec3, t: number): Vec3 => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)],
  norm: (a: Vec3): Vec3 => v3.scale(a, 1 / Math.hypot(...a)),
  scale: (a: Vec3, k: number): Vec3 => [a[0] * k, a[1] * k, a[2] * k],
}

/**
 * The line-up runs into the screen and toward the top left. It is a little
 * off the front axis, so the front view lands while the line still reads as
 * slightly oblique rather than collapsing to a point.
 */
const AXIS = v3.norm([-0.3, 0.16, -1])
/** The high 3/4 view the camera turns out to. */
const VIEW = { pitch: 0.62, yaw: -0.65, zoom: 0.3 }
/** Displays in the line-up are this fraction of the desk; the desk display opens to full size. */
const CARD = 0.5
/** Where the line-up is when the parts dock, and its drift until the run. */
const DOCK = { s: 0.9, t: 1.85 }
const DRIFT = 0.4
/**
 * The run, as the speed of the line past the camera: it ramps up over
 * `RUN.rise` seconds, holds until `RUN.brake`, then brakes along (1 - u)^3 to
 * a stop at OPEN.settle, so the last displays slide in slowly.
 */
const RUN = { brake: 3.0, rise: 0.8 }
/** Only cards this many gaps around the camera's focus are drawn. */
const REACH = 9
/** Motion trail: copies of each card at these look-back times (seconds). */
const TRAIL = [0.006, 0.012, 0.018, 0.024]
/**
 * Index of the display the "U" docks into. The line-up must not run this far
 * before the camera settles, so the stream past the camera never reverses.
 */
const DOCKED = 60

// ---------------------------------------------------------------------------
// Layouts
//
// The choreography is authored once, on the landscape desktop. Another layout
// only moves the windows (same sizes); every cursor key, click, and spawn point
// is re-expressed as "this offset inside that window" and replayed against the
// new rects, so paths follow their windows. Points outside every window fall
// back to scaling the stage.

export interface FilmLayout {
  height: number
  id: 'landscape' | 'portrait'
  rects: Record<string, Rect>
  width: number
}

export const LANDSCAPE: FilmLayout = {
  height: 1080,
  id: 'landscape',
  rects: Object.fromEntries(SPECS.map(s => [s.id, s.rect])),
  width: 1920,
}

/** 9:16 desktop: the same windows, stacked down a tall screen. */
export const PORTRAIT: FilmLayout = {
  height: 1920,
  id: 'portrait',
  rects: {
    browser: { h: 500, w: 680, x: 200, y: 560 },
    chart: { h: 290, w: 400, x: 340, y: 1460 },
    music: { h: 340, w: 460, x: 520, y: 420 },
    notes: { h: 400, w: 520, x: 220, y: 250 },
    term: { h: 330, w: 600, x: 80, y: 1090 },
    todo: { h: 290, w: 380, x: 640, y: 1150 },
  },
  width: 1080,
}

let LAYOUT = LANDSCAPE

/**
 * Where "A" and "V" dock: a display index and a card-local (landscape) point.
 * Uneven slots, so it reads as three separate devices rather than one group.
 * The "U" docks into display DOCKED as a window block over the browser's spot.
 */
const SLOTS = [
  { card: DOCKED + 2, color: 'cyan' as const, part: 0, x: 1240, y: 430 },
  { card: DOCKED - 1, color: 'pink' as const, part: 2, x: 720, y: 640 },
]

function lineScale() {
  return { gap: 0.6 * CARD * Math.hypot(LAYOUT.width, LAYOUT.height) }
}

/** Line-up drift past the mark, in gaps (the mark only matters until the parts dock). */
function lineTravel(t: number) {
  return DOCK.s + DRIFT * (t - DOCK.t)
}

/** Shape of the run's speed: ramp up, hold, then brake to zero at OPEN.settle. */
function runShape(t: number) {
  if (t <= OPEN.run)
    return { drift: 1, run: 0 }
  const rise = prog(t, OPEN.run, OPEN.run + RUN.rise, easeInOutSine)
  const brake = (1 - prog(t, RUN.brake, OPEN.settle)) ** 3
  return { drift: 1 - rise, run: rise * brake }
}

/**
 * Remaining distance (gaps) from the camera's focus to the desk display, from
 * each table step to OPEN.settle. The run's peak speed is solved so the stream
 * that starts on the mark ends exactly on the desk display.
 * NOTICE: a fixed-step table keeps it deterministic; 1 ms steps are far below
 * one frame.
 */
const RUN_TABLE = (() => {
  const dt = 0.001
  const n = Math.ceil((OPEN.settle - OPEN.run) / dt)
  let drift = 0
  let run = 0
  const shapes = Array.from({ length: n }, (_, i) => runShape(OPEN.run + (i + 0.5) * dt))
  for (const sh of shapes) {
    drift += sh.drift * DRIFT * dt
    run += sh.run * dt
  }
  const start = DOCKED - lineTravel(OPEN.run) + lean(OPEN.run)
  const peak = (start - drift) / run
  const remaining = new Float64Array(n + 1)
  for (let i = n - 1; i >= 0; i--)
    remaining[i] = remaining[i + 1] + (shapes[i].drift * DRIFT + shapes[i].run * peak) * dt
  return { dt, peak, remaining }
})()

function cameraAt(t: number): Camera {
  const turn = prog(t, ...OPEN.turnOut, TURN_EASE)
  const align = prog(t, ...OPEN.align, ALIGN_EASE)
  const k = turn * (1 - align)
  return {
    at: v3.scale(AXIS, focusAt(t) * lineScale().gap),
    persp: prog(t, ...OPEN.persp, easeInOutCubic),
    pitch: VIEW.pitch * k,
    yaw: VIEW.yaw * k,
    // Log-space zoom, so pulling back and the final push-in feel even.
    zoom: VIEW.zoom ** (turn * (1 - prog(t, ...OPEN.zoom, ZOOM_EASE))),
  }
}

/** Center of card i in desk-display space. */
function cardCenter(i: number): Vec3 {
  return v3.scale(AXIS, i * lineScale().gap)
}

/** Fade from the card's own lifecycle: the light-up handover, the sweep in front, the final clear. */
function cardFade(i: number, t: number) {
  const clear = 1 - prog(t, ...OPEN.clear, easeInOutCubic)
  if (i === 0)
    return 1 - prog(t, ...OPEN.lightUp, easeInOutCubic)
  if (i < 0) {
    // Left to right: the card right in front of the desk display goes first.
    const t0 = OPEN.sweep + 0.1 * (-i - 1)
    return clear * (1 - prog(t, t0, t0 + 0.45, easeInOutCubic))
  }
  return clear
}

/** A stage point inside card i (scaled to the card), as a world offset from the card center. */
function cardPoint(i: number, x: number, y: number, t: number): Vec3 {
  const s = cardScale(i, t)
  const c = cardCenter(i)
  return [c[0] + (x - LAYOUT.width / 2) * s, c[1] + (LAYOUT.height / 2 - y) * s, c[2]]
}

/** Card size as a fraction of the desk; the desk display opens to full size. */
function cardScale(i: number, t: number) {
  return i === 0 ? lerp(CARD, 1, prog(t, ...OPEN.open, easeInOutCubic)) : CARD
}

/**
 * The line-up in desk-display space. Cards are brightest and sharpest near
 * where the camera looks and fall off (fade and defocus) along the line. While
 * the camera and the line move fast relative to each other, each card leaves a
 * short trail of where it just was.
 */
function displaysAt(t: number, theme: Theme): DisplayNode[] {
  const { gap } = lineScale()
  const focus = focusAt(t)
  const cardsIn = prog(t, ...OPEN.cardsIn, easeOutCubic)
  // The cards behind the desk display recede (per theme) as it lights up and its windows bloom.
  const behind = lerp(1, theme.displayBehindLit, prog(t, OPEN.lightUp[0], OPEN.bloom, easeInOutCubic))
  const speed = Math.abs(focus - focusAt(t - 1 / 60)) * 60
  const trail = clamp((speed - 2) / 10, 0, 1)
  const out: DisplayNode[] = []
  for (let i = Math.floor(focus - REACH); i <= Math.ceil(focus + REACH); i++) {
    const f = Math.abs(i - focus)
    const base = cardsIn * cardFade(i, t) * falloff(f) * (i > 0 ? behind : 1)
    if (base < 0.003)
      continue
    const s = cardScale(i, t)
    const w = LAYOUT.width * s
    const h = LAYOUT.height * s
    const soft = 0.1 * w * clamp((f - 1.5) / 6, 0, 1)
    const card = { h, radius: 0.035 * Math.min(w, h), w }
    out.push({ center: cardCenter(i), id: `display-${i}`, ...card, opacity: base, soft })
    if (trail <= 0)
      continue
    TRAIL.forEach((back, k) => {
      // Where the card sat relative to the camera `back` seconds ago.
      const shift = (focus - focusAt(t - back)) * gap
      out.push({ center: v3.add(cardCenter(i), v3.scale(AXIS, shift)), id: `display-${i}-trail-${k}`, ...card, opacity: base * trail * 0.25 * (1 - k / TRAIL.length), soft: soft + 0.015 * w * (k + 1) })
    })
  }
  return out
}

/** Brightness falloff along the line from where the camera looks (gaps away). */
function falloff(f: number) {
  return lerp(0.15, 1, Math.exp(-((f / 3.2) ** 2)))
}

/**
 * Where the camera looks along the line-up, relative to the desk display
 * (gaps). It sits on the mark until the run; then the line streams past at the
 * run's speed until the desk display slides in and stops. It only ever
 * decreases, so the stream never runs backwards.
 */
function focusAt(t: number) {
  if (t <= OPEN.run)
    return DOCKED - lineTravel(t) + lean(t)
  const x = (t - OPEN.run) / RUN_TABLE.dt
  const i = Math.min(RUN_TABLE.remaining.length - 2, Math.floor(x))
  if (i < 0 || t >= OPEN.settle)
    return 0
  return lerp(RUN_TABLE.remaining[i], RUN_TABLE.remaining[i + 1], x - i)
}

/** A desk rect scaled into the desk display at its current size. */
function inDeskDisplay(r: Rect, t: number): Rect {
  return scaledRect(r, cardScale(0, t))
}

/** The camera leans a little from the mark toward the docking slots. */
function lean(t: number) {
  return 0.4 * DOCK.s * prog(t, 0.9, 1.9, easeInOutSine)
}

/** The mark's fixed spot in desk-display space. */
function markAt(t: number): Vec3 {
  return v3.scale(AXIS, (DOCKED - lineTravel(t)) * lineScale().gap)
}

/** A desk rect scaled into a display of scale `s` (stage coordinates around the stage center). */
function scaledRect(r: Rect, s: number): Rect {
  return { h: r.h * s, w: r.w * s, x: CX + (r.x - CX) * s, y: CY + (r.y - CY) * s }
}

/** The desk display once lit: a real glass card (sharp, with a shadow) that opens and hands over to the desk. */
function screenAt(t: number, theme: Theme): null | WindowNode {
  const opacity = prog(t, ...OPEN.lightUp, easeInOutCubic) * (1 - prog(t, ...OPEN.clear, easeInOutCubic))
  if (opacity <= 0.001)
    return null
  const rect = inDeskDisplay({ h: LAYOUT.height, w: LAYOUT.width, x: 0, y: 0 }, t)
  return { accent: 'cyan', chrome: 0, data: {}, fill: theme.displayLit, glow: 0, id: 'screen', kind: 'notes', opacity, radius: 0.035 * Math.min(rect.w, rect.h), rect, title: '', z: 0 }
}

/** Shift stage-space points onto a parallel plane offset by `o`. */
function shifted(pts: Pt[], o: Vec3): Pt[] {
  return pts.map(([x, y]) => [x + o[0], y - o[1]])
}

/**
 * Fixed stacking order for the whole film. Changing it mid-film (bring to
 * front on click) made layers visibly swap, so every interaction is placed
 * where the target is already visible instead.
 */
const Z: Record<string, number> = { browser: 2, chart: 6, music: 3, notes: 1, term: 4, todo: 5 }

function exists(id: string, tt: number) {
  const spec = SPECS.find(s => s.id === id)!
  return !spec.spawn || tt >= spec.spawn.tt
}

/** Map a landscape point at desk time tt into `layout`. */
function mapPoint(layout: FilmLayout, x: number, y: number, tt: number) {
  if (layout === LANDSCAPE)
    return { x, y }
  const ids = Object.keys(Z).sort((a, b) => Z[b] - Z[a])
  for (const id of ids) {
    if (!exists(id, tt))
      continue
    const l = rectAt(LANDSCAPE, id, tt)
    if (x >= l.x && x <= l.x + l.w && y >= l.y && y <= l.y + l.h) {
      const p = rectAt(layout, id, tt)
      return { x: p.x + (x - l.x), y: p.y + (y - l.y) }
    }
  }
  return { x: (x * layout.width) / LANDSCAPE.width, y: (y * layout.height) / LANDSCAPE.height }
}

/** Window rect at desk time tt in a layout: its base rect plus any drag so far. */
function rectAt(layout: FilmLayout, id: string, tt: number): Rect {
  const r = { ...layout.rects[id] }
  const drag = id === 'notes' ? G1_DRAGS[0] : id === 'music' ? G1_DRAGS[1] : undefined
  if (drag) {
    // Drag offsets come from the landscape track, so both layouts move a window equally.
    const d = dragOffset(G1_KEYS, tt, ...drag)
    r.x += d.x
    r.y += d.y
  }
  return r
}

const keyCache = new Map<string, Record<KeyName, Key[]>>()
interface GhostSpec {
  clicks: number[]
  color: GhostColor
  drags?: [number, number][]
  /** Fade-out window, or a morph into an icon part. */
  fade?: [number, number]
  id: string
  keys: KeyName
  morphInto?: { part: number, t0: number, t1: number }
  spawn: number
}

function dataAt(id: string, tt: number) {
  switch (id) {
    case 'browser': return { scroll: vtrack([[3.7, 0], [4.4, 110], [7.4, 110], [8.3, 260]], tt) }
    case 'chart': return { bars: prog(tt, 10.55, 11.2, easeOutCubic) }
    case 'music': return { playing: tt - 5.5 }
    case 'term': return { output: prog(tt, 9.9, 10.4), t: tt, typed: prog(tt, 7.9, 9.4) }
    case 'todo': return { checks: [7.68, 8.02, 8.38].reduce((a, c) => a + prog(tt, c, c + 0.3), 0) }
    default: return {}
  }
}

/**
 * The docked parts. They keep their shape while gliding up the line and change
 * only as they settle onto their own displays, fading to the docked gray:
 * "A" and "V" into ghost cursors, the "U" into a plain window block (the
 * ending's window -> "U" springs, backwards and in reverse order). Then they
 * fly off with the line-up. They stay GL outlines (the flat Overlay cannot
 * follow the turned camera) and fade with their cards.
 */
function dockedAt(t: number, theme: Theme): WindowNode[] {
  if (t < OPEN.hold)
    return []
  const d = prog(t, ...OPEN.travel, TRAVEL_EASE)
  const m = prog(d, 0.4, 1, easeInOutSine)
  const fill = (part: number) => mix(theme.icon[part], theme.docked, prog(d, 0.6, 1, easeInOutCubic))
  // Full strength while still part of the mark (loop seam), then like their cards.
  const opacity = (card: number) => cardFade(card, t) * lerp(1, falloff(Math.abs(card - focusAt(t))), d)
  const base = { chrome: 0, data: {}, glow: 0, kind: 'notes' as const, title: '' }
  const out: WindowNode[] = []

  const uOpacity = opacity(DOCKED)
  if (uOpacity > 0.001) {
    const round = 1 - spring(t, OPEN.flatten, { f: 1.3, z: 0.5 })
    const lean = 1 - spring(t, OPEN.unlean, { f: 1.4, z: 0.55 })
    const shrink = 1 - spring(t, OPEN.size, { f: 1.1, z: 0.62 })
    const block = scaledRect(LAYOUT.rects.browser, CARD)
    const slab = windowToU(rectSlab(block.x, block.y, block.w, block.h, 18 * CARD), partSlab(ICON_SCALE, CX, CY), shrink, lean, round)
    const o = v3.lerp(markAt(t), cardCenter(DOCKED), d)
    out.push({ ...base, accent: 'violet', depth: o[2], fill: fill(1), id: 'dock-1', opacity: uOpacity, rect: slabBox(slab), slab: { ...slab, x: slab.x + o[0], y: slab.y - o[1] }, z: 7 })
  }

  for (const g of SLOTS) {
    const op = opacity(g.card)
    if (op <= 0.001)
      continue
    const at = mapPoint(LAYOUT, g.x, g.y, 0)
    const slot = cardPoint(g.card, at.x, at.y, t)
    // Both shapes are drawn around the stage center (world origin) and carried
    // by one offset: from the mark's spot to the ghost's slot in its display.
    const ghost = placedGhost(CX, CY, GHOST_SCALE * 2.2)
    const o = v3.lerp(markAt(t), slot, d)
    const poly = shifted(morphPts(placedPart(g.part, ICON_SCALE, CX, CY), ghost, m, [partTemplates()[g.part], ghostTemplate()]), o)
    const box = bbox(poly)
    out.push({ ...base, accent: g.color, depth: o[2], fill: fill(g.part), id: `dock-${g.part}`, opacity: op, poly, rect: { h: box.h, w: box.w, x: box.x, y: box.y }, z: 8 + g.part })
  }
  return out
}

function keysFor(layout: FilmLayout) {
  if (layout === LANDSCAPE)
    return LANDSCAPE_KEYS
  let k = keyCache.get(layout.id)
  if (!k) {
    const map = (keys: Key[]) => keys.map(key => ({ ...key, ...mapPoint(layout, key.x, key.y, key.t) }))
    k = { g1: map(G1_KEYS), g2: map(G2_KEYS), g3: map(G3_KEYS), g4: map(G4_KEYS), u: map(U_KEYS) }
    keyCache.set(layout.id, k)
  }
  return k
}

// ---------------------------------------------------------------------------
// Cursors

function windowAt(spec: WinSpec, t: number, theme: Theme): null | WindowNode {
  const tt = t - DESK_START
  if (spec.spawn && tt < spec.spawn.tt)
    return null

  const rect = rectAt(LAYOUT, spec.id, tt)

  const node: WindowNode = {
    accent: spec.accent,
    chrome: 1,
    data: dataAt(spec.id, tt),
    id: spec.id,
    kind: spec.kind,
    opacity: 1,
    rect,
    title: spec.title,
    z: Z[spec.id],
  }

  // Opening: every window blooms in the focused display as it opens.
  if (spec.intro) {
    const t0 = OPEN.bloom + 0.07 * spec.intro.order
    // Critically damped: no overshoot while the camera is still settling.
    const open = spring(t, t0, { f: 1, z: 1 })
    if (open <= 0)
      return null
    const accent = mix(GHOSTS[spec.accent].fill, theme.name === 'light' ? '#ffffff' : '#000000', 0.18)
    const glass = spec.kind === 'term' ? theme.termTint : theme.tint
    const target = inDeskDisplay(rect, t)
    const w = lerp(BLOCK.w, target.w, open)
    const h = lerp(BLOCK.h, target.h, open)
    node.rect = { h, w, x: target.x + (target.w - w) / 2, y: target.y + (target.h - h) / 2 }
    node.contentSize = { h: rect.h, w: rect.w }
    node.radius = lerp(BLOCK.r, 18, open)
    node.opacity = prog(t, t0, t0 + 0.18, easeOutCubic)
    node.fill = mix(accent, glass, clamp(open, 0, 1))
    node.chrome = prog(t, ...OPEN.chrome, easeOutCubic)
    node.glow = prog(t, t0, OPEN.settle + 0.3)
  }

  // Spring out of the click that created it.
  if (spec.spawn) {
    node.scale = spring(tt, spec.spawn.tt, { f: 1.25, z: 0.58 })
    const from = mapPoint(LAYOUT, spec.spawn.x, spec.spawn.y, spec.spawn.tt)
    node.origin = { x: from.x - rect.x, y: from.y - rect.y }
    node.opacity = prog(tt, spec.spawn.tt, spec.spawn.tt + 0.08)
  }

  // Leave: a quick shrink-and-fade toward the window's own center.
  if (spec.leave !== undefined && tt >= spec.leave) {
    node.scale = 1 - 0.3 * spring(tt, spec.leave, { f: 1.6, z: 0.9 })
    node.origin = undefined
    node.opacity = 1 - prog(tt, spec.leave, spec.leave + 0.35, easeInCubic)
    return node.opacity > 0 ? node : null
  }

  // The browser shrinks, leans, then rounds its bottom edge into the "U".
  if (spec.id === 'browser' && tt >= 11.3) {
    node.chrome = 1 - prog(tt, 11.3, 11.55)
    // Its light leaves with it, so the final mark sits on a clean desk (loop seam).
    node.glow = 1 - prog(tt, 11.4, 12.5, easeInOutCubic)
    if (tt >= 11.45) {
      node.slab = windowToU(
        rectSlab(rect.x, rect.y, rect.w, rect.h, 18),
        partSlab(ICON_SCALE, CX, CY),
        spring(tt, 11.45, { f: 1.1, z: 0.62 }),
        spring(tt, 11.75, { f: 1.4, z: 0.55 }),
        spring(tt, 11.98, { f: 1.3, z: 0.5 }),
      )
      node.fill = mix(theme.tint, GHOSTS.violet.fill, prog(tt, 11.5, 11.95))
    }
  }
  return node
}

const GHOST_SPECS: GhostSpec[] = [
  { clicks: G1_CLICKS, color: 'cyan', drags: G1_DRAGS, id: 'g1', keys: 'g1', morphInto: { part: 0, t0: 11.6, t1: 12.65 }, spawn: 2.8 },
  { clicks: G2_CLICKS, color: 'pink', id: 'g2', keys: 'g2', morphInto: { part: 2, t0: 11.68, t1: 12.75 }, spawn: 2.95 },
  { clicks: G3_CLICKS, color: 'violet', fade: [11.3, 11.7], id: 'g3', keys: 'g3', spawn: 3.1 },
  { clicks: [], color: 'lime', fade: [11.5, 11.9], id: 'g4', keys: 'g4', spawn: 10.7 },
]

function ghostAt(spec: GhostSpec, tt: number): CursorNode | null {
  if (tt < spec.spawn)
    return null
  const p = track(K[spec.keys], tt)
  const spawn = prog(tt, spec.spawn, spec.spawn + 0.5)
  const node: CursorNode = {
    color: spec.color,
    id: spec.id,
    kind: 'ghost',
    label: labelAt(LABELS[spec.id], tt),
    opacity: prog(tt, spec.spawn, spec.spawn + 0.1),
    scale: easeOutBack(spawn, 2.6) * pressScale(tt, spec.clicks, spec.drags),
    x: p.x,
    y: p.y,
  }
  if (spec.fade) {
    node.opacity *= 1 - prog(tt, ...spec.fade)
    if (node.opacity <= 0)
      return null
  }
  const m = spec.morphInto
  if (m && tt >= m.t0) {
    const f = prog(tt, m.t0, m.t1, easeInOutCubic)
    node.outline = morph(placedGhost(p.x, p.y, GHOST_SCALE), placedPart(m.part, ICON_SCALE, CX, CY), f, [ghostTemplate(), partTemplates()[m.part]])
    node.outlineFill = GHOSTS[spec.color].fill
  }
  return node
}

function ripplesAt(tt: number): Ripple[] {
  const out: Ripple[] = []
  const add = (clicks: number[], keys: Key[], color: string) => {
    for (const c of clicks) {
      const p = (tt - c) / 0.6
      if (p < 0 || p > 1)
        continue
      const at = track(keys, c)
      out.push({ color, p: easeOutCubic(p), x: at.x, y: at.y })
    }
  }
  add(U_CLICKS, K.u, 'rgba(120,128,150,0.9)')
  add(G1_CLICKS, K.g1, GHOSTS.cyan.fill)
  add(G2_CLICKS, K.g2, GHOSTS.pink.fill)
  add(G3_CLICKS, K.g3, GHOSTS.violet.fill)
  return out
}

function userAt(tt: number): CursorNode | null {
  if (tt < 2.15 || tt > 12)
    return null
  const p = track(K.u, tt)
  return {
    color: 'cyan',
    id: 'user',
    kind: 'user',
    opacity: prog(tt, 2.15, 2.35) * (1 - prog(tt, 11.35, 11.9)),
    scale: pressScale(tt, U_CLICKS),
    x: p.x,
    y: p.y,
  }
}

// ---------------------------------------------------------------------------

/** Desk time when every part has landed and the exact mark takes over. */
const LANDED = 12.95
/** Desk time the mark starts turning gray. */
const SNAP = 13.0
const GRAY_EASE = cubicBezier(0.4, 0, 0.2, 1)

export function film(t: number, theme: Theme, layout: FilmLayout = LANDSCAPE): SceneState {
  LAYOUT = layout
  CX = layout.width / 2
  CY = layout.height / 2
  K = keysFor(layout)
  const tt = t - DESK_START
  const windows = [screenAt(t, theme), ...SPECS.map(s => windowAt(s, t, theme)), ...dockedAt(t, theme)].filter((w): w is WindowNode => !!w)
  const cursors = [userAt(tt), ...GHOST_SPECS.map(s => ghostAt(s, tt))].filter((c): c is CursorNode => !!c)
  const state: SceneState = { blobs: [], cursors, glows: glowsFor(windows, t), ripples: ripplesAt(tt), windows }

  // Opening: the turned camera over the line-up. At OPEN.settle it is exactly
  // the flat front view, so dropping it there leaves no seam.
  if (t >= OPEN.hold && t < OPEN.settle) {
    state.camera = cameraAt(t)
    state.displays = displaysAt(t, theme)
  }

  // Loop seam: the first frames are the same exact mark the film ends on.
  if (t < OPEN.hold) {
    state.windows = []
    state.icon = { color: 0, cx: CX, cy: CY, opacity: 1, s: ICON_SCALE }
  }

  if (tt >= LANDED) {
    state.windows = state.windows.filter(w => w.id !== 'browser')
    state.glows = glowsFor(state.windows, t)
    state.cursors = state.cursors.filter(c => !c.outline)
    // The color leaves A, then U, then V, like a wave across the mark; OKLab
    // mixing (IconView) keeps the fade clean instead of muddy.
    const fade = (i: number) => 1 - prog(tt, SNAP + 0.1 * i, SNAP + 0.1 * i + 0.7, GRAY_EASE)
    state.icon = { color: [fade(0), fade(1), fade(2)] as [number, number, number], cx: CX, cy: CY, opacity: 1, s: ICON_SCALE }
  }
  return state
}

/** Exposed for the landing page so it can continue from the final frame. */
export const FILM_ICON = { cx: FILM.width / 2, cy: FILM.height / 2, s: ICON_SCALE }

export function filmIcon(layout: FilmLayout) {
  return { cx: layout.width / 2, cy: layout.height / 2, s: ICON_SCALE }
}
