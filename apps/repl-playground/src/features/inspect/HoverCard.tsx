import type { HoverInfo, WireValue } from '../../runtime/protocol'
import type { CallRecord } from '../../store'
import type { Tick } from './TimeTicks'

import { useState } from 'react'

import { confidenceAlpha } from '../../ocr'
import { actions, usePlayground } from '../../store'
import { bindHistory, cursorSeq, framesOf, refOf, sampleIndexAt } from '../../timeline'
import { MethodDocs } from './MethodDocs'
import { FrameView, ResourcePreview } from './ResourcePreview'
import { TimeTicks } from './TimeTicks'
import { RefChip, ValueView } from './ValueView'

/**
 * Editor hover: TypeScript quick info, the history of a name's values, the
 * captures of a window handle over time, and the calls made on the line.
 * Every section can be pinned as a floating card fixed to one moment.
 */
export function HoverCard({ info, line, name }: { info: HoverInfo | null, line: number, name?: string }) {
  const binds = usePlayground(state => state.binds)
  const vars = usePlayground(state => state.vars)
  const allCalls = usePlayground(state => state.calls)
  const resources = usePlayground(state => state.resources)
  const cursor = usePlayground(cursorSeq)

  // Value history: every binding of `name` in this run; top-level values from
  // an earlier cell fall back to the persisted value.
  // `binds` is subscribed above so the card re-renders as new values arrive.
  const samples = name && binds.length > 0 ? bindHistory(usePlayground.getState(), name) : []
  const persisted = name && samples.length === 0 && name in vars ? [{ hit: 1, line: 0, seq: -1, value: vars[name] }] : []
  const values = samples.length > 0 ? samples : persisted
  const [valuePick, setValuePick] = useState<null | number>(null)
  const valueIndex = valuePick ?? sampleIndexAt(values, cursor)
  const value = values[valueIndex]

  const windowRef = value && refOf(value.value)?.startsWith('window:') ? refOf(value.value) : undefined
  const captures = windowRef ? framesOf(usePlayground.getState(), windowRef) : []
  const [capturePick, setCapturePick] = useState<null | number>(null)
  const captureIndex = capturePick ?? sampleIndexAt(captures, cursor)

  const calls = allCalls.filter(call => call.line === line)
  const [callPick, setCallPick] = useState<null | number>(null)
  const callIndex = callPick ?? sampleIndexAt(calls, cursor)
  const call = calls[callIndex]
  // While a call is in flight, keep showing the previous finished one so the
  // preview area does not collapse and re-expand on every step.
  const previewCall = call?.status === 'pending' ? calls.slice(0, callIndex).findLast(candidate => candidate.status !== 'pending') ?? call : call

  const valueTicks: Tick[] = values.map(sample => ({ label: `#${sample.hit}`, seq: sample.seq, title: `${name} @ L${sample.line} #${sample.hit} (seq ${sample.seq})` }))
  const captureTicks: Tick[] = captures.map((frame, index) => ({ label: `${index + 1}`, seq: frame.seq, title: `${frame.handle.$ref} (seq ${frame.seq})` }))
  // Several calls on one line in the same iteration get the method in their label.
  const sharedHit = new Set(calls.filter((record, index) => calls.findIndex(other => other.hit === record.hit) !== index).map(record => record.hit))
  const callTicks: Tick[] = calls.map(record => ({ label: sharedHit.has(record.hit) ? `${record.method.split('.').pop()} #${record.hit}` : `#${record.hit}`, seq: record.seq, title: `${record.method} #${record.hit} (seq ${record.seq})`, tone: record.status === 'error' ? 'bad' : 'default' }))

  return (
    // Fixed width so the card does not resize (and jump) as live values change.
    <div className="text-[12.5px] p-3 flex flex-col gap-2.5 w-[420px]">
      {info && (
        <div className="flex flex-col gap-1">
          <code className="text-[12px] text-fg font-mono whitespace-pre-wrap break-words">{info.type}</code>
          {info.doc && <p className="text-fg-muted m-0">{info.doc}</p>}
        </div>
      )}

      {name && value && (
        <Section
          onPin={event => actions.addPin({ hit: value.hit, label: `${name} @ L${value.line} #${value.hit}`, line: value.line, ref: refOf(value.value), seq: value.seq, value: value.value, x: event.clientX + 12, y: event.clientY - 20 })}
          title={`${name}${value.seq >= 0 ? ` · L${value.line} #${value.hit}` : ' · persisted'}`}
        >
          <TimeTicks cursor={cursor} onSelect={setValuePick} selected={valueIndex} ticks={valueTicks} />
          <ValuePreview value={value.value} />
        </Section>
      )}

      {captures.length > 0 && captures[captureIndex] && (
        <Section
          onPin={event => actions.addPin({ label: `${windowRef} capture ${captureIndex + 1}`, ref: captures[captureIndex]!.handle.$ref, seq: captures[captureIndex]!.seq, x: event.clientX + 12, y: event.clientY - 20 })}
          title={`captures of ${windowRef} · ${captures.length}`}
        >
          <TimeTicks cursor={cursor} onSelect={setCapturePick} selected={captureIndex} ticks={captureTicks} />
          <ResourcePreview compact resource={captures[captureIndex]!} />
        </Section>
      )}

      {call && (
        <Section
          onPin={(event) => {
            const ref = call.refs.findLast(candidate => resources[candidate]?.kind === 'text' || resources[candidate]?.kind === 'frame') ?? call.refs[0]
            actions.addPin({ hit: call.hit, label: `${call.method} @ L${line} #${call.hit}`, line, ref, seq: call.seq, value: call.result, x: event.clientX + 12, y: event.clientY - 20 })
          }}
          title={`line ${line} · ${calls.length} call${calls.length === 1 ? '' : 's'}`}
        >
          <TimeTicks cursor={cursor} onSelect={setCallPick} selected={callIndex} ticks={callTicks} />
          <MethodDocs call={call} />
          <CallSummary call={call} previewCall={previewCall} />
        </Section>
      )}
    </div>
  )
}

/** Renders a handle as a resource preview, otherwise as an expandable value. */
export function ValuePreview({ value }: { value: WireValue }) {
  const resources = usePlayground(state => state.resources)
  const ref = refOf(value)
  const resource = ref ? resources[ref] : undefined
  if (resource)
    return <ResourcePreview compact resource={resource} />
  return <div className="font-mono max-h-48 overflow-auto"><ValueView value={value} /></div>
}

/**
 * The call's visual evidence: its captured frame with any OCR boxes from the
 * same call, or its first previewable resource. The shape depends only on the
 * call itself, so the card does not resize while other sections update.
 */
function CallSummary({ call, previewCall = call }: { call: CallRecord, previewCall?: CallRecord }) {
  const resources = usePlayground(state => state.resources)
  const produced = previewCall.refs.map(ref => resources[ref]).filter(resource => resource !== undefined)
  const frame = produced.find(resource => resource.kind === 'frame')
  const boxes = produced.flatMap(resource => resource.kind === 'text'
    ? resource.handle.matches.map(match => ({ alpha: confidenceAlpha(match.confidence), color: '#facc15', rect: match.bounds }))
    : [])
  // An OCR search that found nothing keeps the frame's size (the card must not
  // resize between iterations) but dims it under a "no match" label.
  const texts = produced.filter(resource => resource.kind === 'text')
  const noMatch = texts.length > 0 && texts.every(resource => resource.kind === 'text' && resource.handle.matches.length === 0)
  const fallback = frame ? undefined : produced.find(resource => resource.kind === 'text' || resource.kind === 'input' || resource.kind === 'window')
  return (
    <div className="flex flex-col gap-1.5">
      {/* One fixed-height row: wrapping or extra status lines would resize the card. */}
      <div className="flex flex-nowrap gap-2 h-5 whitespace-nowrap items-center overflow-hidden">
        <span className={`rounded-full shrink-0 size-1.5 ${call.status === 'ok' ? 'bg-good' : call.status === 'error' ? 'bg-bad' : 'bg-warn'}`} />
        <span className="text-fg font-mono">{`${call.method} #${call.hit}`}</span>
        {call.endedAt && <span className="text-fg-subtle font-mono">{`${Math.round(call.endedAt - call.startedAt)}ms`}</span>}
        {previewCall !== call
          ? <span className="text-[11px] text-warn">{`running… showing #${previewCall.hit}`}</span>
          : call.refs.slice(0, 3).map(ref => <RefChip key={ref} refId={ref} />)}
      </div>
      {call.error && <span className="text-bad">{call.error}</span>}
      {frame?.kind === 'frame' && (
        <div className="self-start relative">
          <div className={noMatch ? 'opacity-30' : undefined}>
            <FrameView bounds={frame.handle.bounds} boxes={boxes} image={frame} maxHeight={150} maxWidth={260} />
          </div>
          {noMatch && (
            <span className="text-[12px] text-fg-muted flex gap-1.5 items-center inset-0 justify-center absolute">
              <span className="i-ph-magnifying-glass-minus" />
              No text matched
            </span>
          )}
        </div>
      )}
      {fallback && <ResourcePreview compact resource={fallback} />}
    </div>
  )
}

function Section({ children, onPin, title }: { children: React.ReactNode, onPin: (event: React.MouseEvent) => void, title: string }) {
  return (
    <div className="pt-2 border-t border-line flex flex-col gap-1.5 first:(pt-0 border-t-0)">
      <div className="flex gap-2 items-center justify-between">
        <div className="panel-title truncate">{title}</div>
        <button
          className="text-fg-subtle rounded flex shrink-0 size-6 cursor-pointer items-center justify-center hover:(text-accent bg-surface-2)"
          onClick={onPin}
          title="Pin this moment as a floating card"
          type="button"
        >
          <span className="i-ph-push-pin text-[13px]" />
        </button>
      </div>
      {children}
    </div>
  )
}
