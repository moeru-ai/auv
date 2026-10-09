import type { WireValue } from '../../runtime/protocol'
import type { Resource } from '../../store'

import { PreviewCard } from '@base-ui/react/preview-card'
import { useState } from 'react'

import { refHint, refOf } from '../../handles'
import { actions, usePlayground } from '../../store'
import { ResourcePreview } from './ResourcePreview'

const KIND_STYLE: Record<string, string> = {
  display: 'bg-kind-display/14 text-kind-display',
  frame: 'bg-kind-frame/14 text-kind-frame',
  input: 'bg-kind-input/14 text-kind-input',
  text: 'bg-kind-text/14 text-kind-text',
  window: 'bg-kind-window/14 text-kind-window',
}

interface AreaValue {
  from?: string
  height: number
  label?: string
  width: number
  x: number
  y: number
}

/**
 * Clickable chip for a resource handle. Hover highlights it on the canvas and,
 * after a short delay, opens a preview card with the handle's data.
 */
export function RefChip({ children, className, refId }: {
  /** Custom content, e.g. a list row; replaces the chip look together with `className`. */
  children?: React.ReactNode
  className?: string
  refId: string
}) {
  const kind = refId.split(':')[0] ?? ''
  const selected = usePlayground(state => state.selectedRef === refId)
  const resource = usePlayground(state => state.resources[refId])
  const hint = refHint(resource)
  // An empty OCR search is still a handle, but nothing to act on: show it faint.
  const empty = resource?.kind === 'text' && resource.handle.matches.length === 0
  // NOTICE(preview-inside-editor-tooltip): CodeMirror closes a hover tooltip
  // once the pointer leaves the tooltip's DOM. A chip inside one portals its
  // preview into that tooltip so moving onto the preview keeps both open.
  const [tooltip, setTooltip] = useState<HTMLElement | null>(null)
  return (
    <PreviewCard.Root>
      {/* NOTICE(preview-card-delay): a short open delay still lets the pointer
          sweep across a dense row of chips without flashing a card per chip.
          The card appears without an enter transition; only closing fades. */}
      <PreviewCard.Trigger
        closeDelay={60}
        delay={30}
        render={(
          <button
            className={className === undefined ? `text-[11.5px] font-mono px-1.5 rounded inline-flex gap-1 h-5 max-w-full cursor-pointer transition-shadow items-center ${KIND_STYLE[kind] ?? 'bg-surface-2 text-fg-muted'}  ${empty ? 'opacity-50' : ''} ${selected ? 'ring-1 ring-accent' : ''}` : `${className} ${selected ? 'bg-accent/14' : ''}`}
            onClick={() => actions.selectRef(refId)}
            onMouseEnter={() => usePlayground.setState({ hoveredRef: refId })}
            onMouseLeave={() => usePlayground.setState({ hoveredRef: null })}
            ref={node => setTooltip(node?.closest<HTMLElement>('.cm-tooltip') ?? null)}
            type="button"
          />
        )}
      >
        {children ?? (
          <>
            <span className="opacity-60">◆</span>
            {refId}
            {hint && <span className="opacity-60 max-w-40 truncate">{`· ${hint}`}</span>}
          </>
        )}
      </PreviewCard.Trigger>
      <PreviewCard.Portal container={tooltip ?? undefined}>
        {/* NOTICE(preview-above-editor-tooltip): chips also render inside
            CodeMirror hover tooltips, which use `z-index: 500`
            (`@codemirror/view` base theme, `.cm-tooltip`); the preview must sit above them. */}
        {/* `fixed` escapes the editor tooltip's `overflow: hidden` when portaled into it. */}
        <PreviewCard.Positioner className="z-[600]" positionMethod="fixed" side="top" sideOffset={6}>
          <PreviewCard.Popup className="p-3 border border-line rounded-xl bg-surface-1 w-[320px] shadow-2xl transition-[opacity,transform] duration-80 ease-out data-[ending-style]:(opacity-0 scale-98)">
            {resource ? <RefPreview refId={refId} resource={resource} /> : <span className="text-fg-subtle">This handle is no longer available.</span>}
          </PreviewCard.Popup>
        </PreviewCard.Positioner>
      </PreviewCard.Portal>
    </PreviewCard.Root>
  )
}

/** Compact, expandable rendering of values coming from the script worker. */
export function ValueView({ depth = 0, value }: { depth?: number, value: WireValue }) {
  const [open, setOpen] = useState(depth < 1)
  // SDK values (a `Window`, a `WindowClient`, a `CaptureRef`) show as a chip
  // once the playground recorded the resource; otherwise as plain data.
  const ref = refOf(value)
  const known = usePlayground(state => ref !== undefined && ref in state.resources)

  if (value === null)
    return <span className="text-fg-subtle">null</span>
  if (value === undefined)
    return <span className="text-fg-subtle">undefined</span>
  if (typeof value === 'string')
    return <span className="text-syn-string">{JSON.stringify(value)}</span>
  if (typeof value === 'number' || typeof value === 'boolean')
    return <span className="text-syn-number">{String(value)}</span>
  if (typeof value !== 'object')
    return <span>{String(value)}</span>

  const record = value as Record<string, WireValue>
  if (ref && (typeof record.$ref === 'string' || known))
    return <RefChip refId={ref} />
  if (record.kind === 'area' && typeof record.x === 'number' && typeof record.width === 'number')
    return <AreaChip area={record as unknown as AreaValue} />
  if ('$fn' in record) {
    return (
      <span className="text-fg-subtle italic">
        ƒ
        {String(record.$fn)}
      </span>
    )
  }
  if ('$error' in record)
    return <span className="text-bad">{String(record.$error)}</span>
  if ('$bytes' in record)
    return <span className="text-fg-subtle">{`<${String(record.$bytes)} bytes>`}</span>

  const entries = Array.isArray(value) ? value.map((item, index) => [String(index), item] as const) : Object.entries(record)
  const brackets = Array.isArray(value) ? ['[', ']'] : ['{', '}']
  if (entries.length === 0)
    return <span className="text-fg-subtle">{brackets.join('')}</span>

  return (
    <span className="font-mono">
      <button className="text-fg-subtle cursor-pointer hover:text-fg" onClick={() => setOpen(!open)} type="button">
        {open ? '▾' : '▸'}
        {' '}
        {brackets[0]}
        {!open && <span>{` ${entries.length} ${Array.isArray(value) ? 'items' : 'keys'} `}</span>}
        {!open && brackets[1]}
      </button>
      {open && (
        <div className="ml-3.5 pl-2 border-l border-line">
          {entries.slice(0, 200).map(([key, item]) => (
            <div className="leading-6" key={key}>
              <span className="text-fg-muted">{key}</span>
              <span className="text-fg-subtle">: </span>
              <ValueView depth={depth + 1} value={item} />
            </div>
          ))}
        </div>
      )}
      {open && <span className="text-fg-subtle">{brackets[1]}</span>}
    </span>
  )
}

/** An `area()` value; hovering outlines it on the desktop canvas. */
function AreaChip({ area }: { area: AreaValue }) {
  const rect = { height: area.height, width: area.width, x: area.x, y: area.y }
  return (
    <span
      className="text-[11.5px] text-syn-string font-mono px-1.5 rounded bg-syn-string/12 inline-flex gap-1 h-5 max-w-full cursor-default items-center"
      onMouseEnter={() => usePlayground.setState({ hoveredRect: rect })}
      onMouseLeave={() => usePlayground.setState({ hoveredRect: null })}
      title={area.from ? `area from ${area.from}` : 'area'}
    >
      <span className="opacity-60">▭</span>
      {area.label && <span>{area.label}</span>}
      <span className="opacity-60 truncate">{`${Math.round(area.width)}×${Math.round(area.height)} @ (${Math.round(area.x)}, ${Math.round(area.y)})`}</span>
    </span>
  )
}

function RefPreview({ refId, resource }: { refId: string, resource: Resource }) {
  const producer = usePlayground(state => state.calls.find(call => call.id === resource.callId))
  return (
    <div className="text-[12.5px] flex flex-col gap-2">
      <div className="flex gap-2 items-start">
        <div className="flex flex-col gap-0.5 min-w-0">
          <span className="text-fg font-mono break-all">{refId}</span>
          {producer && <span className="text-[11px] text-fg-subtle font-mono truncate">{`from ${producer.method}${producer.line === null ? '' : ` · L${producer.line} #${producer.hit}`}`}</span>}
        </div>
        <button
          className="text-fg-subtle ml-auto rounded flex shrink-0 size-6 cursor-pointer items-center justify-center hover:(text-accent bg-surface-2)"
          onClick={event => actions.addPin({ label: refId, ref: refId, seq: resource.seq, x: event.clientX + 12, y: event.clientY - 20 })}
          title="Pin as a floating card"
          type="button"
        >
          <span className="i-ph-push-pin text-[13px]" />
        </button>
      </div>
      <ResourcePreview compact resource={resource} />
    </div>
  )
}
