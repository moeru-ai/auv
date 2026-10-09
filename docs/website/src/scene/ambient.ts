// Endless background desk for the landing page, in film stage coordinates
// (1920x1080). Each window has its own ghost cursor working on a loop, and an
// explainer tying the loop to something AUV actually does.
//
// NOTICE: explainer copy and code snippets are provisional marketing drafts.
// They should be checked against README "Capability Matrix" before shipping.

import type { Key } from '../lib/ease'
import type { GhostColor } from '../theme'
import type { CursorNode, Rect, Ripple, SceneState, WindowKind, WindowNode } from './types'

import { bump, easeOutBack, easeOutCubic, prog, track } from '../lib/ease'
import { GHOSTS } from '../theme'
import { glowsFor } from './blobs'
import { labelAt } from './film'

export interface Desk {
  ghost: GhostColor
  id: string
  info: DeskInfo
  kind: WindowKind
  labels: string[]
  /**
   * Waypoints in window-local fractions; the ghost clicks on arrival. Empty for
   * windows nobody clicks in (the REPL runs code, so it has no ghost).
   */
  path: [number, number][]
  /** Loop length and phase, seconds. */
  period: number
  phase: number
  rect: Rect
  title: string
}

export interface DeskInfo {
  body: string
  code: string
  title: string
}

export const DESKS: Desk[] = [
  {
    ghost: 'amber',
    id: 'a-browser',
    info: { body: 'OCR runs on a capture AUV already holds (macOS Vision, Windows OCR, Tesseract on Linux). The click lands on the match.', code: 'await win.findText(\'Pricing\').click()', title: 'Find text, then act' },
    kind: 'browser',
    labels: ['findText(“Pricing”)', 'Clicking the match'],
    path: [[0.3, 0.55], [0.7, 0.7], [0.5, 0.85], [0.82, 0.4]],
    period: 8,
    phase: 1.3,
    rect: { h: 300, w: 480, x: 60, y: 70 },
    title: 'Browser',
  },
  {
    ghost: 'violet',
    id: 'a-trace',
    info: { body: 'Inputs, captures, and results become a run with artifacts you can inspect, trace, and replay.', code: 'run · 5 steps · 3 captures · replayable', title: 'Every run is recorded' },
    kind: 'trace',
    labels: ['Replaying the run', 'Inspecting a capture'],
    path: [[0.2, 0.86], [0.5, 0.86], [0.8, 0.86], [0.5, 0.3]],
    period: 7,
    phase: 0.4,
    rect: { h: 260, w: 480, x: 720, y: 36 },
    title: 'Run 2026-10-08 · 5 steps',
  },
  {
    ghost: 'pink',
    id: 'a-music',
    info: { body: 'App crates such as auv-apple-music turn an app into commands, instead of a screenshot-and-guess loop.', code: 'music.play({ query: \'lo-fi\' })', title: 'Apps as typed operations' },
    kind: 'music',
    labels: ['Queueing songs', 'Next track'],
    path: [[0.5, 0.84], [0.62, 0.84], [0.7, 0.45], [0.38, 0.84]],
    period: 6.5,
    phase: 2.1,
    rect: { h: 320, w: 460, x: 1390, y: 80 },
    title: 'Music',
  },
  {
    ghost: 'cyan',
    id: 'a-scan',
    info: { body: 'Scroll scan keeps every row exactly once, skips repeats, and reports why it stopped.', code: '→ 128 rows · stop: end of list', title: 'Scroll until it really ends' },
    kind: 'scan',
    labels: ['Scrolling the list', 'Keeping each row once'],
    path: [[0.5, 0.5], [0.5, 0.62], [0.5, 0.38], [0.5, 0.5]],
    period: 7.5,
    phase: 4.4,
    rect: { h: 380, w: 420, x: 80, y: 600 },
    title: 'Inbox',
  },
  {
    ghost: 'lime',
    id: 'a-term',
    info: { body: 'The same typed operation runs from the CLI, the TypeScript SDK, or an agent over MCP.', code: 'CLI · TypeScript · MCP · gRPC', title: 'One operation, every frontend' },
    kind: 'term',
    labels: [],
    path: [],
    period: 9,
    phase: 0.7,
    rect: { h: 300, w: 440, x: 1430, y: 620 },
    title: 'auv repl',
  },
  {
    ghost: 'cyan',
    id: 'a-notes',
    info: { body: 'On macOS, AUV can deliver clicks to a background window, so you keep working while it does.', code: 'await notes.findText(\'New\').click()', title: 'Input without taking your mouse' },
    kind: 'notes',
    labels: ['Clicking in the background', 'Your cursor stays yours'],
    path: [[0.88, 0.08], [0.5, 0.45], [0.3, 0.7], [0.12, 0.17]],
    period: 7,
    phase: 3.2,
    rect: { h: 260, w: 420, x: 560, y: 830 },
    title: 'Notes',
  },
]

export function ambient(t: number, appear: number, width: number, height: number, focus: null | string = null, visibleW = 1920, rectOf?: (d: Desk) => Rect): SceneState {
  const windows: WindowNode[] = []
  const cursors: CursorNode[] = []
  const ripples: Ripple[] = []

  DESKS.forEach((d, i) => {
    const a = easeOutCubic(prog(appear, i * 0.08, 0.5 + i * 0.08))
    if (a <= 0)
      return
    const float = { x: Math.sin(t * 0.3 + i) * 6, y: Math.cos(t * 0.25 + i * 2) * 6 }
    // Other layouts (the mobile collage and gallery) place the desks themselves.
    const base = rectOf ? rectOf(d) : deskRect(d, visibleW)
    const rect = { ...base, x: base.x + float.x, y: base.y + float.y + (1 - a) * 30 }

    const lt = (t + d.phase) % d.period
    const loop = Math.floor((t + d.phase) / d.period)

    // NOTICE: 15 fps content under the veil is indistinguishable after the blur
    // and lets memoized window content skip most frames.
    const focused = d.id === focus
    const q = (v: number) => (focused ? v : Math.floor(v * 15) / 15)
    const tq = q(t)
    const lq = q(lt)
    let data: WindowNode['data'] = {}
    switch (d.kind) {
      case 'browser':
        data = { scroll: 60 + Math.sin(tq * 0.4 + i) * 60 }
        break
      case 'music':
        data = { playing: tq }
        break
      case 'scan':
        data = { scroll: tq * 46 }
        break
      case 'term':
        data = { output: prog(lq, d.period * 0.72, d.period * 0.9), t: Math.floor(t * 2.5) / 2.5, typed: prog(lq, 0.4, d.period * 0.65) }
        break
      case 'trace':
        data = { t: tq }
        break
    }

    windows.push({ accent: d.ghost, chrome: 1, data, flat: !focused, id: d.id, kind: d.kind, opacity: a, rect, title: d.title, z: i })
    if (d.path.length === 0)
      return

    const step = d.period / d.path.length
    // Waypoint k is reached at (k + 0.6) * step; the ghost then dwells and clicks.
    const keys: Key[] = []
    const last = d.path[d.path.length - 1]
    keys.push({ t: 0, x: rect.x + last[0] * rect.w, y: rect.y + last[1] * rect.h })
    d.path.forEach(([px, py], k) => {
      keys.push({ arc: 0.12, t: (k + 0.6) * step, x: rect.x + px * rect.w, y: rect.y + py * rect.h })
      keys.push({ t: (k + 1) * step, x: rect.x + px * rect.w, y: rect.y + py * rect.h })
    })
    const clicks = d.kind === 'scan' ? [] : d.path.map((_, k) => (k + 0.68) * step)
    const p = track(keys, lt)

    const press = clicks.reduce((s, c) => s - 0.2 * bump(lt, c), 1)
    const text = d.labels[loop % d.labels.length]
    cursors.push({
      color: d.ghost,
      id: `${d.id}-ghost`,
      kind: 'ghost',
      label: labelAt([{ t0: 0.3, t1: d.period * 0.55, text }], lt),
      opacity: prog(appear, 0.4 + i * 0.08, 0.6 + i * 0.08),
      scale: easeOutBack(prog(appear, 0.4 + i * 0.08, 0.8 + i * 0.08), 2.4) * press,
      x: p.x,
      y: p.y,
    })
    for (const c of clicks) {
      const rp = (lt - c) / 0.6
      if (rp >= 0 && rp <= 1) {
        const at = track(keys, c)
        ripples.push({ color: GHOSTS[d.ghost].fill, p: easeOutCubic(rp), x: at.x, y: at.y })
      }
    }
  })

  return { blobs: [], cursors, glows: glowsFor(windows, t), ripples, windows }
}

/**
 * `appear` (0..1) fades the desk in after the intro film. `focus` is the window
 * revealed through the veil; the rest sit under a blur, so they skip their own
 * glass and update their content at a lower rate.
 */
/**
 * Desk rects for a stage whose visible width is `visibleW` (cover-fit on
 * narrower screens such as iPad landscape crops the sides): windows slide in
 * so they stay fully on screen.
 */
export function deskRect(d: Desk, visibleW = 1920): Rect {
  const left = (1920 - visibleW) / 2 + 24
  const right = (1920 + visibleW) / 2 - 24 - d.rect.w
  return { ...d.rect, x: Math.min(Math.max(d.rect.x, left), Math.max(left, right)) }
}
