// Canvas-drawn textures for the WebGL desk: window content, cursors, labels.
// All drawing is synchronous so a frame is complete as soon as render() returns.

import type { WindowNode } from '../scene/types'
import type { GhostColor, Theme } from '../theme'

import { createElement } from 'react'

import * as THREE from 'three'

import { GHOST_PATH, USER_PATH } from '../lib/icon'
import { SANS, WindowChromeImpl } from '../render/WindowContent'
import { GHOSTS } from '../theme'
import { drawSvg } from './svgcanvas'

interface ContentEntry { canvas: HTMLCanvasElement, key: string, tex: THREE.CanvasTexture }

/**
 * One texture per window id, redrawn only when what it shows changes
 * (size, identity, data, theme, or raster scale), never for movement.
 */
export class ContentTextures {
  private map = new Map<string, ContentEntry>()

  dispose() {
    for (const e of this.map.values()) e.tex.dispose()
    this.map.clear()
  }

  get(win: WindowNode, theme: Theme, scale: number): THREE.CanvasTexture {
    const { h, w } = win.rect
    const key = `${win.kind}|${win.title}|${win.accent}|${w}|${h}|${theme.name}|${scale}|${JSON.stringify(win.data)}`
    let e = this.map.get(win.id)
    if (e && e.key === key)
      return e.tex
    const cw = Math.ceil(w * scale)
    const ch = Math.ceil(h * scale)
    if (!e || e.canvas.width !== cw || e.canvas.height !== ch) {
      e?.tex.dispose()
      const canvas = makeCanvas(cw, ch)
      e = { canvas, key: '', tex: texture(canvas) }
      this.map.set(win.id, e)
    }
    const ctx = e.canvas.getContext('2d')!
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.clearRect(0, 0, cw, ch)
    ctx.scale(scale, scale)
    drawSvg(ctx, createElement(WindowChromeImpl, { clipId: `${win.id}-clip`, theme, win }))
    e.key = key
    e.tex.needsUpdate = true
    return e.tex
  }

  /** Drop textures for windows no longer on screen. */
  retain(ids: Set<string>) {
    for (const [id, e] of this.map) {
      if (!ids.has(id)) {
        e.tex.dispose()
        this.map.delete(id)
      }
    }
  }
}

function makeCanvas(w: number, h: number) {
  const c = document.createElement('canvas')
  c.width = Math.max(1, Math.ceil(w))
  c.height = Math.max(1, Math.ceil(h))
  return c
}

function texture(canvas: HTMLCanvasElement) {
  const t = new THREE.CanvasTexture(canvas)
  t.colorSpace = THREE.NoColorSpace
  t.generateMipmaps = true
  t.minFilter = THREE.LinearMipmapLinearFilter
  t.magFilter = THREE.LinearFilter
  t.anisotropy = 4
  return t
}

/** Texture-space box of a cursor drawing, in path units. */
export const CURSOR_BOX = { h: 30, w: 26, x: -3, y: -3 }
const CURSOR_PX = 10

const cursorCache = new Map<string, THREE.CanvasTexture>()

interface LabelEntry { tex: THREE.CanvasTexture, w: number }

/** Ghost or user pointer with its drop shadow, hotspot at path origin. */
export function cursorTexture(kind: 'ghost' | 'user', color: GhostColor) {
  const key = `${kind}|${color}`
  const hit = cursorCache.get(key)
  if (hit)
    return hit
  const c = makeCanvas(CURSOR_BOX.w * CURSOR_PX, CURSOR_BOX.h * CURSOR_PX)
  const ctx = c.getContext('2d')!
  ctx.scale(CURSOR_PX, CURSOR_PX)
  ctx.translate(-CURSOR_BOX.x, -CURSOR_BOX.y)
  ctx.shadowColor = 'rgba(0,0,0,0.28)'
  ctx.shadowBlur = 2.4 * CURSOR_PX
  ctx.shadowOffsetY = 1 * CURSOR_PX
  const path = new Path2D(kind === 'ghost' ? GHOST_PATH : USER_PATH)
  ctx.lineJoin = 'round'
  if (kind === 'ghost') {
    ctx.fillStyle = GHOSTS[color].fill
    ctx.fill(path)
    ctx.shadowColor = 'transparent'
    ctx.strokeStyle = GHOSTS[color].edge
    ctx.lineWidth = 1.6
    ctx.stroke(path)
  }
  else {
    ctx.fillStyle = '#111'
    ctx.fill(path)
    ctx.shadowColor = 'transparent'
    ctx.strokeStyle = '#fff'
    ctx.lineWidth = 1.3
    ctx.stroke(path)
  }
  const t = texture(c)
  cursorCache.set(key, t)
  return t
}
const labelCache = new Map<string, LabelEntry>()
const LABEL_H = 32
const LABEL_SCALE = 3

/** Pill label for a ghost cursor. Cached per visible text, bounded LRU. */
export function labelTexture(color: GhostColor, text: string): LabelEntry {
  const key = `${color}|${text}`
  const hit = labelCache.get(key)
  if (hit) {
    labelCache.delete(key)
    labelCache.set(key, hit)
    return hit
  }
  const font = `600 15px ${SANS}`
  const probe = makeCanvas(1, 1).getContext('2d')!
  probe.font = font
  const w = Math.ceil(30 + probe.measureText(text).width)
  const c = makeCanvas(w * LABEL_SCALE, LABEL_H * LABEL_SCALE)
  const ctx = c.getContext('2d')!
  ctx.scale(LABEL_SCALE, LABEL_SCALE)
  const g = GHOSTS[color]
  ctx.fillStyle = g.fill
  ctx.beginPath()
  ctx.roundRect(0.5, 0.5, w - 1, LABEL_H - 1, (LABEL_H - 1) / 2)
  ctx.fill()
  ctx.strokeStyle = 'rgba(255,255,255,0.55)'
  ctx.stroke()
  ctx.font = font
  ctx.fillStyle = g.label
  ctx.textBaseline = 'alphabetic'
  ctx.fillText(text, 15, 21)
  const entry = { tex: texture(c), w }
  labelCache.set(key, entry)
  // NOTICE: typewriter labels create one texture per prefix; keep the cache bounded.
  if (labelCache.size > 240) {
    const [oldKey, old] = labelCache.entries().next().value as [string, LabelEntry]
    old.tex.dispose()
    labelCache.delete(oldKey)
  }
  return entry
}
export { LABEL_H }
