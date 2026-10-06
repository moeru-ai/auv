import type { BirpcReturn } from 'birpc'

import type { RecordedCall } from '../backend/replay'
import type { Backend } from '../backend/types'
import type { Rect } from '../script-api/api'
import type { BindSite, StepSite } from '../stepper/compile'
import type { BindEvent, Mark, StepEvent } from '../store'
import type { ExecWorkerApi, HostApi, LanguageWorkerApi, ResumeMode } from './protocol'

import { createBirpc } from 'birpc'

import { RecordingBackend, ReplayBackend } from '../backend/replay'
import { boundsOf } from '../handles'
import { StepTimer } from '../stepper/timing'
import { actions, usePlayground } from '../store'
import { decodeBitmap, invokeBinding } from './bindings'

/** Language service + compiler worker, shared by the editor and the runner. */
export const language: BirpcReturn<LanguageWorkerApi, object> = createBirpc<LanguageWorkerApi, object>({}, (() => {
  const worker = new Worker(new URL('../workers/language.worker.ts', import.meta.url), { type: 'module' })
  return {
    on: (fn: (data: unknown) => void) => worker.addEventListener('message', event => fn(event.data)),
    post: (data: unknown) => worker.postMessage(data),
    timeout: 60_000,
  }
})())

export interface RunOptions {
  /** Pause at the first statement. */
  mode: 'run' | 'step'
  /** Answer device calls from the previous live run's recording. */
  replay?: boolean
  source: string
  /** Editor line where `source` starts. */
  startLine: number
}

/**
 * Collects high-frequency hook events (thousands per second in tight loops)
 * and commits them to the store once per animation frame.
 */
class EventBatch {
  binds: BindEvent[] = []
  lines = new Set<number>()
  seqAt: Array<[number, number]> = []
  steps: StepEvent[] = []
  #scheduled = false

  constructor(private readonly commit: (batch: EventBatch) => void) {}

  flush(): void {
    this.#scheduled = false
    if (this.steps.length === 0 && this.binds.length === 0 && this.seqAt.length === 0)
      return
    this.commit(this)
    this.binds = []
    this.lines = new Set()
    this.seqAt = []
    this.steps = []
  }

  schedule(): void {
    if (this.#scheduled)
      return
    this.#scheduled = true
    requestAnimationFrame(() => this.flush())
  }
}

/**
 * Owns the execution worker and turns its hook events into the run's event
 * log. One cell runs at a time; top-level names persist until `reset()`.
 */
class ExecSession {
  get running(): boolean {
    return this.#running
  }

  #batch: EventBatch
  #bindHits: number[] = []
  #binds: BindSite[] = []
  #cellId = 0
  #exec!: BirpcReturn<ExecWorkerApi, HostApi>
  #lastLine: null | number = null
  #lineHits = new Map<number, number>()
  #liveTimer?: number
  #recording?: { entries: RecordedCall[], label: string }
  #runBackend: Backend | null = null
  #running = false
  #seq = 0
  #steps: StepSite[] = []
  #timer?: StepTimer

  #worker?: Worker

  constructor() {
    this.#batch = new EventBatch((batch) => {
      actions.appendEvents(batch.steps, batch.binds, batch.seqAt)
      usePlayground.setState((state) => {
        const executed = new Set(state.executedLines)
        batch.lines.forEach(line => executed.add(line))
        return {
          executedLines: [...executed],
          run: { ...state.run, currentLine: this.#lastLine },
          timings: this.#timer?.snapshot() ?? state.timings,
        }
      })
    })
    this.#spawn()
  }

  /** Reloads the accessibility tree from the active backend, when supported. */
  async refreshAxTree(): Promise<void> {
    const backend = usePlayground.getState().backend
    if (!backend?.accessibilityTree) {
      usePlayground.setState({ axTree: null })
      return
    }
    usePlayground.setState({ axTree: await backend.accessibilityTree() })
  }

  /** Terminates the worker (also escapes infinite sync loops) and clears REPL state. */
  reset(): void {
    this.#worker?.terminate()
    this.#running = false
    actions.resetSession()
    this.#spawn()
  }

  // TODO(step-over): `step` pauses at the next hook anywhere, so it steps into
  // called async functions. Step-over needs call-depth tracking in the stepper
  // (enter/exit hooks around function bodies).
  resume(mode: ResumeMode): void {
    this.#exec.resume(mode)
  }

  async run(options: RunOptions): Promise<void> {
    if (this.#running)
      return
    const live = usePlayground.getState().backend
    if (options.replay && !this.#recording)
      return
    this.#running = true
    const recorder = !options.replay && live ? new RecordingBackend(live) : undefined
    this.#runBackend = options.replay ? new ReplayBackend(this.#recording!.entries, this.#recording!.label) : recorder ?? live
    this.#seq = 0
    this.#lineHits = new Map()
    this.#lastLine = null
    actions.beginRun(options.replay ? 'replay' : 'live')
    try {
      const compiled = await language.compile({ cellId: ++this.#cellId, source: options.source, startLine: options.startLine })
      if (!compiled.ok) {
        actions.patchRun({ endedAt: Date.now(), errorLine: compiled.line, errorMessage: compiled.message, status: 'error' })
        return
      }
      this.#steps = compiled.steps
      this.#binds = compiled.binds
      this.#bindHits = Array.from<number>({ length: compiled.binds.length }).fill(0)
      this.#timer = new StepTimer(compiled.steps)
      const runId = await this.#runBackend?.beginRun()
      actions.patchRun({ runId, status: 'running' })
      const outcome = await this.#exec.run({
        binds: compiled.binds,
        breakpoints: usePlayground.getState().breakpoints,
        code: compiled.code,
        hoisted: compiled.hoisted,
        mode: options.mode,
        prelude: compiled.prelude,
        steps: compiled.steps,
      })
      this.#timer.finish(performance.timeOrigin + performance.now())
      this.#batch.flush()
      const timings = this.#timer.snapshot()
      const ended = { currentLine: null, endedAt: Date.now() }
      if (outcome.status === 'ok') {
        actions.patchRun({ ...ended, outcome, status: 'done' })
        if (outcome.value !== undefined)
          this.#log('show', [outcome.value], null, 'result')
      }
      else if (outcome.status === 'stopped') {
        actions.patchRun({ ...ended, outcome, status: 'stopped' })
      }
      else {
        actions.patchRun({ ...ended, errorLine: outcome.line, errorMessage: outcome.message, outcome, status: 'error' })
        this.#log('error', [outcome.message], outcome.line ?? null)
      }
      usePlayground.setState({ timings })
      await this.#runBackend?.endRun(outcome.status === 'ok' ? 'succeeded' : outcome.status === 'stopped' ? 'canceled' : 'failed')
      if (recorder) {
        this.#recording = { entries: recorder.entries, label: recorder.label }
        usePlayground.setState({ hasRecording: true })
      }
      void this.refreshAxTree().catch(() => {})
    }
    finally {
      this.#running = false
    }
  }

  setBreakpoints(lines: number[]): void {
    this.#exec.setBreakpoints(lines)
  }

  /** Polls display captures into the canvas while live mode is on. */
  setLive(live: boolean, intervalMs = 900): void {
    usePlayground.setState({ live })
    window.clearTimeout(this.#liveTimer)
    if (!live)
      return
    const tick = async () => {
      const { backend, displays, live: stillLive } = usePlayground.getState()
      if (!stillLive || !backend)
        return
      for (const display of displays) {
        try {
          const frame = await backend.captureDisplay(display.id)
          const bitmap = await decodeBitmap(frame)
          actions.setLiveFrame(display.id, { bitmap, bounds: frame.bounds, capturedAt: Date.now() })
        }
        catch (error) {
          console.warn(`Live capture failed for display ${display.id}`, error)
        }
      }
      if (usePlayground.getState().live)
        this.#liveTimer = window.setTimeout(() => void tick(), intervalMs)
    }
    void tick()
  }

  stop(): void {
    this.#exec.stop()
  }

  #log(level: Parameters<HostApi['onLog']>[0], values: unknown[], line: null | number, label?: string): void {
    actions.addLog({ at: Date.now(), label, level, line, seq: this.#nextSeq(), values })
  }

  #nextSeq(at = performance.timeOrigin + performance.now()): number {
    const seq = this.#seq++
    this.#batch.seqAt.push([seq, at])
    this.#batch.schedule()
    return seq
  }

  #spawn(): void {
    const worker = new Worker(new URL('../workers/exec.worker.ts', import.meta.url), { type: 'module' })
    this.#worker = worker
    const lineOf = (stepId: null | number) => stepId === null ? null : this.#steps[stepId]?.line ?? null
    const host: HostApi = {
      call: async (method, args, stepId) => {
        const line = lineOf(stepId)
        return await invokeBinding(this.#runBackend, method, args, {
          hit: line === null ? 1 : this.#lineHits.get(line) ?? 1,
          line,
          nextSeq: () => {
            const seq = this.#nextSeq()
            this.#batch.flush()
            return seq
          },
        })
      },
      onBind: (bindId, values, at) => {
        const site = this.#binds[bindId]
        if (!site)
          return
        const hit = ++this.#bindHits[bindId]!
        this.#batch.binds.push({ at, hit, line: site.line, seq: this.#nextSeq(at), values })
      },
      onDraw: (target, label, stepId) => {
        const shape = markShape(target)
        if (!shape) {
          this.#log('warn', ['draw() expects an area, a rectangle { x, y, width, height } or a point { x, y }'], lineOf(stepId))
          return
        }
        const area = target as { label?: unknown }
        actions.addMark({ label: label ?? (typeof area.label === 'string' ? area.label : undefined), line: lineOf(stepId), seq: this.#nextSeq(), shape })
      },
      onFocus: (target, options, stepId, at) => {
        const rect = boundsOf(target, usePlayground.getState().resources)
        if (!rect) {
          this.#log('warn', ['focus() expects a window, area, OCR result or match, frame, click or point'], lineOf(stepId))
          return
        }
        actions.addFocus({ at, autoZoomOut: options?.autoZoomOut ?? true, line: lineOf(stepId), rect, seq: this.#nextSeq(at), zoom: options?.zoom ?? true })
      },
      onLog: (level, values, stepId, label) => {
        this.#log(level, values, lineOf(stepId), label)
      },
      onPause: (stepId, at) => {
        this.#timer?.pause(at)
        this.#batch.flush()
        actions.pauseAt(at)
        actions.patchRun({ currentLine: this.#steps[stepId]?.line ?? null, status: 'paused' })
      },
      onResume: (at) => {
        this.#timer?.resume(at)
        actions.resumeAt(at)
        actions.patchRun({ status: 'running' })
      },
      onStep: (stepId, at) => {
        const site = this.#timer?.step(stepId, at)
        if (!site)
          return
        const hit = (this.#lineHits.get(site.line) ?? 0) + 1
        this.#lineHits.set(site.line, hit)
        this.#lastLine = site.line
        this.#batch.lines.add(site.line)
        this.#batch.steps.push({ at, hit, line: site.line, seq: this.#nextSeq(at) })
      },
      onVars: (vars) => {
        usePlayground.setState({ vars })
      },
    }
    this.#exec = createBirpc<ExecWorkerApi, HostApi>(host, {
      // Control messages to the worker are fire-and-forget; only `run` returns.
      eventNames: ['resume', 'setBreakpoints', 'stop'],
      on: fn => worker.addEventListener('message', event => fn(event.data)),
      post: data => worker.postMessage(data),
      timeout: 24 * 60 * 60_000,
    })
  }
}

export const session = new ExecSession()

/** Switches the active device backend and refreshes displays. */
export async function activateBackend(backend: Backend | null): Promise<void> {
  const previous = usePlayground.getState().backend
  session.setLive(false)
  usePlayground.setState({ axTree: null, backend, connection: backend ? 'connected' : 'disconnected', connectionError: undefined, displays: [], liveFrames: {} })
  if (previous && previous !== backend)
    await previous.dispose().catch(() => {})
  if (!backend)
    return
  const displays = await backend.listDisplays()
  usePlayground.setState({ displays })
  // One snapshot per display so the canvas is not empty before live mode.
  for (const display of displays) {
    void backend.captureDisplay(display.id)
      .then(async frame => actions.setLiveFrame(display.id, { bitmap: await decodeBitmap(frame), bounds: frame.bounds, capturedAt: Date.now() }))
      .catch(error => console.warn(`Initial capture failed for display ${display.id}`, error))
  }
  void session.refreshAxTree().catch(() => {})
}

/** Canvas shape for a `draw()` target: rectangles (and areas) by bounds, otherwise a point. */
function markShape(value: unknown): Mark['shape'] | undefined {
  const target = value as null | Partial<Rect> | undefined
  if (typeof target?.x !== 'number' || typeof target.y !== 'number')
    return undefined
  if (typeof target.width === 'number' && typeof target.height === 'number')
    return { rect: { height: target.height, width: target.width, x: target.x, y: target.y } }
  return { point: { x: target.x, y: target.y } }
}
