import type { Rect } from '../../script-api/api'
import type { Resource } from '../../store'

import { useEffect, useRef } from 'react'

import { CopyButton } from '../../components/CopyButton'
import { confidenceAlpha, confidenceLabel } from '../../ocr'
import { usePlayground } from '../../store'
import { ClickLoupe } from './ClickLoupe'

interface OverlayBox {
  /** Stroke opacity, e.g. from OCR confidence. */
  alpha?: number
  color: string
  /** Dashed outline, e.g. the area a search was limited to. */
  dashed?: boolean
  label?: string
  rect: Rect
}

/** Draws a captured frame scaled to fit, with screen-space boxes on top. */
export function FrameView({ bitmap, bounds, boxes = [], maxHeight = 220, maxWidth = 360 }: {
  bitmap?: ImageBitmap
  bounds: Rect
  boxes?: OverlayBox[]
  maxHeight?: number
  maxWidth?: number
}) {
  const ref = useRef<HTMLCanvasElement>(null)
  const k = Math.min(maxWidth / bounds.width, maxHeight / bounds.height, 1)
  const width = Math.max(1, Math.round(bounds.width * k))
  const height = Math.max(1, Math.round(bounds.height * k))

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
    if (bitmap)
      ctx.drawImage(bitmap, 0, 0, width, height)
    for (const box of boxes) {
      const x = (box.rect.x - bounds.x) * k
      const y = (box.rect.y - bounds.y) * k
      ctx.globalAlpha = box.alpha ?? 1
      ctx.strokeStyle = box.color
      ctx.lineWidth = 1.5
      ctx.setLineDash(box.dashed ? [4, 3] : [])
      ctx.strokeRect(x, y, box.rect.width * k, box.rect.height * k)
      ctx.setLineDash([])
      ctx.globalAlpha = 1
    }
  }, [bitmap, bounds, boxes, height, k, width])

  return (
    <canvas
      className="border border-line rounded bg-surface-2 block"
      ref={ref}
      style={{ height, width }}
    />
  )
}

const OVERLAY = {
  area: '#34d399',
  text: '#facc15',
  window: '#38bdf8',
}

/** Visual preview for a resource handle (frame, OCR result, window, input, display). */
export function ResourcePreview({ compact = false, resource }: { compact?: boolean, resource: Resource }) {
  const resources = usePlayground(state => state.resources)
  const size = compact ? { maxHeight: 150, maxWidth: 260 } : { maxHeight: 260, maxWidth: 420 }

  switch (resource.kind) {
    case 'display': {
      const { handle } = resource
      return <Meta items={[handle.name ?? `display ${handle.id}`, rectText(handle.frame), `@${handle.scale}x`, handle.primary ? 'primary' : undefined]} />
    }
    case 'frame': {
      const { handle } = resource
      return (
        <div className="flex flex-col gap-1.5">
          <FrameView bitmap={resource.bitmap} bounds={handle.bounds} {...size} />
          <Meta items={[`${handle.width}×${handle.height}px`, `@${handle.scale}x`, handle.source]} />
        </div>
      )
    }
    case 'input': {
      const { handle } = resource
      // Pointer actions get before/after magnified views around the click.
      if (handle.point)
        return <ClickLoupe compact={compact} resource={resource} />
      return <Meta items={[handle.action, handle.path]} />
    }
    case 'text': {
      const { handle } = resource
      const frame = handle.frame ? resources[handle.frame.$ref] : undefined
      if (handle.matches.length === 0) {
        // An empty search keeps its frame as evidence of what OCR looked at, collapsed.
        return (
          <div className="flex flex-col gap-1.5">
            <div className="text-[12px] text-fg-subtle flex gap-1.5 items-center">
              <span className="i-ph-magnifying-glass-minus shrink-0" />
              <span className="truncate">{handle.query ? `No text matched “${handle.query}”` : 'No text recognized'}</span>
            </div>
            {frame?.kind === 'frame' && (
              <details className="text-[11px] text-fg-subtle">
                <summary className="cursor-pointer select-none hover:text-fg-muted">{`searched in ${frame.handle.$ref}`}</summary>
                <div className="mt-1.5 opacity-70"><FrameView bitmap={frame.bitmap} bounds={frame.handle.bounds} {...size} /></div>
              </details>
            )}
          </div>
        )
      }
      const boxes: OverlayBox[] = handle.matches.map(match => ({ alpha: confidenceAlpha(match.confidence), color: OVERLAY.text, rect: match.bounds }))
      if (handle.within)
        boxes.push({ color: OVERLAY.area, dashed: true, rect: handle.within })
      return (
        <div className="flex flex-col gap-1.5">
          {frame?.kind === 'frame' && <FrameView bitmap={frame.bitmap} bounds={frame.handle.bounds} boxes={boxes} {...size} />}
          <Meta items={[`${handle.matches.length} match${handle.matches.length === 1 ? '' : 'es'}`, handle.query ? `"${handle.query}"` : 'full OCR']} />
          <ul className="text-[12px] m-0 p-0 list-none max-h-40 overflow-auto">
            {handle.matches.slice(0, compact ? 5 : 50).map((match, index) => (
              // Confidence is shown as text opacity; the exact value is in the tooltip.
              <li
                className="text-fg font-mono py-0.5 cursor-default truncate"
                key={index}
                style={{ opacity: confidenceAlpha(match.confidence) }}
                title={confidenceLabel(match.confidence)}
              >
                {match.text}
              </li>
            ))}
          </ul>
        </div>
      )
    }
    case 'window': {
      const { handle } = resource
      const capture = Object.values(resources).findLast(candidate => candidate.kind === 'frame' && candidate.handle.source === handle.$ref)
      return (
        <div className="flex flex-col gap-1.5">
          {capture?.kind === 'frame' && <FrameView bitmap={capture.bitmap} bounds={capture.handle.bounds} {...size} />}
          <Fields
            items={[
              ['title', handle.title],
              ['app', handle.app],
              ['bundle', handle.bundleId],
              ['window', handle.id],
              ['pid', handle.pid === undefined ? undefined : String(handle.pid)],
              ['frame', rectText(handle.frame)],
            ]}
          />
        </div>
      )
    }
  }
}

/** Label/value rows; every present value can be copied. */
function Fields({ items }: { items: [label: string, value: string | undefined][] }) {
  return (
    <div className="text-[12px] flex flex-col">
      {items.filter(([, value]) => value).map(([label, value]) => (
        <div className="group flex gap-2 min-h-6 items-center" key={label}>
          <span className="text-[11px] text-fg-subtle shrink-0 w-12">{label}</span>
          <span className="text-fg font-mono flex-1 min-w-0 select-text [overflow-wrap:anywhere]">{value}</span>
          <CopyButton className="opacity-40 group-hover:opacity-100" label={`Copy ${label}`} text={value!} />
        </div>
      ))}
    </div>
  )
}

function Meta({ items }: { items: (string | undefined)[] }) {
  return (
    <div className="text-[11.5px] text-fg-subtle font-mono flex flex-wrap gap-x-2 gap-y-0.5">
      {items.filter(Boolean).map((item, index) => <span key={index}>{item}</span>)}
    </div>
  )
}

function rectText(rect: Rect): string {
  return `${Math.round(rect.width)}×${Math.round(rect.height)} @ (${Math.round(rect.x)}, ${Math.round(rect.y)})`
}
