import type { Rect } from '../../script-api/api'

export interface CameraFlight {
  /** View at progress `t` in [0, 1] (already eased). */
  at: (t: number) => View
  /** Natural duration in ms for the path's length. */
  duration: number
}

/** Canvas view: screen pixel = world point × `k` + (`tx`, `ty`). */
export interface View {
  k: number
  tx: number
  ty: number
}

/** View as the world-space center and visible width, the space zoom paths are planned in. */
type Center = [cx: number, cy: number, width: number]

const RHO = Math.SQRT2
// NOTICE(camera-context): a focused target never fills the canvas beyond
// this much desktop (logical points), so a single OCR match keeps its
// surroundings instead of turning into blurry, magnified pixels.
const MIN_VISIBLE = { height: 240, width: 360 }

/**
 * Camera path from one view to another. With `arc`, the path is the
 * smooth zoom-and-pan of van Wijk & Nuij ("Smooth and efficient zooming and
 * panning", 2003), as in d3-interpolate's `interpolateZoom`: far jumps rise
 * (zoom out) mid-way and descend onto the target, and their duration grows
 * with the distance. Without `arc`, center moves linearly and scale changes
 * geometrically.
 */
export function flight(from: View, to: View, width: number, height: number, arc: boolean): CameraFlight {
  const p0 = toCenter(from, width, height)
  const p1 = toCenter(to, width, height)
  const path = arc ? zoomPath(p0, p1) : straightPath(p0, p1)
  return {
    at: t => fromCenter(path.at(easeInOutCubic(Math.min(1, Math.max(0, t)))), width, height),
    duration: path.duration,
  }
}

/**
 * Duration for a flight: its natural length, kept within readable bounds and
 * squeezed into `budget` (the time until the next focus on the timeline) when
 * that is known, so consecutive focuses do not lag behind the run.
 */
export function flightDuration(natural: number, budget?: number): number {
  // NOTICE(camera-duration): 280–1400 ms keeps short hops visible and long
  // jumps from stalling; a budget may shorten a flight down to 160 ms.
  const readable = Math.min(1400, Math.max(280, natural))
  if (budget === undefined || !Number.isFinite(budget))
    return readable
  return Math.max(160, Math.min(readable, budget * 0.85))
}

/**
 * View that fits `rect` with some margin into a `width`×`height` canvas. With
 * `zoom: false` the current scale is kept and only the center moves.
 */
export function viewFor(rect: Rect, width: number, height: number, current: View, zoom: boolean): View {
  const k = zoom
    ? Math.min(width / MIN_VISIBLE.width, height / MIN_VISIBLE.height, (width * 0.8) / Math.max(rect.width, 1), (height * 0.8) / Math.max(rect.height, 1))
    : current.k
  return { k, tx: width / 2 - (rect.x + rect.width / 2) * k, ty: height / 2 - (rect.y + rect.height / 2) * k }
}

function easeInOutCubic(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2
}

function fromCenter([cx, cy, w]: Center, width: number, height: number): View {
  const k = width / w
  return { k, tx: width / 2 - cx * k, ty: height / 2 - cy * k }
}

function straightPath(p0: Center, p1: Center): { at: (t: number) => Center, duration: number } {
  const distance = Math.hypot(p1[0] - p0[0], p1[1] - p0[1]) / Math.max(p0[2], p1[2])
  const scale = Math.abs(Math.log(p1[2] / p0[2]))
  return {
    at: t => [p0[0] + (p1[0] - p0[0]) * t, p0[1] + (p1[1] - p0[1]) * t, p0[2] * (p1[2] / p0[2]) ** t],
    duration: 1000 * Math.max(distance, scale) * 0.8,
  }
}

function toCenter(view: View, width: number, height: number): Center {
  return [(width / 2 - view.tx) / view.k, (height / 2 - view.ty) / view.k, width / view.k]
}

/** d3-interpolate `interpolateZoom` (ISC), with ρ = √2. */
function zoomPath(p0: Center, p1: Center): { at: (t: number) => Center, duration: number } {
  const [ux0, uy0, w0] = p0
  const [ux1, uy1, w1] = p1
  const dx = ux1 - ux0
  const dy = uy1 - uy0
  const d2 = dx * dx + dy * dy
  const rho2 = RHO * RHO
  const rho4 = rho2 * rho2
  if (d2 < 1e-12) {
    // Same center: zoom only.
    const S = Math.log(w1 / w0) / RHO
    return { at: t => [ux0 + t * dx, uy0 + t * dy, w0 * Math.exp(RHO * t * S)], duration: Math.abs(S) * 1000 }
  }
  const d1 = Math.sqrt(d2)
  const b0 = (w1 * w1 - w0 * w0 + rho4 * d2) / (2 * w0 * rho2 * d1)
  const b1 = (w1 * w1 - w0 * w0 - rho4 * d2) / (2 * w1 * rho2 * d1)
  const r0 = Math.log(Math.sqrt(b0 * b0 + 1) - b0)
  const r1 = Math.log(Math.sqrt(b1 * b1 + 1) - b1)
  const S = (r1 - r0) / RHO
  return {
    at: (t) => {
      const s = t * S
      const coshr0 = Math.cosh(r0)
      const u = w0 / (rho2 * d1) * (coshr0 * Math.tanh(RHO * s + r0) - Math.sinh(r0))
      return [ux0 + u * dx, uy0 + u * dy, w0 * coshr0 / Math.cosh(RHO * s + r0)]
    },
    duration: S * 1000,
  }
}
