import type { WireValue } from '../runtime/protocol'
import type { CallRecord } from '../store'

import { useEffect, useRef, useState } from 'react'

import { actions, usePlayground } from '../store'
import { cursorSeq } from '../timeline'
import { RefChip } from './ValueView'

/** Every binding call of the current run. Click a row to move the time cursor to it. */
export function CallList() {
  const calls = usePlayground(state => state.calls)
  const hoveredCallId = usePlayground(state => state.hoveredCallId)
  const selectedCallId = usePlayground(state => state.selectedCallId)
  const run = usePlayground(state => state.run)
  const cursor = usePlayground(cursorSeq)
  const following = usePlayground(state => state.cursor === null)
  const longest = Math.max(1, ...calls.map(call => (call.endedAt ?? call.startedAt) - call.startedAt))
  const scrollRef = useRef<HTMLDivElement>(null)
  // Stick to the newest call while following, unless the user scrolled up.
  const stickRef = useRef(true)

  useEffect(() => {
    const element = scrollRef.current
    if (!element)
      return
    if (following) {
      if (stickRef.current)
        element.scrollTop = element.scrollHeight
      return
    }
    // When time travelling, keep the call at the cursor in view.
    const active = [...calls].reverse().find(call => call.seq <= cursor)
    element.querySelector(`[data-call-id="${active?.id}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [calls, cursor, following])

  if (calls.length === 0) {
    return (
      <Empty>
        {run.status === 'idle' ? 'Run a script to record its AUV calls here.' : 'No AUV calls in this run.'}
      </Empty>
    )
  }

  return (
    <div
      className="h-full overflow-auto"
      onMouseLeave={() => usePlayground.setState({ hoveredCallId: null })}
      onScroll={(event) => {
        const element = event.currentTarget
        stickRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24
      }}
      ref={scrollRef}
    >
      <table className="text-[12.5px] w-full border-collapse">
        <tbody>
          {calls.map((call, index) => (
            <CallRow
              call={call}
              future={!following && call.seq > cursor}
              hovered={call.id === hoveredCallId}
              index={index + 1}
              key={call.id}
              selected={call.id === selectedCallId}
              share={((call.endedAt ?? call.startedAt) - call.startedAt) / longest}
            />
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function Empty({ children }: { children: React.ReactNode }) {
  return <div className="text-fg-subtle p-6 text-center flex h-full items-center justify-center">{children}</div>
}

/** Handle chips shown before a row collapses the rest into `+N`. */
const COLLAPSED_REFS = 3

function CallRow({ call, future, hovered, index, selected, share }: { call: CallRecord, future: boolean, hovered: boolean, index: number, selected: boolean, share: number }) {
  const duration = call.endedAt ? call.endedAt - call.startedAt : undefined
  const [expanded, setExpanded] = useState(false)
  const refs = expanded ? call.refs : call.refs.slice(0, COLLAPSED_REFS)
  return (
    <tr
      className={`border-b border-line/60 cursor-pointer [&>td]:align-top ${selected ? 'bg-accent/14' : hovered ? 'bg-accent/10' : 'hover:bg-surface-2'}  ${future ? 'opacity-40' : ''}`}
      data-call-id={call.id}
      onClick={(event) => {
        // Chips and the expand toggle handle their own clicks.
        if ((event.target as HTMLElement).closest('button'))
          return
        actions.setCursor(call.endSeq ?? call.seq)
        actions.selectCall(call.id)
      }}
      onMouseEnter={() => usePlayground.setState({ hoveredCallId: call.id })}
    >
      <td className="text-fg-subtle leading-5 font-mono py-1.5 pl-3 text-right w-8">{index}</td>
      <td className="leading-5 px-2 py-1.5 w-20 whitespace-nowrap">
        {call.line !== null && <span className="text-[11px] text-fg-muted font-mono px-1.5 py-0.5 rounded bg-surface-2">{`L${call.line}`}</span>}
        {call.hit > 1 && <span className="text-[11px] text-fg-subtle font-mono ml-1">{`#${call.hit}`}</span>}
      </td>
      <td className="leading-5 font-mono py-1.5 pr-3 whitespace-nowrap">
        <span className={`mr-1.5 rounded-full size-1.5 inline-block ${call.effect === 'input' ? 'bg-rose-400' : 'bg-violet-400'}`} title={call.effect} />
        <span className={call.status === 'error' ? 'text-bad' : 'text-fg'}>{call.method}</span>
        <span className="text-fg-subtle">{`(${call.args.map(summarize).join(', ')})`}</span>
      </td>
      <td className="py-1.5 pr-3 w-full">
        <div className="flex flex-wrap gap-1 items-center">
          {refs.map(ref => <RefChip key={ref} refId={ref} />)}
          {call.refs.length > COLLAPSED_REFS && (
            <button
              className="text-[11.5px] text-fg-subtle font-mono px-1.5 rounded h-5 cursor-pointer hover:(text-fg bg-surface-2)"
              onClick={() => setExpanded(!expanded)}
              title={expanded ? 'Show fewer' : `Show all ${call.refs.length} handles`}
              type="button"
            >
              {expanded ? 'less' : `+${call.refs.length - COLLAPSED_REFS}`}
            </button>
          )}
          {call.error && <span className="text-bad truncate">{call.error}</span>}
        </div>
      </td>
      <td className="py-1.5 pr-3 w-36">
        <div className="flex gap-2 h-5 items-center">
          <div className="rounded-full bg-surface-2 flex-1 h-1.5 overflow-hidden">
            <div
              className={`rounded-full h-full ${call.status === 'pending' ? 'animate-pulse bg-warn' : call.status === 'error' ? 'bg-bad' : 'bg-accent'}`}
              style={{ width: `${Math.max(4, share * 100)}%` }}
            />
          </div>
          <span className="text-fg-muted font-mono text-right w-14">{duration === undefined ? '…' : `${Math.round(duration)}ms`}</span>
        </div>
      </td>
    </tr>
  )
}

function summarize(value: WireValue): string {
  if (value === undefined)
    return ''
  if (typeof value === 'string')
    return JSON.stringify(value.length > 24 ? `${value.slice(0, 24)}…` : value)
  if (typeof value !== 'object' || value === null)
    return String(value)
  const record = value as Record<string, unknown>
  if (typeof record.$predicate === 'number')
    return 'ƒ'
  if (typeof record.$ref === 'string')
    return record.$ref
  if (record.kind === 'area' && typeof record.width === 'number' && typeof record.height === 'number')
    return typeof record.label === 'string' ? `▭${record.label}` : `▭${Math.round(record.width)}×${Math.round(record.height)}`
  if (typeof record.x === 'number' && typeof record.y === 'number')
    return `{x:${Math.round(record.x)}, y:${Math.round(record.y)}}`
  // Nested values (e.g. `{ within: area }`) use the same summaries as arguments.
  const text = Array.isArray(value)
    ? `[${value.map(summarize).join(', ')}]`
    : `{${Object.entries(record).map(([key, item]) => `${key}: ${summarize(item)}`).join(', ')}}`
  return text.length > 40 ? `${text.slice(0, 40)}…` : text
}
