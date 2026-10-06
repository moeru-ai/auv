import { useEffect, useRef, useState } from 'react'

import { session } from '../runtime/session'
import { usePlayground } from '../store'
import { CopyButton, IconButton } from './ui'

const POSITION_KEY = 'auv-playground:debugbar'

interface Offset {
  /** Distance from the panel's right edge. */
  right: number
  /** Distance from the panel's top edge. */
  top: number
}

/**
 * Floating run/debug controls over the editor (VS Code style). Drag it by the
 * grip; the position is kept per browser.
 */
export function DebugBar() {
  const status = usePlayground(state => state.run.status)
  const mode = usePlayground(state => state.run.mode)
  const currentLine = usePlayground(state => state.run.currentLine)
  const errorMessage = usePlayground(state => state.run.errorMessage)
  const errorLine = usePlayground(state => state.run.errorLine)
  const endedAt = usePlayground(state => state.run.endedAt)
  const [dismissedAt, setDismissedAt] = useState<number>()
  const hasRecording = usePlayground(state => state.hasRecording)
  const [offset, setOffset] = useState(loadOffset)
  const barRef = useRef<HTMLDivElement>(null)
  const busy = status === 'running' || status === 'paused' || status === 'compiling'
  const paused = status === 'paused'

  useEffect(() => {
    try {
      localStorage.setItem(POSITION_KEY, JSON.stringify(offset))
    }
    catch {}
  }, [offset])

  const startDrag = (event: React.PointerEvent) => {
    const bar = barRef.current
    const panel = bar?.offsetParent as HTMLElement | null
    if (!bar || !panel)
      return
    event.preventDefault()
    const start = { offset, x: event.clientX, y: event.clientY }
    const move = (moveEvent: PointerEvent) => {
      const maxRight = panel.clientWidth - bar.offsetWidth - 4
      const maxTop = panel.clientHeight - bar.offsetHeight - 4
      setOffset({
        right: Math.min(maxRight, Math.max(4, start.offset.right - (moveEvent.clientX - start.x))),
        top: Math.min(maxTop, Math.max(4, start.offset.top + (moveEvent.clientY - start.y))),
      })
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const tone = { compiling: 'text-fg-muted', done: 'text-good', error: 'text-bad', idle: 'text-fg-subtle', paused: 'text-warn', running: 'text-accent', stopped: 'text-fg-muted' }[status]

  return (
    <div
      className="p-1 border border-line rounded-lg bg-surface-1/95 flex gap-0.5 shadow-xl items-center absolute z-20 backdrop-blur"
      ref={barRef}
      style={{ right: offset.right, top: offset.top }}
    >
      <span
        className="i-lucide-grip-vertical text-[14px] text-fg-subtle mx-0.5 cursor-grab active:cursor-grabbing"
        onPointerDown={startDrag}
        title="Drag to move"
      />
      {!busy && (
        <>
          <IconButton hint="Run all or the selection" icon="i-lucide-play" keys="⌘↩" label="Run" onClick={() => runEditor('run')} tone="primary" />
          <IconButton hint="Run and pause at the first statement" icon="i-lucide-footprints" keys="⌘⇧↩" onClick={() => runEditor('step')} />
          <IconButton
            disabled={!hasRecording}
            hint={hasRecording ? 'Replay offline: re-run against the last live run’s recorded device calls' : 'Run live once to record device calls for offline replay'}
            icon="i-lucide-history"
            onClick={() => runEditor('replay')}
          />
        </>
      )}
      {busy && (
        <>
          <IconButton disabled={!paused} hint="Continue" icon="i-lucide-play" keys="F8" onClick={() => session.resume('continue')} />
          <IconButton disabled={!paused} hint="Step to the next statement" icon="i-lucide-arrow-down-to-line" keys="F10" onClick={() => session.resume('step')} />
          <IconButton hint="Stop at the next statement" icon="i-lucide-square" keys="⇧F5" onClick={() => session.stop()} />
        </>
      )}
      <span className={`text-[11.5px] font-mono ml-1.5 mr-1 flex gap-1.5 whitespace-nowrap items-center ${tone}`} data-run-status={status}>
        {status === 'running' && <span className="i-lucide-loader-circle animate-spin" />}
        {status === 'error' && <span className="i-lucide-circle-x" />}
        {mode === 'replay' && status !== 'idle' && <span className="text-violet-300 px-1 rounded bg-violet-500/15">replay</span>}
        {paused && currentLine !== null ? `paused L${currentLine}` : status === 'error' && errorLine ? `error · L${errorLine}` : status}
      </span>
      {status === 'error' && errorMessage && dismissedAt !== endedAt && (
        <ErrorCard message={errorMessage} onDismiss={() => setDismissedAt(endedAt)} />
      )}
    </div>
  )
}

/** Full error of the last run, under the bar; wraps instead of truncating. */
function ErrorCard({ message, onDismiss }: { message: string, onDismiss: () => void }) {
  // Worker errors arrive as `<name>: <message>`.
  const [, name = 'Error', body = message] = /^(\w*Error): ([\s\S]*)$/.exec(message) ?? []

  return (
    <div className="mt-1.5 border border-bad/40 rounded-lg bg-surface-1 w-[min(420px,calc(100vw-32px))] shadow-xl right-0 top-full absolute" role="alert">
      <div className="text-[11px] text-bad font-medium pl-2.5 pr-1 pt-1 flex gap-1.5 items-center">
        <span className="i-lucide-circle-x" />
        <span className="flex-1">{name}</span>
        <CopyButton label="Copy error" text={message} />
        <button aria-label="Dismiss" className="text-fg-subtle rounded flex size-6 cursor-pointer items-center justify-center hover:(text-fg bg-surface-2)" onClick={onDismiss} title="Dismiss" type="button">
          <span className="i-lucide-x" />
        </button>
      </div>
      <div className="text-[12px] text-fg leading-relaxed font-mono px-2.5 pb-2 max-h-40 select-text [overflow-wrap:anywhere] overflow-auto">{body}</div>
    </div>
  )
}

function loadOffset(): Offset {
  try {
    const raw = localStorage.getItem(POSITION_KEY)
    if (raw)
      return JSON.parse(raw) as Offset
  }
  catch {}
  return { right: 16, top: 10 }
}

function runEditor(detail: 'replay' | 'run' | 'step'): void {
  document.dispatchEvent(new CustomEvent('playground:run', { detail }))
}
