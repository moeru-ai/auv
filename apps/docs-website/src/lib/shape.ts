// Polygon morphing between arbitrary closed outlines (icon parts, cursors,
// window rectangles). Shapes are resampled to the same point count at equal arc
// length, wound the same way, and index-aligned before point-wise lerp.

import { svgPathProperties as SvgPathProperties } from 'svg-path-properties'

export type Pt = [number, number]

export const SAMPLES = 160

const sampleCache = new Map<string, Pt[]>()

export function bbox(pts: Pt[]) {
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (const [x, y] of pts) {
    if (x < x0)
      x0 = x
    if (y < y0)
      y0 = y
    if (x > x1)
      x1 = x
    if (y > y1)
      y1 = y
  }
  return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, h: y1 - y0, w: x1 - x0, x: x0, y: y0 }
}

/** Move the shape so its bbox center lands at (cx, cy), scaled so its height is `h`. */
export function placeByHeight(pts: Pt[], cx: number, cy: number, h: number): Pt[] {
  const b = bbox(pts)
  const s = h / b.h
  return pts.map(([x, y]) => [(x - b.cx) * s + cx, (y - b.cy) * s + cy])
}

/** Equal-arc-length points around a rounded rectangle, clockwise from the top edge. */
export function roundedRect(x: number, y: number, w: number, h: number, r: number, n = SAMPLES): Pt[] {
  r = Math.min(r, w / 2, h / 2)
  const sw = w - 2 * r
  const sh = h - 2 * r
  const arc = (Math.PI / 2) * r
  const segs = [sw, arc, sh, arc, sw, arc, sh, arc]
  const total = segs.reduce((a, b) => a + b, 0)
  const pts: Pt[] = []
  for (let i = 0; i < n; i++) {
    let s = (i / n) * total
    let k = 0
    while (k < 7 && s > segs[k]) {
      s -= segs[k]
      k++
    }
    const f = segs[k] === 0 ? 0 : s / segs[k]
    const corner = (cx: number, cy: number, a0: number) => {
      const a = a0 + f * Math.PI / 2
      pts.push([cx + Math.cos(a) * r, cy + Math.sin(a) * r])
    }
    switch (k) {
      case 0:
        pts.push([x + r + sw * f, y])
        break
      case 1:
        corner(x + w - r, y + r, -Math.PI / 2)
        break
      case 2:
        pts.push([x + w, y + r + sh * f])
        break
      case 3:
        corner(x + w - r, y + h - r, 0)
        break
      case 4:
        pts.push([x + w - r - sw * f, y + h])
        break
      case 5:
        corner(x + r, y + h - r, Math.PI / 2)
        break
      case 6:
        pts.push([x, y + h - r - sh * f])
        break
      default:
        corner(x + r, y + r, Math.PI)
        break
    }
  }
  return pts
}

export function samplePath(d: string, n = SAMPLES): Pt[] {
  const key = `${n}|${d}`
  const hit = sampleCache.get(key)
  if (hit)
    return hit
  const props = new SvgPathProperties(d)
  const len = props.getTotalLength()
  const pts: Pt[] = []
  for (let i = 0; i < n; i++) {
    const p = props.getPointAtLength((i / n) * len)
    pts.push([p.x, p.y])
  }
  const out = clockwise(pts)
  sampleCache.set(key, out)
  return out
}

/** Affine map: scale about the origin, rotate (radians), then translate. */
export function transform(pts: Pt[], s: number, tx: number, ty: number, rot = 0): Pt[] {
  const c = Math.cos(rot)
  const si = Math.sin(rot)
  return pts.map(([x, y]) => [(x * c - y * si) * s + tx, (x * si + y * c) * s + ty])
}

/** Clockwise (in SVG's y-down space) so every shape winds the same way. */
function clockwise(pts: Pt[]) {
  return signedArea(pts) < 0 ? pts.slice().reverse() : pts
}

function signedArea(pts: Pt[]) {
  let a = 0
  for (let i = 0; i < pts.length; i++) {
    const [x0, y0] = pts[i]
    const [x1, y1] = pts[(i + 1) % pts.length]
    a += x0 * y1 - x1 * y0
  }
  return a / 2
}

const alignCache = new WeakMap<Pt[], WeakMap<Pt[], number>>()

export function morph(a: Pt[], b: Pt[], t: number, alignWith?: [Pt[], Pt[]]) {
  return toPath(morphPts(a, b, t, alignWith))
}

/**
 * Point-wise morph. `alignWith` lets callers pass stable template arrays for the
 * index alignment (cached), while `a`/`b` carry the placed, per-frame points.
 */
export function morphPts(a: Pt[], b: Pt[], t: number, alignWith?: [Pt[], Pt[]]) {
  const k = alignment(alignWith?.[0] ?? a, alignWith?.[1] ?? b)
  const n = a.length
  const out: Pt[] = Array.from({ length: n })
  for (let i = 0; i < n; i++) {
    const p = a[i]
    const q = b[(i + k) % n]
    out[i] = [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
  }
  return out
}

export function toPath(pts: Pt[]) {
  let d = `M${pts[0][0].toFixed(2)} ${pts[0][1].toFixed(2)}`
  for (let i = 1; i < pts.length; i++)
    d += `L${pts[i][0].toFixed(2)} ${pts[i][1].toFixed(2)}`
  return `${d}Z`
}

/**
 * Index offset k so that b[(i + k) % n] best matches a[i], compared on
 * normalized shapes so absolute position and size do not bias the match.
 */
function alignment(a: Pt[], b: Pt[]) {
  const byA = alignCache.get(a)
  const hit = byA?.get(b)
  if (hit !== undefined)
    return hit
  const norm = (p: Pt[]) => {
    const bb = bbox(p)
    const s = 1 / Math.max(bb.w, bb.h, 1e-6)
    return p.map(([x, y]) => [(x - bb.cx) * s, (y - bb.cy) * s] as Pt)
  }
  const na = norm(a)
  const nb = norm(b)
  const n = a.length
  let best = 0
  let bestErr = Infinity
  for (let k = 0; k < n; k++) {
    let err = 0
    for (let i = 0; i < n; i += 2) {
      const p = na[i]
      const q = nb[(i + k) % n]
      err += (p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2
      if (err > bestErr)
        break
    }
    if (err < bestErr) {
      bestErr = err
      best = k
    }
  }
  if (byA)
    byA.set(b, best)
  else
    alignCache.set(a, new WeakMap([[b, best]]))
  return best
}
