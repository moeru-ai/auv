import type { BirpcReturn } from 'birpc'

import type { Recording } from '../backend/replay'
import type { RunBackend, RunOutcomeKind } from '../backend/types'
import type { Rect } from '../script-api/api'
import type { BindSite, StepSite } from '../stepper/compile'
import type { BindEvent, Mark, StepEvent } from '../store'
import type { CompileResult, ExecWorkerApi, HostApi, LanguageWorkerApi, ResumeMode } from './protocol'

import { createContext as createWorkerChannel } from '@moeru/eventa/adapters/webworkers'
import { createBirpc } from 'birpc'

import { RecordingBackend, ReplayBackend } from '../backend/replay'
import { reserveDevice, withDeviceRun } from '../features/device/device-target'
import { actions, scriptDesktop, scriptWorkspace, usePlayground } from '../features/editor/script-workspace'
import { boundsOf } from '../handles'
import { StepTimer } from '../stepper/timing'
import { recordCall } from './record-call'
import { SDK_PORT_MESSAGE, serveBridge } from './sdk-bridge'

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
    return this.#active !== undefined
  }

  #active?: { abort: AbortController, completion: Promise<void> }
  #batch: EventBatch
  #bindHits: number[] = []
  #binds: BindSite[] = []
  #cellId = 0
  #exec!: BirpcReturn<ExecWorkerApi, HostApi>
  #lastLine: null | number = null
  #lineHits = new Map<number, number>()
  #port?: MessagePort
  #recording?: Recording
  #runBackend: null | RunBackend = null
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

  /** Terminates the worker (also escapes infinite sync loops) and clears REPL state. */
  reset(): Promise<void> | undefined {
    const active = this.#active
    active?.abort.abort()
    this.#worker?.terminate()
    this.#port?.close()
    this.#exec.$close()
    this.#batch.flush()
    actions.resetSession()
    this.#spawn()
    return active?.completion
  }

  // TODO(step-over): `step` pauses at the next hook anywhere, so it steps into
  // called async functions. Step-over needs call-depth tracking in the stepper
  // (enter/exit hooks around function bodies).
  resume(mode: ResumeMode): void {
    this.#exec.resume(mode)
  }

  run(options: RunOptions): Promise<void> | undefined {
    const savedRecording = options.replay ? this.#recording : undefined
    if (this.running || (options.replay && !savedRecording))
      return
    const release = savedRecording ? undefined : reserveDevice()
    if (!savedRecording && !release)
      return
    const abort = new AbortController()
    const completion = this.#run(options, abort.signal, savedRecording).finally(() => {
      abort.abort()
      this.#active = undefined
      this.#runBackend = null
      release?.()
    })
    this.#active = { abort, completion }
    return completion
  }

  setBreakpoints(lines: number[]): void {
    this.#exec.setBreakpoints(lines)
  }

  stop(): void {
    this.#exec.stop()
  }

  async #execute(compiled: Extract<CompileResult, { ok: true }>, mode: RunOptions['mode'], backend: null | RunBackend, signal: AbortSignal): Promise<RunOutcomeKind> {
    if (signal.aborted)
      return 'canceled'
    const timer = this.#timer = new StepTimer(compiled.steps)
    this.#runBackend = backend
    const sdkRoute = backend?.sdk().route
    actions.patchRun({ runId: sdkRoute?.runId, status: 'running' })
    const outcome = await this.#exec.run({
      binds: compiled.binds,
      breakpoints: usePlayground.getState().breakpoints,
      code: compiled.code,
      hoisted: compiled.hoisted,
      mode,
      prelude: compiled.prelude,
      sdkRoute,
      steps: compiled.steps,
    })
    if (signal.aborted)
      return 'canceled'
    timer.finish(performance.timeOrigin + performance.now())
    this.#batch.flush()
    const timings = timer.snapshot()
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
    return outcome.status === 'ok' ? 'succeeded' : outcome.status === 'stopped' ? 'canceled' : 'failed'
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

  async #run(options: RunOptions, signal: AbortSignal, savedRecording: Recording | undefined): Promise<void> {
    this.#seq = 0
    this.#lineHits = new Map()
    this.#lastLine = null
    actions.beginRun(savedRecording ? 'replay' : 'live')
    try {
      const compiled = await language.compile({ cellId: ++this.#cellId, source: options.source, startLine: options.startLine })
      if (signal.aborted)
        return
      if (!compiled.ok) {
        actions.patchRun({ endedAt: Date.now(), errorLine: compiled.line, errorMessage: compiled.message, status: 'error' })
        return
      }
      this.#steps = compiled.steps
      this.#binds = compiled.binds
      this.#bindHits = Array.from<number>({ length: compiled.binds.length }).fill(0)

      if (savedRecording) {
        await this.#execute(compiled, options.mode, new ReplayBackend(savedRecording), signal)
        return
      }

      const recorder = await withDeviceRun(signal, async (run) => {
        const recorder = run ? new RecordingBackend(run) : null
        const outcome = await this.#execute(compiled, options.mode, recorder, signal)
        return { outcome, value: recorder }
      })
      if (signal.aborted)
        return
      if (recorder) {
        this.#recording = recorder.recording()
        usePlayground.setState({ hasRecording: true })
      }
      void scriptDesktop.refreshAxTree().catch(() => {})
    }
    catch (error) {
      if (!signal.aborted)
        throw error
    }
  }

  #spawn(): void {
    const worker = new Worker(new URL('../workers/exec.worker.ts', import.meta.url), { type: 'module' })
    this.#worker = worker
    const lineOf = (stepId: null | number) => stepId === null ? null : this.#steps[stepId]?.line ?? null
    // Scripts' `auv` calls (`@auv-js/sdk`) arrive here encoded, on their own port.
    const channel = new MessageChannel()
    this.#port = channel.port1
    worker.postMessage({ type: SDK_PORT_MESSAGE }, [channel.port2])
    serveBridge(createWorkerChannel(channel.port1 as unknown as Worker).context, {
      record: async (start) => {
        const line = lineOf(start.stepId)
        const signal = this.#active?.abort.signal
        if (!signal || this.#worker !== worker)
          return undefined
        const hit = line === null ? 1 : this.#lineHits.get(line) ?? 1
        return recordCall(scriptWorkspace, this.#runBackend, start, { hit, line }, () => {
          const seq = this.#nextSeq()
          this.#batch.flush()
          return seq
        }, signal)
      },
      target: () => {
        const signal = this.#active?.abort.signal
        if (!signal || signal.aborted || this.#worker !== worker)
          return 'No Script execution is active'
        const target = this.#runBackend?.sdk().target
        return target ? { ...target, signal } : '`auv` needs a device, the mock desktop, or a replay of a run that made the same calls'
      },
    })
    const host: HostApi = {
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
      on: fn => worker.addEventListener('message', (event) => {
        if (this.#worker === worker)
          fn(event.data)
      }),
      post: data => worker.postMessage(data),
      timeout: 24 * 60 * 60_000,
    })
  }
}

export const session = new ExecSession()

/** Canvas shape for a `draw()` target: rectangles (and areas) by bounds, otherwise a point. */
function markShape(value: unknown): Mark['shape'] | undefined {
  const target = value as null | Partial<Rect> | undefined
  if (typeof target?.x !== 'number' || typeof target.y !== 'number')
    return undefined
  if (typeof target.width === 'number' && typeof target.height === 'number')
    return { rect: { height: target.height, width: target.width, x: target.x, y: target.y } }
  return { point: { x: target.x, y: target.y } }
}
