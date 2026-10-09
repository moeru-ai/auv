// Draws the small SVG-in-JSX subset used by our window content components
// straight onto a Canvas 2D context, synchronously.
//
// Why: window content becomes a WebGL texture. Rasterizing it through an
// <img src="data:image/svg+xml"> is async and decodes on every change; walking
// the JSX tree ourselves is sync (deterministic for export) and much cheaper.
//
// Supported: svg, g, defs, clipPath, linearGradient/stop, rect, circle, line,
// path, text/tspan; attributes transform (translate/scale/rotate), opacity,
// fill (color | url(#gradient) | none), stroke, strokeWidth, strokeDasharray,
// strokeLinecap/Join, clipPath="url(#id)", textAnchor, font*.
// NOTICE: anything else is ignored on purpose; extend here when content needs it.

import type { ReactElement, ReactNode } from 'react'

import { Fragment } from 'react'

type Ctx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D
type Defs = Map<string, El>
type El = ReactElement<Record<string, any>>

/** Presentation attributes that SVG inherits down the tree. */
interface Inherit { fill?: string }

export function drawSvg(ctx: Ctx, node: ReactNode, defs: Defs = new Map(), inh: Inherit = {}) {
  const n = resolve(node)
  if (n == null || typeof n !== 'object')
    return
  if (Array.isArray(n)) {
    for (const c of n) drawSvg(ctx, c, defs, inh)
    return
  }
  const el = n as El
  const p = el.props
  if (el.type === Fragment) {
    for (const c of children(p.children)) drawSvg(ctx, c, defs, inh)
    return
  }
  switch (el.type) {
    case 'clipPath':
    case 'linearGradient':
      defs.set(p.id, el)
      return
    case 'defs':
      for (const c of children(p.children)) drawSvg(ctx, c, defs, inh)
      return
    case 'desc':
    case 'title':
      return
  }

  ctx.save()
  applyTransform(ctx, p.transform)
  if (p.opacity != null)
    ctx.globalAlpha *= Number(p.opacity)
  const clipId = urlId(p.clipPath)
  if (clipId) {
    const clip = defs.get(clipId)
    if (clip) {
      const region = new Path2D()
      for (const c of children(clip.props.children)) {
        const sp = shapePath(resolve(c) as El)
        if (sp)
          region.addPath(sp)
      }
      ctx.clip(region)
    }
  }

  switch (el.type) {
    case 'g':
    case 'svg': {
      const next = { fill: p.fill ?? inh.fill }
      for (const c of children(p.children)) drawSvg(ctx, c, defs, next)
      break
    }
    case 'text':
      drawText(ctx, el, { fill: inh.fill })
      break
    default: {
      const path = shapePath(el)
      if (!path)
        break
      const fill = p.fill ?? inh.fill ?? (el.type === 'line' ? 'none' : '#000')
      const gid = urlId(fill)
      if (gid) {
        const def = defs.get(gid)
        if (def && el.type === 'rect') {
          ctx.fillStyle = gradient(ctx, def, { h: Number(p.height), w: Number(p.width), x: Number(p.x ?? 0), y: Number(p.y ?? 0) })
          ctx.fill(path)
        }
      }
      else if (fill !== 'none') {
        ctx.fillStyle = fill
        ctx.fill(path)
      }
      if (p.stroke && p.stroke !== 'none') {
        ctx.strokeStyle = p.stroke
        ctx.lineWidth = Number(p.strokeWidth ?? 1)
        ctx.lineCap = p.strokeLinecap ?? 'butt'
        ctx.lineJoin = p.strokeLinejoin ?? 'miter'
        ctx.setLineDash(p.strokeDasharray ? String(p.strokeDasharray).split(/[\s,]+/).map(Number) : [])
        ctx.stroke(path)
      }
    }
  }
  ctx.restore()
}

function applyTransform(ctx: Ctx, t: string | undefined) {
  if (!t)
    return
  for (const m of t.matchAll(/(\w+)\(([^)]*)\)/g)) {
    const a = m[2].split(/[\s,]+/).filter(Boolean).map(Number)
    switch (m[1]) {
      case 'rotate':
        ctx.rotate(((a[0] ?? 0) * Math.PI) / 180)
        break
      case 'scale':
        ctx.scale(a[0], a[1] ?? a[0])
        break
      case 'translate':
        ctx.translate(a[0] ?? 0, a[1] ?? 0)
        break
    }
  }
}

function children(node: ReactNode): ReactNode[] {
  if (node == null || typeof node === 'boolean')
    return []
  return Array.isArray(node) ? node.flatMap(children) : [node]
}

function drawText(ctx: Ctx, el: El, inherited: Record<string, any>) {
  const p = { ...inherited, ...el.props }
  ctx.font = `${p.fontWeight ?? 400} ${p.fontSize ?? 16}px ${p.fontFamily ?? 'sans-serif'}`
  const runs = children(p.children).map((c) => {
    if (typeof c === 'string' || typeof c === 'number')
      return { fill: p.fill ?? '#000', text: String(c) }
    const r = resolve(c) as El
    return { fill: r.props.fill ?? p.fill ?? '#000', text: textOf(r.props.children) }
  })
  const total = runs.reduce((w, r) => w + ctx.measureText(r.text).width, 0)
  let x = Number(p.x ?? 0)
  if (p.textAnchor === 'middle')
    x -= total / 2
  else if (p.textAnchor === 'end')
    x -= total
  const y = Number(p.y ?? 0)
  ctx.textAlign = 'left'
  ctx.textBaseline = 'alphabetic'
  for (const r of runs) {
    if (r.fill && r.fill !== 'none') {
      ctx.fillStyle = r.fill
      ctx.fillText(r.text, x, y)
    }
    x += ctx.measureText(r.text).width
  }
}

function gradient(ctx: Ctx, def: El, box: { h: number, w: number, x: number, y: number }) {
  const p = def.props
  const g = ctx.createLinearGradient(
    box.x + Number(p.x1 ?? 0) * box.w,
    box.y + Number(p.y1 ?? 0) * box.h,
    box.x + Number(p.x2 ?? 1) * box.w,
    box.y + Number(p.y2 ?? 0) * box.h,
  )
  for (const s of children(p.children) as El[]) {
    const sp = s.props
    const color = sp.stopColor ?? '#000'
    g.addColorStop(Number(sp.offset ?? 0), sp.stopOpacity == null ? color : withAlpha(ctx, color, Number(sp.stopOpacity)))
  }
  return g
}

/** Resolve function and memo components into host elements. */
function resolve(node: ReactNode): ReactNode {
  let n = node
  for (;;) {
    if (n == null || typeof n !== 'object' || Array.isArray(n))
      return n
    const el = n as El
    const type: any = el.type
    if (typeof type === 'function') {
      n = type(el.props)
      continue
    }
    if (type && typeof type === 'object' && 'type' in type && typeof type.type === 'function') {
      n = type.type(el.props)
      continue
    }
    return n
  }
}

function shapePath(el: El): null | Path2D {
  const p = el.props
  const path = new Path2D()
  switch (el.type) {
    case 'circle':
      if (Number(p.r) <= 0)
        return null
      path.arc(Number(p.cx ?? 0), Number(p.cy ?? 0), Number(p.r), 0, Math.PI * 2)
      return path
    case 'line':
      path.moveTo(Number(p.x1 ?? 0), Number(p.y1 ?? 0))
      path.lineTo(Number(p.x2 ?? 0), Number(p.y2 ?? 0))
      return path
    case 'path':
      return new Path2D(p.d)
    case 'rect': {
      const w = Number(p.width ?? 0)
      const h = Number(p.height ?? 0)
      if (w <= 0 || h <= 0)
        return null
      const r = Math.min(Number(p.rx ?? 0), w / 2, h / 2)
      path.roundRect(Number(p.x ?? 0), Number(p.y ?? 0), w, h, r)
      return path
    }
  }
  return null
}

function textOf(node: ReactNode): string {
  return children(node).map(c => (typeof c === 'string' || typeof c === 'number' ? String(c) : textOf((c as El).props?.children))).join('')
}

function urlId(v: unknown) {
  const m = typeof v === 'string' ? /url\(#([^)]+)\)/.exec(v) : null
  return m?.[1]
}

/** Canvas has no stop-opacity; bake it into the color via the context's parser. */
function withAlpha(ctx: Ctx, color: string, a: number) {
  ctx.save()
  ctx.fillStyle = color
  const c = String(ctx.fillStyle)
  ctx.restore()
  if (c.startsWith('#')) {
    const n = Number.parseInt(c.slice(1), 16)
    return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`
  }
  return c.replace(/rgba?\(([^)]+)\)/, (_, inner: string) => {
    const parts = inner.split(',').map(s => s.trim())
    return `rgba(${parts[0]},${parts[1]},${parts[2]},${Number(parts[3] ?? 1) * a})`
  })
}
