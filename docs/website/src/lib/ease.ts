// Deterministic timing helpers. Every animated value in the film is a pure
// function of `t` (seconds), so the same frame renders identically in the live
// player and in the video exporter.

import { clamp } from 'es-toolkit'

export type Ease = (t: number) => number

export const lerp = (a: number, b: number, t: number) => a + (b - a) * t

export const linear: Ease = t => t
export const easeInCubic: Ease = t => t * t * t
export const easeOutCubic: Ease = t => 1 - (1 - t) ** 3
export const easeOutQuint: Ease = t => 1 - (1 - t) ** 5
export const easeInOutSine: Ease = t => (1 - Math.cos(Math.PI * t)) / 2
export const easeInOutCubic: Ease = t => (t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2)
/**
 * CSS-style cubic-bezier(x1, y1, x2, y2) easing. With y1 = 0 and y2 = 1 it
 * starts and ends at zero velocity, but x1 / x2 move where the speed peaks:
 * small x2 gives a quick push and a long, decisive settle.
 */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): Ease {
  const bx = (u: number) => 3 * x1 * u * (1 - u) ** 2 + 3 * x2 * u * u * (1 - u) + u ** 3
  const by = (u: number) => 3 * y1 * u * (1 - u) ** 2 + 3 * y2 * u * u * (1 - u) + u ** 3
  const dx = (u: number) => 3 * x1 * (1 - u) ** 2 + 6 * (x2 - x1) * u * (1 - u) + 3 * (1 - x2) * u * u
  return (t) => {
    if (t <= 0)
      return 0
    if (t >= 1)
      return 1
    let u = t
    for (let i = 0; i < 8; i++) {
      const d = dx(u)
      if (Math.abs(d) < 1e-6)
        break
      u = clamp(u - (bx(u) - t) / d, 0, 1)
    }
    // Bisection fallback for flat spots where Newton stalls.
    let lo = 0
    let hi = 1
    for (let i = 0; i < 20 && Math.abs(bx(u) - t) > 1e-5; i++) {
      if (bx(u) < t)
        lo = u
      else hi = u
      u = (lo + hi) / 2
    }
    return by(u)
  }
}

export function easeOutBack(t: number, s = 1.6) {
  const c3 = s + 1
  return 1 + c3 * (t - 1) ** 3 + s * (t - 1) ** 2
}
// Damped spring settle: overshoots once, then rests at 1.
export const easeOutSpring: Ease = (t) => {
  if (t >= 1)
    return 1
  return 1 - Math.exp(-6.5 * t) * Math.cos(9 * t)
}

export interface Key {
  /** Sideways bow of the segment ending at this key, as a fraction of its length. */
  arc?: number
  ease?: Ease
  t: number
  x: number
  y: number
}

/** 0 -> 1 -> 0 bump centered on `tc`, used for click presses. */
export function bump(t: number, tc: number, half = 0.09) {
  const d = Math.abs(t - tc)
  return d >= half ? 0 : Math.cos((d / half) * Math.PI / 2)
}

/** Deterministic pseudo-random in [0, 1) from an integer seed. */
export function hash(n: number) {
  const s = Math.sin(n * 127.1 + 311.7) * 43758.5453
  return s - Math.floor(s)
}

/** Eased progress of `t` across the window [t0, t1]. */
export function prog(t: number, t0: number, t1: number, ease: Ease = linear) {
  return ease(clamp((t - t0) / (t1 - t0), 0, 1))
}

/**
 * Piecewise 2D track. Segments bow slightly sideways so cursor motion reads as
 * a hand movement instead of a straight tween.
 */
export function track(keys: Key[], t: number) {
  if (t <= keys[0].t)
    return { x: keys[0].x, y: keys[0].y }
  for (let i = 1; i < keys.length; i++) {
    const b = keys[i]
    if (t > b.t)
      continue
    const a = keys[i - 1]
    const u = (b.ease ?? easeInOutCubic)(clamp((t - a.t) / (b.t - a.t), 0, 1))
    const dx = b.x - a.x
    const dy = b.y - a.y
    const bow = Math.sin(u * Math.PI) * (b.arc ?? 0.1)
    return { x: a.x + dx * u - dy * bow, y: a.y + dy * u + dx * bow }
  }
  const last = keys[keys.length - 1]
  return { x: last.x, y: last.y }
}

/** Piecewise scalar track: [[t, value], ...]. */
export function vtrack(keys: [number, number][], t: number, ease: Ease = easeInOutCubic) {
  if (t <= keys[0][0])
    return keys[0][1]
  for (let i = 1; i < keys.length; i++) {
    const [t1, v1] = keys[i]
    if (t > t1)
      continue
    const [t0, v0] = keys[i - 1]
    return lerp(v0, v1, ease(clamp((t - t0) / (t1 - t0), 0, 1)))
  }
  return keys[keys.length - 1][1]
}
