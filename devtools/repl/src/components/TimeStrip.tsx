import type { CallRecord, PlaygroundState } from '../store'

import { useEffect, useRef, useState } from 'react'

import { actions, nowMs, usePlayground } from '../store'
import { callAt, cursorSeq, lineAt } from '../timeline'
import { IconButton } from './ui'

const CALL_COLORS = { error: '#f87171', input: '#fb7185', pending: '#fbbf24', read: '#a78bfa' }

interface Axis {
  active: (t: number) => number
  t0: number
  t1: number
}

/**
 * Thin horizontal timeline of the run: step ticks colored by line, call spans
 * colored by effect, and pause markers. Click or drag to move the time cursor.
 */
export function TimeStrip() {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [hoverLabel, setHoverLabel] = useState<null | { text: string, x: number }>(null)
  const following = usePlayground(state => state.cursor === null)
  const cursor = usePlayground(cursorSeq)
  const hasEvents = usePlayground(state => state.seqAt.length > 0)
  const cursorLine = usePlayground(state => lineAt(state, cursorSeq(state)))
  const cursorCall = usePlayground(state => callAt(state, cursorSeq(state)))
  const elapsed = usePlayground((state) => {
    const t = state.seqAt[cursorSeq(state)]
    const start = state.seqAt[0]
    return t !== undefined && start !== undefined ? activeClock(state)(t) - start : 0
  })

  useEffect(() => {
    const canvas = canvasRef.current!
    const ctx = canvas.getContext('2d')!
    let raf = 0
    const draw = () => {
      const dpr = window.devicePixelRatio || 1
      const { clientHeight: h, clientWidth: w } = canvas
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
        canvas.width = w * dpr
        canvas.height = h * dpr
      }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, w, h)
      const state = usePlayground.getState()
      const axis = axisOf(state)
      if (axis) {
        const style = getComputedStyle(canvas)
        const x = (t: number) => ((axis.active(t) - axis.t0) / (axis.t1 - axis.t0)) * (w - 2) + 1
        ctx.fillStyle = style.getPropertyValue('--c-surface-2')
        ctx.fillRect(0, 4, w, 8)
        ctx.fillRect(0, 15, w, 12)
        for (const step of state.steps) {
          ctx.fillStyle = `hsl(${(step.line * 47) % 360} 70% 62%)`
          ctx.fillRect(Math.round(x(step.at)), 4, 1, 8)
        }
        for (const call of state.calls) {
          const end = call.endedAt ?? nowMs()
          const left = x(call.startedAt)
          ctx.globalAlpha = state.hoveredCallId === null || state.hoveredCallId === call.id ? 1 : 0.45
          ctx.fillStyle = call.status === 'error' ? CALL_COLORS.error : call.status === 'pending' ? CALL_COLORS.pending : CALL_COLORS[call.effect]
          ctx.fillRect(left, 15, Math.max(2, x(end) - left), 12)
        }
        ctx.globalAlpha = 1
        // Pauses collapse to a dashed marker.
        ctx.strokeStyle = style.getPropertyValue('--c-warn')
        ctx.setLineDash([2, 2])
        for (const pause of state.pauses) {
          const px = Math.round(x(pause.start)) + 0.5
          ctx.beginPath()
          ctx.moveTo(px, 2)
          ctx.lineTo(px, h - 2)
          ctx.stroke()
        }
        ctx.setLineDash([])
        const at = state.seqAt[cursorSeq(state)]
        if (at !== undefined && state.cursor !== null) {
          const cx = x(at)
          ctx.fillStyle = style.getPropertyValue('--o-hover')
          ctx.fillRect(cx - 1, 0, 2, h)
          ctx.beginPath()
          ctx.moveTo(cx - 5, 0)
          ctx.lineTo(cx + 5, 0)
          ctx.lineTo(cx, 6)
          ctx.fill()
        }
      }
      raf = requestAnimationFrame(draw)
    }
    raf = requestAnimationFrame(draw)
    return () => cancelAnimationFrame(raf)
  }, [])

  const onPointer = (event: React.PointerEvent) => {
    const state = usePlayground.getState()
    const axis = axisOf(state)
    if (!axis)
      return
    const rect = canvasRef.current!.getBoundingClientRect()
    const ratio = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width))
    const t = axis.t0 + ratio * (axis.t1 - axis.t0)
    const call = callUnder(state, axis, t, (axis.t1 - axis.t0) / 400)
    usePlayground.setState({ hoveredCallId: call?.id ?? null })
    const seq = call ? call.endSeq ?? call.seq : seqNear(state, axis, t)
    const line = lineAt(state, seq)
    setHoverLabel({
      text: call
        ? `${call.method} · L${call.line ?? '?'} #${call.hit} · ${Math.round((call.endedAt ?? call.startedAt) - call.startedAt)}ms`
        : `L${line ?? '?'} · +${((t - axis.t0) / 1000).toFixed(2)}s`,
      x: event.clientX - rect.left,
    })
    if (event.buttons === 1)
      actions.setCursor(seq)
  }

  const jumpCall = (direction: -1 | 1) => {
    const state = usePlayground.getState()
    const current = cursorSeq(state)
    const next = direction === 1
      ? state.calls.find(call => call.seq > current)
      : state.calls.findLast(call => (call.endSeq ?? call.seq) < current)
    if (next)
      actions.setCursor(next.endSeq ?? next.seq)
  }

  return (
    <div className="px-2 border-t border-line bg-surface-1 flex shrink-0 gap-1 h-10 items-center">
      <IconButton disabled={!hasEvents} hint="Previous call" icon="i-lucide-skip-back" onClick={() => jumpCall(-1)} />
      <IconButton disabled={!hasEvents} hint="Next call" icon="i-lucide-skip-forward" onClick={() => jumpCall(1)} />
      <IconButton active={following} disabled={!hasEvents} hint={following ? 'Following the latest event' : 'Follow the latest event'} icon="i-lucide-radio-tower" onClick={() => actions.setCursor(null)} />
      <div className="mx-2 flex-1 h-full relative">
        <canvas
          className="h-7 w-full block cursor-crosshair inset-x-0 top-1.5 absolute"
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId)
            onPointer(event)
          }}
          onPointerLeave={() => {
            setHoverLabel(null)
            usePlayground.setState({ hoveredCallId: null })
          }}
          onPointerMove={onPointer}
          ref={canvasRef}
        />
        {!hasEvents && <span className="text-[11.5px] text-fg-subtle flex items-center inset-0 absolute">Run a script to fill the timeline</span>}
        {hoverLabel && (
          <span
            className="text-[11px] text-fg font-mono px-1.5 py-0.5 border border-line rounded bg-surface-2 pointer-events-none whitespace-nowrap shadow absolute z-30 -translate-x-1/2 -top-6"
            style={{ left: hoverLabel.x }}
          >
            {hoverLabel.text}
          </span>
        )}
      </div>
      <span className="text-[11.5px] text-fg-muted font-mono text-right w-56 truncate">
        {!hasEvents ? '' : following ? `latest · seq ${cursor}` : `L${cursorLine ?? '?'}${cursorCall ? ` · ${cursorCall.method} #${cursorCall.hit}` : ''} · +${(elapsed / 1000).toFixed(2)}s`}
      </span>
    </div>
  )
}

/**
 * Maps wall-clock time to "active" time with debugger pauses removed, so a
 * long pause does not squeeze the rest of the run into a corner.
 */
function activeClock(state: PlaygroundState): (t: number) => number {
  return (t) => {
    let paused = 0
    for (const pause of state.pauses) {
      if (t > pause.start)
        paused += Math.min(t, pause.end ?? t) - pause.start
    }
    return t - paused
  }
}

function axisOf(state: PlaygroundState): Axis | null {
  if (state.seqAt.length === 0)
    return null
  const active = activeClock(state)
  const t0 = active(state.seqAt[0]!)
  const running = state.run.status === 'running'
  const ends = state.calls.map(call => call.endedAt ?? (running ? nowMs() : call.startedAt))
  const last = Math.max(state.seqAt.at(-1)!, ...ends, running ? nowMs() : 0)
  return { active, t0, t1: Math.max(active(last), t0 + 1) }
}

function callUnder(state: PlaygroundState, axis: Axis, t: number, slack: number): CallRecord | undefined {
  return state.calls.find(call => t >= axis.active(call.startedAt) - slack && t <= axis.active(call.endedAt ?? call.startedAt) + slack)
}

/** Event-log position whose (active) time is closest to `t`. */
function seqNear(state: PlaygroundState, axis: Axis, t: number): number {
  let best = 0
  let bestDistance = Infinity
  state.seqAt.forEach((at, seq) => {
    if (at === undefined)
      return
    const distance = Math.abs(axis.active(at) - t)
    if (distance < bestDistance) {
      best = seq
      bestDistance = distance
    }
  })
  return best
}
