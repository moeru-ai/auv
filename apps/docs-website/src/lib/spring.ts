// Analytic damped spring, 0 -> 1. Being closed-form keeps scripted film frames
// deterministic, unlike an integrated spring whose value depends on frame rate.

export interface SpringOpts {
  /** Natural frequency in Hz. Higher settles faster. */
  f?: number
  /** Damping ratio; below 1 overshoots once or twice. */
  z?: number
}

export function spring(t: number, t0: number, { f = 1.6, z = 0.62 }: SpringOpts = {}) {
  const x = t - t0
  if (x <= 0)
    return 0
  const w0 = 2 * Math.PI * f
  if (z >= 1)
    return 1 - Math.exp(-w0 * x) * (1 + w0 * x)
  const wd = w0 * Math.sqrt(1 - z * z)
  return 1 - Math.exp(-z * w0 * x) * (Math.cos(wd * x) + (z * w0 / wd) * Math.sin(wd * x))
}
