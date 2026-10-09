// Parametric "slab": a rounded rectangle with independent elliptical corners,
// sheared horizontally about its top edge.
//
// The middle part of the AUV mark is exactly such a shape. Unshearing its path
// (x' = x - 0.3839 * (y - 96)) gives a 68.01 x 92.42 rectangle whose corners are
// quarter ellipses drawn with the standard 0.5523 Bezier constant:
//
//   top-left  14.15 x 13.20     top-right    10.61 x  9.91
//   bot-left  28.46 x 26.58     bot-right    39.54 x 36.90
//
// The two bottom corners add up to the full width, which is what makes the
// bottom read as one asymmetric "U" curve. Interpolating these parameters lets
// a window shrink, lean, then round its bottom edge, and land on the exact mark.

import { lerp } from './ease'
import { ICON_CENTER } from './icon'

export type Corner = [rx: number, ry: number]

export interface Slab {
  bl: Corner
  br: Corner
  h: number
  /** Horizontal shear: X = x' + k * (y' - y). */
  k: number
  tl: Corner
  tr: Corner
  w: number
  x: number
  y: number
}

const KAPPA = 0.5523

const PART2 = { bl: [28.46, 26.58], br: [39.54, 36.9], h: 92.42, k: 0.3839, tl: [14.15, 13.2], tr: [10.61, 9.91], w: 68.01, x: 92.36, y: 96 } as const

/** The mark's middle part placed like `placedPart(1, s, cx, cy)`. */
export function partSlab(s: number, cx: number, cy: number): Slab {
  const c = (r: readonly [number, number]): Corner => [r[0] * s, r[1] * s]
  return {
    bl: c(PART2.bl),
    br: c(PART2.br),
    h: PART2.h * s,
    k: PART2.k,
    tl: c(PART2.tl),
    tr: c(PART2.tr),
    w: PART2.w * s,
    x: (PART2.x - ICON_CENTER.x) * s + cx,
    y: (PART2.y - ICON_CENTER.y) * s + cy,
  }
}

export function rectSlab(x: number, y: number, w: number, h: number, r: number): Slab {
  return { bl: [r, r], br: [r, r], h, k: 0, tl: [r, r], tr: [r, r], w, x, y }
}

const lc = (a: Corner, b: Corner, t: number): Corner => [lerp(a[0], b[0], t), lerp(a[1], b[1], t)]

export type SlabCmd
  = | { c1: [number, number], c2: [number, number], op: 'C', p: [number, number] }
    | { op: 'L' | 'M', p: [number, number] }

export function lerpSlab(a: Slab, b: Slab, t: number): Slab {
  return {
    bl: lc(a.bl, b.bl, t),
    br: lc(a.br, b.br, t),
    h: lerp(a.h, b.h, t),
    k: lerp(a.k, b.k, t),
    tl: lc(a.tl, b.tl, t),
    tr: lc(a.tr, b.tr, t),
    w: lerp(a.w, b.w, t),
    x: lerp(a.x, b.x, t),
    y: lerp(a.y, b.y, t),
  }
}

export function slabBox(s: Slab) {
  const shift = s.k * s.h
  return { h: s.h, w: s.w + Math.abs(shift), x: s.x + Math.min(0, shift), y: s.y }
}

/** Outline commands in stage space (sheared), shared by the SVG path and WebGL shapes. */
export function slabCommands(input: Slab): SlabCmd[] {
  const s = fit(input)
  const { bl, br, h, k, tl, tr, w, x, y } = s
  const P = (px: number, py: number): [number, number] => [px + k * (py - y), py]
  return [
    { op: 'M', p: P(x + tl[0], y) },
    { op: 'L', p: P(x + w - tr[0], y) },
    { c1: P(x + w - tr[0] + KAPPA * tr[0], y), c2: P(x + w, y + tr[1] - KAPPA * tr[1]), op: 'C', p: P(x + w, y + tr[1]) },
    { op: 'L', p: P(x + w, y + h - br[1]) },
    { c1: P(x + w, y + h - br[1] + KAPPA * br[1]), c2: P(x + w - br[0] + KAPPA * br[0], y + h), op: 'C', p: P(x + w - br[0], y + h) },
    { op: 'L', p: P(x + bl[0], y + h) },
    { c1: P(x + bl[0] - KAPPA * bl[0], y + h), c2: P(x, y + h - bl[1] + KAPPA * bl[1]), op: 'C', p: P(x, y + h - bl[1]) },
    { op: 'L', p: P(x, y + tl[1]) },
    { c1: P(x, y + tl[1] - KAPPA * tl[1]), c2: P(x + tl[0] - KAPPA * tl[0], y), op: 'C', p: P(x + tl[0], y) },
  ]
}

/** SVG path for the slab, translated by (-ox, -oy). */
export function slabPath(input: Slab, ox = 0, oy = 0) {
  const f = ([px, py]: [number, number]) => `${(px - ox).toFixed(2)} ${(py - oy).toFixed(2)}`
  return `${slabCommands(input).map(c => (c.op === 'C' ? `C${f(c.c1)} ${f(c.c2)} ${f(c.p)}` : `${c.op}${f(c.p)}`)).join('')}Z`
}

/**
 * Window -> "U" in three overlapping springs: shrink to the slab's footprint,
 * lean into the shear, then pull the bottom edge into its round. Each stage
 * value is a spring progress (may overshoot slightly past 1).
 */
export function windowToU(win: Slab, target: Slab, shrink: number, lean: number, round: number): Slab {
  const smallR = target.tl
  const footprint: Slab = { ...target, bl: smallR, br: smallR, k: 0, tl: smallR, tr: target.tr }
  const s = lerpSlab(win, footprint, shrink)
  return {
    ...s,
    bl: lc(s.bl, target.bl, round),
    br: lc(s.br, target.br, round),
    k: lerp(0, target.k, lean),
    tl: lc(s.tl, target.tl, lean),
    tr: lc(s.tr, target.tr, lean),
  }
}

/** Keep corners from overlapping when a spring overshoots. */
function fit(s: Slab): Slab {
  const pos = (c: Corner): Corner => [Math.max(0, c[0]), Math.max(0, c[1])]
  let { bl, br, tl, tr } = { bl: pos(s.bl), br: pos(s.br), tl: pos(s.tl), tr: pos(s.tr) }
  const sx = (a: Corner, b: Corner) => {
    const sum = a[0] + b[0]
    return sum > s.w ? s.w / sum : 1
  }
  const sy = (a: Corner, b: Corner) => {
    const sum = a[1] + b[1]
    return sum > s.h ? s.h / sum : 1
  }
  const top = sx(tl, tr)
  const bottom = sx(bl, br)
  const left = sy(tl, bl)
  const right = sy(tr, br)
  tl = [tl[0] * top, tl[1] * left]
  tr = [tr[0] * top, tr[1] * right]
  br = [br[0] * bottom, br[1] * right]
  bl = [bl[0] * bottom, bl[1] * left]
  return { ...s, bl, br, tl, tr }
}
