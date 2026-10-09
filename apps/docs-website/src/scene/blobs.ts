import type { Blob, WindowNode } from './types'

import { hash } from '../lib/ease'
import { GHOSTS } from '../theme'

/** Smooth pseudo-random wander in roughly [-1, 1]: three detuned sines per seed. */
function wander(t: number, seed: number) {
  const a = 0.19 + 0.13 * hash(seed)
  const b = 0.37 + 0.19 * hash(seed + 11)
  const c = 0.71 + 0.23 * hash(seed + 23)
  return (Math.sin(t * a + seed * 1.7) + 0.6 * Math.sin(t * b + seed * 3.1) + 0.3 * Math.sin(t * c + seed * 5.3)) / 1.9
}

const seedOf = (id: string) => [...id].reduce((s, ch) => s + ch.charCodeAt(0) * 7, 0) % 997

const NEIGHBOR: Record<string, keyof typeof GHOSTS> = { amber: 'pink', cyan: 'violet', lime: 'cyan', pink: 'amber', violet: 'pink' }

/**
 * Soft light that lives with each window: a glow in the window's accent color
 * and a smaller one in a neighboring hue, each drifting on its own inside the
 * window's footprint. The renderer projects them onto the back-most desk plane.
 *
 * NOTICE: there is deliberately no screen-wide base field; light only exists
 * where windows are, so empty frames (opening, final mark) stay clean.
 */
export function glowsFor(windows: WindowNode[], t: number): Blob[] {
  const out: Blob[] = []
  for (const w of windows) {
    const s = seedOf(w.id)
    const { h, w: ww, x, y } = w.rect
    const cx = x + ww / 2
    const cy = y + h / 2
    const alpha = w.opacity * (w.slab ? 0.4 : 1) * (w.glow ?? 1)
    if (alpha <= 0.01)
      continue
    // NOTICE: capped so the largest window (the browser, 680 px) does not flood
    // the empty desk around it with its accent; on the light desk a 510 px cyan
    // glow tinted the whole sky above the browser.
    const r = Math.min(Math.max(ww, h), 440) * 0.75
    const layer = { depth: w.depth, z: w.z }
    out.push({ alpha, color: GHOSTS[w.accent].fill, r, x: cx + wander(t, s) * ww * 0.42, y: cy + wander(t, s + 5) * h * 0.42, ...layer })
    out.push({ alpha: alpha * 0.7, color: GHOSTS[NEIGHBOR[w.accent]].fill, r: r * 0.6, x: cx + wander(t, s + 9) * ww * 0.5, y: cy + wander(t, s + 13) * h * 0.5, ...layer })
  }
  return out
}
