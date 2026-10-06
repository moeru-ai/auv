import type { CameraFlight, View } from '../camera'
import type { Point, Rect } from '../script-api/api'
import type { CallRecord, FocusEvent, Mark, PlaygroundState, Resource } from '../store'

import { useEffect, useRef, useState } from 'react'

import { flight, flightDuration, viewFor } from '../camera'
import { confidenceAlpha } from '../ocr'
import { nowMs, usePlayground } from '../store'
import { callAt, callsUpTo, cursorSeq, latestFramesAt, pickAxNode } from '../timeline'

const COLORS = {
  ax: '#22d3ee',
  click: '#f43f5e',
  display: '#3a4152',
  focus: '#a78bfa',
  mark: '#34d399',
  text: '#facc15',
  window: '#38bdf8',
}

const RIPPLE_MS = 1400

interface Scene {
  cursor: number
  /** Hovered call, or the call at the time cursor. */
  focusCall?: CallRecord
  timeTravel: boolean
}

/**
 * Logical-desktop canvas: displays laid out in screen coordinates, the latest
 * frames, and overlays derived from the current run's binding calls. Draws
 * read the store directly (never re-rendering React) and run only when the
 * store, the view or the size changes, or while a click ripple animates.
 */
export function DesktopCanvas() {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const viewRef = useRef<View>({ k: 0.3, tx: 20, ty: 20 })
  const fittedRef = useRef(false)
  // Keep fitting the desktop to the canvas as panels resize, until the user
  // pans or zooms or a `focus()` moves the camera; "Fit" turns it back on.
  const autoFitRef = useRef(true)
  const invalidateRef = useRef<() => void>(undefined)
  const [cursor, setCursor] = useState<null | Point>(null)
  const displays = usePlayground(state => state.displays)

  useEffect(() => {
    fittedRef.current = false
    invalidateRef.current?.()
  }, [displays])

  useEffect(() => {
    const canvas = canvasRef.current!
    const ctx = canvas.getContext('2d')!
    // Displays and captured frames, cached between draws; only overlays are
    // redrawn when just hover, focus or selection changes.
    const base = document.createElement('canvas')
    const baseCtx = base.getContext('2d')!
    let baseKey: unknown[] = []
    let raf = 0
    // `focus()` camera: the focus the view last flew to, and the flight in progress.
    let appliedFocus: null | number = null
    let camera: null | { duration: number, path: CameraFlight, start: number } = null

    function draw() {
      raf = 0
      const dpr = window.devicePixelRatio || 1
      const { clientHeight: ch, clientWidth: cw } = canvas
      if (canvas.width !== cw * dpr || canvas.height !== ch * dpr) {
        canvas.width = cw * dpr
        canvas.height = ch * dpr
      }
      const state = usePlayground.getState()
      if (!fittedRef.current && state.displays.length > 0 && cw > 0) {
        fit(viewRef.current, state.displays.map(display => display.frame), cw, ch)
        fittedRef.current = true
        camera = null
      }
      const v = viewRef.current
      const scene = sceneOf(state)
      const focus = activeFocus(state, scene)
      if (focus && focus.id !== appliedFocus && cw > 0) {
        appliedFocus = focus.id
        autoFitRef.current = false
        const path = flight({ ...v }, viewFor(focus.rect, cw, ch, v, focus.zoom), cw, ch, focus.autoZoomOut)
        // The time until the next focus on the timeline bounds this flight.
        const next = state.focuses.find(candidate => candidate.seq > focus.seq)
        camera = { duration: flightDuration(path.duration, next ? next.at - focus.at : undefined), path, start: performance.now() }
      }
      if (camera) {
        const t = (performance.now() - camera.start) / camera.duration
        Object.assign(v, camera.path.at(t))
        if (t >= 1)
          camera = null
      }
      // NOTICE(camera-smoothing): high-quality downscaling of Retina captures
      // costs ~40 ms a frame; flights use low quality, then redraw once landed.
      const quality: ImageSmoothingQuality = camera ? 'low' : 'high'
      const key = [canvas.width, canvas.height, v.k, v.tx, v.ty, quality, state.displays, state.liveFrames, state.resources, scene.cursor, scene.timeTravel, scene.focusCall?.id]
      if (key.some((value, index) => value !== baseKey[index])) {
        baseKey = key
        base.width = canvas.width
        base.height = canvas.height
        baseCtx.setTransform(dpr * v.k, 0, 0, dpr * v.k, dpr * v.tx, dpr * v.ty)
        baseCtx.imageSmoothingEnabled = true
        baseCtx.imageSmoothingQuality = quality
        paintBase(baseCtx, state, scene, v.k)
      }
      ctx.setTransform(1, 0, 0, 1, 0, 0)
      ctx.clearRect(0, 0, canvas.width, canvas.height)
      ctx.drawImage(base, 0, 0)
      ctx.setTransform(dpr * v.k, 0, 0, dpr * v.k, dpr * v.tx, dpr * v.ty)
      // Click ripples and camera flights keep animating until done.
      if (paintOverlays(ctx, state, scene, v.k) || camera)
        invalidate()
    }
    function invalidate() {
      if (raf === 0)
        raf = requestAnimationFrame(draw)
    }
    invalidateRef.current = invalidate
    invalidate()
    const unsubscribe = usePlayground.subscribe(invalidate)
    const resize = new ResizeObserver(() => {
      if (autoFitRef.current)
        fittedRef.current = false
      invalidate()
    })
    resize.observe(canvas)

    const onWheel = (event: WheelEvent) => {
      event.preventDefault()
      autoFitRef.current = false
      camera = null
      const v = viewRef.current
      const k2 = Math.min(6, Math.max(0.03, v.k * Math.exp(-event.deltaY * 0.0015)))
      v.tx = event.offsetX - (event.offsetX - v.tx) * (k2 / v.k)
      v.ty = event.offsetY - (event.offsetY - v.ty) * (k2 / v.k)
      v.k = k2
      invalidate()
    }
    let drag: null | Point = null
    let downAt: null | Point = null
    const onDown = (event: PointerEvent) => {
      autoFitRef.current = false
      camera = null
      drag = { x: event.clientX, y: event.clientY }
      downAt = drag
      canvas.setPointerCapture(event.pointerId)
    }
    const onMove = (event: PointerEvent) => {
      const v = viewRef.current
      setCursor({ x: (event.offsetX - v.tx) / v.k, y: (event.offsetY - v.ty) / v.k })
      if (!drag)
        return
      v.tx += event.clientX - drag.x
      v.ty += event.clientY - drag.y
      drag = { x: event.clientX, y: event.clientY }
      invalidate()
    }
    const onUp = (event: PointerEvent) => {
      // A click without dragging picks the element under the pointer (AX tree).
      if (downAt && Math.hypot(event.clientX - downAt.x, event.clientY - downAt.y) < 4) {
        const v = viewRef.current
        const tree = usePlayground.getState().axTree
        const node = tree ? pickAxNode(tree, { x: (event.offsetX - v.tx) / v.k, y: (event.offsetY - v.ty) / v.k }) : undefined
        if (node)
          usePlayground.setState({ selectedAxPath: node.path })
      }
      drag = null
      downAt = null
    }
    const onLeave = () => setCursor(null)
    canvas.addEventListener('wheel', onWheel, { passive: false })
    canvas.addEventListener('pointerdown', onDown)
    canvas.addEventListener('pointermove', onMove)
    canvas.addEventListener('pointerup', onUp)
    canvas.addEventListener('pointerleave', onLeave)
    return () => {
      cancelAnimationFrame(raf)
      unsubscribe()
      resize.disconnect()
      invalidateRef.current = undefined
      canvas.removeEventListener('wheel', onWheel)
      canvas.removeEventListener('pointerdown', onDown)
      canvas.removeEventListener('pointermove', onMove)
      canvas.removeEventListener('pointerup', onUp)
      canvas.removeEventListener('pointerleave', onLeave)
    }
  }, [])

  return (
    <div className="bg-surface-0 h-full w-full relative overflow-hidden" style={{ backgroundImage: 'radial-gradient(circle, var(--c-line) 1px, transparent 1px)', backgroundSize: '22px 22px' }}>
      <canvas className="h-full w-full block cursor-grab touch-none active:cursor-grabbing" ref={canvasRef} />
      {displays.length === 0 && (
        <div className="text-fg-subtle flex pointer-events-none items-center inset-0 justify-center absolute">
          Connect a device to see its displays
        </div>
      )}
      <div className="flex gap-2 items-center bottom-3 right-3 absolute">
        {cursor && (
          <span className="text-[11px] text-fg-muted font-mono px-2 py-1 rounded bg-surface-1/90 shadow">
            {`x ${Math.round(cursor.x)}  y ${Math.round(cursor.y)}`}
          </span>
        )}
        <button
          className="btn-ghost bg-surface-1/90 shadow"
          onClick={() => {
            fittedRef.current = false
            autoFitRef.current = true
            invalidateRef.current?.()
          }}
          type="button"
        >
          <span className="i-lucide-scan" />
          Fit
        </button>
      </div>
    </div>
  )
}

/** Latest `focus()` at the time cursor (or overall while following). */
function activeFocus(state: PlaygroundState, { cursor, timeTravel }: Scene): FocusEvent | undefined {
  return timeTravel ? state.focuses.findLast(focus => focus.seq <= cursor) : state.focuses.at(-1)
}

function contains(outer: Rect, inner: Rect): boolean {
  return inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.width <= outer.x + outer.width && inner.y + inner.height <= outer.y + outer.height
}

function fit(v: View, rects: Rect[], width: number, height: number): void {
  const minX = Math.min(...rects.map(rect => rect.x))
  const minY = Math.min(...rects.map(rect => rect.y))
  const maxX = Math.max(...rects.map(rect => rect.x + rect.width))
  const maxY = Math.max(...rects.map(rect => rect.y + rect.height))
  const pad = 32
  v.k = Math.min((width - pad * 2) / (maxX - minX), (height - pad * 2) / (maxY - minY))
  v.tx = pad + ((width - pad * 2) - (maxX - minX) * v.k) / 2 - minX * v.k
  v.ty = pad + ((height - pad * 2) - (maxY - minY) * v.k) / 2 - minY * v.k
}

function label(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, k: number, color: string): void {
  ctx.font = `500 ${12 / k}px Inter, "Helvetica Neue", Arial, sans-serif`
  ctx.fillStyle = color
  ctx.fillText(text, x, y)
}

/** Pixels: displays, live frames and captured frames as of the cursor. */
function paintBase(ctx: CanvasRenderingContext2D, state: PlaygroundState, { cursor, focusCall, timeTravel }: Scene, k: number): void {
  const px = (n: number) => n / k

  // 1. Displays. When following the latest event, show live pixels; when time
  //    travelling, only what had been captured by the cursor.
  for (const display of state.displays) {
    const f = display.frame
    ctx.fillStyle = '#0f1218'
    ctx.fillRect(f.x, f.y, f.width, f.height)
    const live = state.liveFrames[display.id]
    if (live && !timeTravel)
      ctx.drawImage(live.bitmap, live.bounds.x, live.bounds.y, live.bounds.width, live.bounds.height)
    ctx.strokeStyle = COLORS.display
    ctx.lineWidth = px(1)
    ctx.strokeRect(f.x, f.y, f.width, f.height)
    label(ctx, `${display.name ?? display.id}  ${f.width}×${f.height} @${display.scale}x`, f.x, f.y - px(6), k, '#8b93a7')
  }

  // 2. The newest frame of every source as of the cursor, oldest first.
  for (const frame of latestFramesAt(state, cursor)) {
    if (!frame.bitmap)
      continue
    const liveIsNewer = !timeTravel && Object.values(state.liveFrames).some(candidate => candidate.capturedAt > frame.capturedAt && contains(candidate.bounds, frame.handle.bounds))
    if (liveIsNewer)
      continue
    const b = frame.handle.bounds
    ctx.drawImage(frame.bitmap, b.x, b.y, b.width, b.height)
  }

  // 3. The focused call's own frame (hovered, or at the cursor) is what that
  //    step actually saw.
  if (focusCall) {
    for (const ref of focusCall.refs) {
      const resource = state.resources[ref]
      if (resource?.kind === 'frame' && resource.bitmap) {
        const b = resource.handle.bounds
        ctx.drawImage(resource.bitmap, b.x, b.y, b.width, b.height)
      }
    }
  }
}

function paintFocus(ctx: CanvasRenderingContext2D, resource: Resource | undefined, k: number): void {
  if (!resource)
    return
  const px = (n: number) => n / k
  const rects: Rect[] = []
  if (resource.kind === 'window')
    rects.push(resource.handle.frame)
  else if (resource.kind === 'display')
    rects.push(resource.handle.frame)
  else if (resource.kind === 'frame')
    rects.push(resource.handle.bounds)
  else if (resource.kind === 'text')
    rects.push(...resource.handle.matches.map(match => match.bounds))
  else if (resource.kind === 'input' && resource.handle.point)
    rects.push({ height: px(24), width: px(24), x: resource.handle.point.x - px(12), y: resource.handle.point.y - px(12) })
  ctx.save()
  ctx.shadowBlur = 12
  ctx.shadowColor = COLORS.focus
  ctx.strokeStyle = COLORS.focus
  ctx.lineWidth = px(2.5)
  for (const rect of rects)
    ctx.strokeRect(rect.x, rect.y, rect.width, rect.height)
  ctx.restore()
}

function paintMark(ctx: CanvasRenderingContext2D, mark: Mark, k: number): void {
  const px = (n: number) => n / k
  ctx.save()
  ctx.strokeStyle = COLORS.mark
  ctx.lineWidth = px(1.5)
  if ('rect' in mark.shape) {
    const r = mark.shape.rect
    ctx.fillStyle = 'rgba(52, 211, 153, 0.08)'
    ctx.fillRect(r.x, r.y, r.width, r.height)
    ctx.strokeRect(r.x, r.y, r.width, r.height)
    // Labels go inside boxes tall enough to hold them, so stacked areas
    // (e.g. `input` and `input.below(...)`) do not overlap their labels.
    if (mark.label) {
      const inside = r.height * k >= 40
      tag(ctx, mark.label, r.x + (inside ? px(4) : 0), inside ? r.y + px(4) : r.y - px(20), k, COLORS.mark)
    }
  }
  else {
    const { x, y } = mark.shape.point
    ctx.beginPath()
    ctx.arc(x, y, px(7), 0, Math.PI * 2)
    ctx.moveTo(x - px(12), y)
    ctx.lineTo(x + px(12), y)
    ctx.moveTo(x, y - px(12))
    ctx.lineTo(x, y + px(12))
    ctx.stroke()
    if (mark.label)
      tag(ctx, mark.label, x + px(10), y + px(6), k, COLORS.mark)
  }
  ctx.restore()
}

/** Vector overlays drawn every time; returns whether a click ripple is still animating. */
function paintOverlays(ctx: CanvasRenderingContext2D, state: PlaygroundState, { cursor, focusCall, timeTravel }: Scene, k: number): boolean {
  let animating = false
  // 4. Overlays for calls up to the cursor; the focused call is emphasized.
  //    Enumerations (`*.list`) can return dozens of windows; they only draw
  //    when their call is focused, so the desktop stays readable.
  for (const call of timeTravel ? callsUpTo(state, cursor) : state.calls) {
    const focused = focusCall?.id === call.id
    if (call.method.endsWith('.list') && !focused)
      continue
    const emphasis = focusCall ? (focused ? 1 : 0.18) : 0.75
    for (const ref of call.refs)
      animating = paintResource(ctx, state.resources[ref], k, emphasis, call) || animating
  }

  // 5. Editor-only `draw()` marks up to the cursor.
  for (const mark of state.marks) {
    if (timeTravel && mark.seq > cursor)
      continue
    paintMark(ctx, mark, k)
  }

  // 6. Hovered or selected handle, and AX-tree or area hover.
  const focusRef = state.hoveredRef ?? state.selectedRef
  if (focusRef)
    paintFocus(ctx, state.resources[focusRef], k)
  if (state.hoveredRect)
    paintRect(ctx, state.hoveredRect, k, COLORS.ax)
  return animating
}

function paintRect(ctx: CanvasRenderingContext2D, rect: Rect, k: number, color: string): void {
  ctx.save()
  ctx.fillStyle = `${color}22`
  ctx.fillRect(rect.x, rect.y, rect.width, rect.height)
  ctx.strokeStyle = color
  ctx.lineWidth = 2 / k
  ctx.strokeRect(rect.x, rect.y, rect.width, rect.height)
  ctx.restore()
}

/** Returns whether the resource is still animating (a fading click ripple). */
function paintResource(ctx: CanvasRenderingContext2D, resource: Resource | undefined, k: number, alpha: number, call: CallRecord): boolean {
  if (!resource)
    return false
  let animating = false
  const px = (n: number) => n / k
  ctx.globalAlpha = alpha
  switch (resource.kind) {
    case 'input': {
      const point = resource.handle.point
      if (!point)
        break
      const age = call.endedAt ? nowMs() - call.endedAt : 0
      if (age < RIPPLE_MS) {
        animating = true
        const p = age / RIPPLE_MS
        ctx.globalAlpha = alpha * (1 - p)
        ctx.strokeStyle = COLORS.click
        ctx.lineWidth = px(2)
        ctx.beginPath()
        ctx.arc(point.x, point.y, px(8 + 36 * p), 0, Math.PI * 2)
        ctx.stroke()
        ctx.globalAlpha = alpha
      }
      ctx.fillStyle = COLORS.click
      ctx.beginPath()
      ctx.arc(point.x, point.y, px(5), 0, Math.PI * 2)
      ctx.fill()
      ctx.strokeStyle = '#fff'
      ctx.lineWidth = px(1.5)
      ctx.stroke()
      break
    }
    case 'text': {
      // The area a search was limited to (`within`), dashed like an area.
      const within = resource.handle.within
      if (within) {
        ctx.strokeStyle = COLORS.mark
        ctx.lineWidth = px(1.25)
        ctx.setLineDash([px(5), px(4)])
        ctx.strokeRect(within.x, within.y, within.width, within.height)
        ctx.setLineDash([])
      }
      for (const match of resource.handle.matches) {
        const b = match.bounds
        // Low-confidence matches fade, matching the OCR list in previews.
        ctx.globalAlpha = alpha * confidenceAlpha(match.confidence)
        ctx.fillStyle = 'rgba(250, 204, 21, 0.12)'
        ctx.fillRect(b.x, b.y, b.width, b.height)
        ctx.strokeStyle = COLORS.text
        ctx.lineWidth = px(1.25)
        ctx.strokeRect(b.x, b.y, b.width, b.height)
      }
      break
    }
    case 'window': {
      const f = resource.handle.frame
      ctx.strokeStyle = COLORS.window
      ctx.lineWidth = px(1.5)
      ctx.setLineDash([px(6), px(4)])
      ctx.strokeRect(f.x, f.y, f.width, f.height)
      ctx.setLineDash([])
      if (alpha >= 0.5)
        tag(ctx, resource.handle.title || resource.handle.app || resource.handle.$ref, f.x + px(4), f.y + px(4), k, COLORS.window)
      break
    }
  }
  ctx.globalAlpha = 1
  return animating
}

function sceneOf(state: PlaygroundState): Scene {
  const timeTravel = state.cursor !== null
  const cursor = cursorSeq(state)
  const hoveredCall = state.calls.find(call => call.id === state.hoveredCallId)
  return { cursor, focusCall: hoveredCall ?? (timeTravel ? callAt(state, cursor) : undefined), timeTravel }
}

/** A small filled label anchored at its top-left corner, readable on any frame. */
function tag(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, k: number, color: string): void {
  const size = 11 / k
  const padding = 4 / k
  ctx.font = `600 ${size}px Inter, "Helvetica Neue", Arial, sans-serif`
  const clipped = text.length > 48 ? `${text.slice(0, 47)}…` : text
  const width = ctx.measureText(clipped).width + padding * 2
  ctx.fillStyle = 'rgba(11, 13, 18, 0.82)'
  ctx.beginPath()
  ctx.roundRect(x, y, width, size + padding * 2, 3 / k)
  ctx.fill()
  ctx.fillStyle = color
  ctx.textBaseline = 'top'
  ctx.fillText(clipped, x + padding, y + padding)
  ctx.textBaseline = 'alphabetic'
}
