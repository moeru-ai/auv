// Minimal color mixing for '#rrggbb' and 'rgba(r,g,b,a)' strings.

type RGBA = [number, number, number, number]

const cache = new Map<string, RGBA>()

export function alpha(c: string, a: number) {
  const [r, g, b, a0] = parse(c)
  return `rgba(${r},${g},${b},${(a0 * a).toFixed(3)})`
}

export function mix(a: string, b: string, t: number) {
  if (t <= 0)
    return a
  if (t >= 1)
    return b
  const x = parse(a)
  const y = parse(b)
  const m = (i: number) => x[i] + (y[i] - x[i]) * t
  return `rgba(${Math.round(m(0))},${Math.round(m(1))},${Math.round(m(2))},${m(3).toFixed(3)})`
}

export function mixOklab(a: string, b: string, t: number) {
  if (t <= 0)
    return a
  if (t >= 1)
    return b
  const x = parse(a)
  const y = parse(b)
  const p = oklab(x)
  const q = oklab(y)
  const [r, g, bl] = fromOklab([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t])
  return `rgba(${r},${g},${bl},${(x[3] + (y[3] - x[3]) * t).toFixed(3)})`
}

export function parse(c: string): RGBA {
  const hit = cache.get(c)
  if (hit)
    return hit
  let out: RGBA
  if (c.startsWith('#')) {
    const h = c.length === 4 ? c.slice(1).split('').map(x => x + x).join('') : c.slice(1)
    out = [Number.parseInt(h.slice(0, 2), 16), Number.parseInt(h.slice(2, 4), 16), Number.parseInt(h.slice(4, 6), 16), 1]
  }
  else {
    const n = c.slice(c.indexOf('(') + 1, c.indexOf(')')).split(',').map(Number)
    out = [n[0], n[1], n[2], n[3] ?? 1]
  }
  cache.set(c, out)
  return out
}
function fromOklab([L, A, B]: [number, number, number]) {
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return [
    toSrgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
    toSrgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
    toSrgb(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
  ]
}
function oklab([r, g, b]: RGBA): [number, number, number] {
  const lr = toLin(r)
  const lg = toLin(g)
  const lb = toLin(b)
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}
// OKLab mixing: perceptually even, so saturated colors fade to grays without
// passing through the muddy, darkened midpoints a plain sRGB lerp produces.
function toLin(c: number) {
  const v = c / 255
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
}

function toSrgb(v: number) {
  const c = v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055
  return Math.round(Math.min(1, Math.max(0, c)) * 255)
}
