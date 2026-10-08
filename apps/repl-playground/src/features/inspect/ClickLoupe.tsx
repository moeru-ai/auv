import type { Point } from '../../script-api/api'
import type { Resource } from '../../store'

import { contains } from '@auv-js/sdk'
import { useEffect, useRef } from 'react'

import { usePlayground } from '../../store'

type FrameResource = Resource & { kind: 'frame' }
type InputResource = Resource & { kind: 'input' }

/** Logical (point) size of the area around the click shown in each loupe. */
const AREA = { height: 90, width: 150 }

/**
 * Before/after magnified views around a click, from the frames of the same run
 * that cover the click point: the latest one before the click and the first
 * one after it. Shows whether the click landed where intended and what changed.
 */
export function ClickLoupe({ compact = false, resource }: { compact?: boolean, resource: InputResource }) {
  const resources = usePlayground(state => state.resources)
  const point = resource.handle.point
  if (!point)
    return null

  const frames = Object.values(resources)
    .filter((candidate): candidate is FrameResource => candidate.kind === 'frame' && candidate.run === resource.run && contains(candidate.handle.bounds, point))
    .sort((a, b) => a.seq - b.seq)
  const before = frames.findLast(frame => frame.seq < resource.seq)
  const after = frames.find(frame => frame.seq > resource.seq)

  // The OCR match under the point, from the latest recognition before the click.
  const target = Object.values(resources)
    .filter(candidate => candidate.kind === 'text' && candidate.run === resource.run && candidate.seq < resource.seq)
    .sort((a, b) => a.seq - b.seq)
    .flatMap(candidate => (candidate.kind === 'text' ? candidate.handle.matches : []))
    .findLast(match => contains(match.bounds, point))

  const width = compact ? 260 : 360
  return (
    <div className="flex flex-col gap-1.5">
      <div className="text-[11.5px] text-fg-subtle font-mono flex flex-wrap gap-x-2">
        <span className="text-kind-input">{`${resource.handle.action} (${Math.round(point.x)}, ${Math.round(point.y)})`}</span>
        {target && <span className="text-fg">{`on “${target.text}”`}</span>}
        {resource.handle.path && <span>{resource.handle.path}</span>}
      </div>
      <Loupe frame={before} label="before" point={point} width={width} />
      <Loupe frame={after} label="after" point={point} width={width} />
    </div>
  )
}

function Loupe({ frame, label, point, width }: { frame?: FrameResource, label: string, point: Point, width: number }) {
  const ref = useRef<HTMLCanvasElement>(null)
  const height = Math.round(width * AREA.height / AREA.width)

  useEffect(() => {
    const canvas = ref.current
    if (!canvas)
      return
    const dpr = window.devicePixelRatio || 1
    canvas.width = width * dpr
    canvas.height = height * dpr
    const ctx = canvas.getContext('2d')!
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, width, height)
    if (!frame?.bitmap)
      return
    const bounds = frame.handle.bounds
    // Crop a logical area centered on the click, clamped inside the frame.
    const area = {
      height: Math.min(AREA.height, bounds.height),
      width: Math.min(AREA.width, bounds.width),
      x: 0,
      y: 0,
    }
    area.x = Math.min(Math.max(point.x - area.width / 2, bounds.x), bounds.x + bounds.width - area.width)
    area.y = Math.min(Math.max(point.y - area.height / 2, bounds.y), bounds.y + bounds.height - area.height)
    const sx = frame.bitmap.width / bounds.width
    const sy = frame.bitmap.height / bounds.height
    ctx.imageSmoothingQuality = 'high'
    ctx.drawImage(frame.bitmap, (area.x - bounds.x) * sx, (area.y - bounds.y) * sy, area.width * sx, area.height * sy, 0, 0, width, height)

    // Crosshair and landing marker at the click point.
    // Axes scale separately: a clamped area may not keep the loupe's aspect ratio.
    const cx = (point.x - area.x) * (width / area.width)
    const cy = (point.y - area.y) * (height / area.height)
    ctx.strokeStyle = 'rgba(244, 63, 94, 0.55)'
    ctx.lineWidth = 1
    ctx.setLineDash([3, 3])
    ctx.beginPath()
    ctx.moveTo(cx, 0)
    ctx.lineTo(cx, height)
    ctx.moveTo(0, cy)
    ctx.lineTo(width, cy)
    ctx.stroke()
    ctx.setLineDash([])
    ctx.beginPath()
    ctx.arc(cx, cy, 9, 0, Math.PI * 2)
    ctx.strokeStyle = '#f43f5e'
    ctx.lineWidth = 2
    ctx.stroke()
    ctx.beginPath()
    ctx.arc(cx, cy, 2.5, 0, Math.PI * 2)
    ctx.fillStyle = '#f43f5e'
    ctx.fill()
  }, [frame, height, point, width])

  return (
    <div className="relative">
      <canvas className="border border-line rounded bg-surface-2 block" ref={ref} style={{ height, width }} />
      <span className="text-[10.5px] text-white font-mono px-1.5 py-0.5 rounded bg-black/60 left-1.5 top-1.5 absolute">
        {frame ? `${label} · ${frame.handle.$ref}` : `${label} · no capture covers this point`}
      </span>
    </div>
  )
}
