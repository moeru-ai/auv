import type { BirpcReturn } from 'birpc'

import type { Recording } from '../backend/replay'
import type { Backend, RunBackend } from '../backend/types'
import type { Rect } from '../script-api/api'
import type { BindSite, StepSite } from '../stepper/compile'
import type { BindEvent, Mark, ScriptSource, StepEvent } from '../store'
import type { CodeModeState, ExecWorkerApi, HostApi, LanguageWorkerApi, ResumeMode, RunOutcome } from './protocol'

import { camelCaseName } from '@auv-js/sdk'
import { fromBinary, fromJsonString } from '@bufbuild/protobuf'
import { createContext as createWorkerChannel } from '@moeru/eventa/adapters/webworkers'
import { createBirpc } from 'birpc'

import { RecordingBackend, ReplayBackend } from '../backend/replay'
import { boundsOf } from '../handles'
import { decodeThumbHash } from '../preview'
import { StepTimer } from '../stepper/timing'
import { actions, nowMs, usePlayground } from '../store'
import { CallScope, decodeBitmap, recordRpcResources } from './rpc-resources'
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
  codemode?: { store: Record<string, unknown>, timeoutMs: number }
  /** Pause at the first statement. */
  mode: 'run' | 'step'
  /** Answer device calls from the previous live run's recording. */
  replay?: boolean
  signal?: AbortSignal
  source: string
  /** Editor line where `source` starts. */
  startLine: number
}

/** One worker and the RPC channels that share its lifetime. */
interface ExecutionWorker {
  exec: BirpcReturn<ExecWorkerApi, HostApi>
  port: MessagePort
  worker: Worker
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
 * Owns the editor worker and temporary codemode workers, and turns their
 * hook events into the run's event log. One cell runs at a time; editor
 * top-level names persist until `reset()`.
 */
class ExecSession {
  get running(): boolean {
    return this.#active !== undefined
  }

  #active?: ExecutionWorker
  #batch: EventBatch
  #bindHits: number[] = []
  #binds: BindSite[] = []
  #cellId = 0
  #codeOutput: CodeModeState['content'] = []
  #editor: ExecutionWorker
  #lastLine: null | number = null
  #lineHits = new Map<number, number>()
  #liveTimer?: number
  #recording?: Recording
  #runAbort?: AbortController
  #runBackend: null | RunBackend = null
  #seq = 0
  #steps: StepSite[] = []
  #timer?: StepTimer

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
    this.#editor = this.#spawn()
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
    if (this.#active && this.#active !== this.#editor) {
      this.stop()
      return
    }
    this.#editor.worker.terminate()
    this.#active = undefined
    actions.resetSession()
    this.#editor = this.#spawn()
  }

  // TODO(step-over): `step` pauses at the next hook anywhere, so it steps into
  // called async functions. Step-over needs call-depth tracking in the stepper
  // (enter/exit hooks around function bodies).
  resume(mode: ResumeMode): void {
    const worker = this.#active ?? this.#editor
    worker.exec.resume(mode)
  }

  async run(options: RunOptions): Promise<RunOutcome> {
    if (this.running)
      return { message: 'Another playground execution is running.', status: 'error' }
    const live = usePlayground.getState().backend
    if (options.replay && !this.#recording)
      return { message: 'No recording is available.', status: 'error' }
    if (options.signal?.aborted)
      return { status: 'stopped' }
    this.#codeOutput = []
    const worker = options.codemode ? this.#spawn() : this.#editor
    this.#active = worker
    const abort = new AbortController()
    this.#runAbort = abort
    const cancel = () => this.stop()
    options.signal?.addEventListener('abort', cancel, { once: true })
    let timer: ReturnType<typeof setTimeout> | undefined
    let timedOut = false
    if (options.codemode) {
      timer = setTimeout(() => {
        timedOut = true
        cancel()
      }, options.codemode.timeoutMs)
    }
    const recorder = !options.replay && live ? new RecordingBackend(live) : undefined
    this.#runBackend = options.replay ? new ReplayBackend(this.#recording!) : recorder ?? live
    this.#lineHits = new Map()
    this.#lastLine = null
    const script: ScriptSource = { id: ++this.#cellId, kind: options.codemode ? 'codemode' : 'editor', source: options.source, startLine: options.startLine }
    actions.beginRun(options.replay ? 'replay' : 'live', script, options.codemode !== undefined)
    this.#seq = usePlayground.getState().seqAt.length
    try {
      const compiled = await language.compile({ allowReturn: options.codemode !== undefined, cellId: script.id, recordOnly: options.codemode !== undefined, source: options.source, startLine: options.startLine })
      if (!compiled.ok) {
        actions.patchRun({ endedAt: Date.now(), errorLine: compiled.line, errorMessage: compiled.message, status: 'error' })
        return { line: compiled.line, message: compiled.message, status: 'error' }
      }
      this.#steps = compiled.steps
      this.#binds = compiled.binds
      this.#bindHits = Array.from<number>({ length: compiled.binds.length }).fill(0)
      this.#timer = new StepTimer(compiled.steps)
      abort.signal.throwIfAborted()
      const runId = await this.#runBackend?.beginRun()
      actions.patchRun({ runId, status: 'running' })
      const executing = worker.exec.run({
        binds: compiled.binds,
        breakpoints: usePlayground.getState().breakpoints,
        code: compiled.code,
        codemode: options.codemode,
        executionId: this.#cellId,
        hoisted: compiled.hoisted,
        mode: options.mode,
        prelude: compiled.prelude,
        sdkRoute: this.#runBackend?.sdk().route,
        steps: compiled.steps,
      })
      const outcome = options.codemode
        ? await Promise.race([executing, new Promise<RunOutcome>((resolve) => {
            const stopped = () => resolve({ codemode: { content: [...this.#codeOutput], store: options.codemode!.store }, reason: timedOut ? 'timeout' : 'canceled', status: 'stopped' })
            if (abort.signal.aborted)
              stopped()
            else abort.signal.addEventListener('abort', stopped, { once: true })
          })])
        : await executing
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
      // Run-control cleanup cannot replace the direct script result.
      await this.#runBackend?.endRun(outcome.status === 'ok' ? 'succeeded' : outcome.status === 'stopped' ? 'canceled' : 'failed')
        .catch(error => this.#log('warn', [`Closing the AUV Run failed: ${error instanceof Error ? error.message : String(error)}`], null))
      if (recorder) {
        this.#recording = recorder.recording()
        usePlayground.setState({ hasRecording: true })
      }
      void this.refreshAxTree().catch(() => {})
      return outcome
    }
    catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      actions.patchRun({ currentLine: null, endedAt: Date.now(), errorMessage: message, status: 'error' })
      await this.#runBackend?.endRun('failed').catch(() => {})
      if (recorder) {
        this.#recording = recorder.recording()
        usePlayground.setState({ hasRecording: true })
      }
      return { codemode: options.codemode && { content: [...this.#codeOutput], store: options.codemode.store }, message, status: 'error' }
    }
    finally {
      abort.abort()
      this.#batch.flush()
      clearTimeout(timer)
      options.signal?.removeEventListener('abort', cancel)
      if (options.codemode) {
        worker.worker.terminate()
        worker.port.close()
        worker.exec.$close()
      }
      this.#runAbort = undefined
      this.#active = undefined
    }
  }

  setBreakpoints(lines: number[]): void {
    const worker = this.#active ?? this.#editor
    worker.exec.setBreakpoints(lines)
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
          await captureLiveFrame(backend, display.id)
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
    if (this.#active && this.#active !== this.#editor) {
      this.#runAbort?.abort()
      this.#active.worker.terminate()
      return
    }
    this.#editor.exec.stop()
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

  #spawn(): ExecutionWorker {
    const worker = new Worker(new URL('../workers/exec.worker.ts', import.meta.url), { type: 'module' })
    const lineOf = (stepId: null | number) => stepId === null ? null : this.#steps[stepId]?.line ?? null
    // Scripts' `auv` calls (`@auv-js/sdk`) arrive here encoded, on their own port.
    const channel = new MessageChannel()
    worker.postMessage({ type: SDK_PORT_MESSAGE }, [channel.port2])
    serveBridge(createWorkerChannel(channel.port1 as unknown as Worker).context, {
      record: async (start) => {
        const script = usePlayground.getState().run.script!
        const backend = this.#runBackend
        const line = lineOf(start.stepId)
        const described = await backend?.sdk().describe(start.method)
        const decode = (body: Uint8Array, as: 'request' | 'response') => {
          try {
            return as === 'request' ? described?.decodeRequest(body) : described?.decodeResponse(body)
          }
          catch {
            return `${body.byteLength} bytes`
          }
        }
        const seq = this.#nextSeq()
        this.#batch.flush()
        const callId = actions.beginCall({
          args: start.body && described ? [decode(start.body, 'request')] : [],
          effect: described?.effect === 'input' ? 'input' : 'read',
          hit: line === null ? 1 : this.#lineHits.get(line) ?? 1,
          line,
          // The method's API name as JavaScript spells it (`windows.list`), or
          // `Service/Method` without the package for a method that has none.
          method: described?.presentation ? camelCaseName(described.presentation.name) : start.method.slice(start.method.lastIndexOf('.') + 1),
          rpc: { path: start.method, presentation: described?.presentation },
          script,
          seq,
          startedAt: nowMs(),
        })
        // TODO(playground-sdk-stream-progress): a stream's resources appear
        // when it ends; show each scroll-until step as it arrives.
        return (end) => {
          if (this.#cellId !== script.id)
            return
          const responses = end.exchange.flatMap(frame => 'response' in frame ? [frame.response] : [])
          const results = end.json !== undefined ? [JSON.parse(end.json) as unknown] : responses.map(body => decode(body, 'response'))
          const scope = new CallScope(callId, seq, backend)
          if (described) {
            try {
              const request = start.body && fromBinary(described.input, start.body)
              const messages = end.json !== undefined
                ? [fromJsonString(described.output, end.json, { ignoreUnknownFields: true })]
                : responses.map(body => fromBinary(described.output, body))
              recordRpcResources(scope, start.method, request || undefined, messages)
            }
            catch (error) {
              console.warn(`Showing ${start.method} on the canvas failed`, error)
            }
          }
          actions.endCall(callId, {
            endSeq: this.#nextSeq(),
            error: end.error?.message,
            refs: scope.refs,
            result: results.length === 1 ? results[0] : results,
            status: end.error === undefined ? 'ok' : 'error',
          })
        }
      },
      target: executionId => executionId !== this.#cellId || !this.running || this.#runAbort?.signal.aborted
        ? 'This script execution has ended.'
        : this.#runBackend
          ? { ...this.#runBackend.sdk().target, signal: this.#runAbort?.signal }
          : '`auv` needs a device, the mock desktop, or a replay of a run that made the same calls',
    })
    const host: HostApi = {
      onBind: (bindId, values, at) => {
        const site = this.#binds[bindId]
        if (!site)
          return
        const hit = ++this.#bindHits[bindId]!
        this.#batch.binds.push({ at, hit, line: site.line, scriptId: this.#cellId, seq: this.#nextSeq(at), values })
      },
      onCodeOutput: (content) => {
        this.#codeOutput.push(content)
        this.#log('show', [content.type === 'text' ? content.text : 'Image emitted to the agent.'], this.#lastLine)
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
        this.#batch.steps.push({ at, hit, line: site.line, scriptId: this.#cellId, seq: this.#nextSeq(at) })
      },
      onVars: (vars) => {
        usePlayground.setState({ vars })
      },
    }
    const exec = createBirpc<ExecWorkerApi, HostApi>(host, {
      // Control messages to the worker are fire-and-forget; only `run` returns.
      eventNames: ['resume', 'setBreakpoints', 'stop'],
      on: fn => worker.addEventListener('message', event => fn(event.data)),
      post: data => worker.postMessage(data),
      timeout: 24 * 60 * 60_000,
    })
    return { exec, port: channel.port1, worker }
  }
}

export const session = new ExecSession()

/**
 * Switches the backend scripts and the canvas use, and refreshes displays.
 * The device registry (`features/device`) owns backend lifecycles, so the
 * previous backend stays open.
 */
export async function activateBackend(backend: Backend | null): Promise<void> {
  session.setLive(false)
  usePlayground.setState({ axTree: null, backend, displays: [], liveFrames: {} })
  if (!backend)
    return
  const displays = await backend.listDisplays()
  usePlayground.setState({ displays })
  // One snapshot per display so the canvas is not empty before live mode.
  for (const display of displays) {
    void captureLiveFrame(backend, display.id)
      .catch(error => console.warn(`Initial capture failed for display ${display.id}`, error))
  }
  void session.refreshAxTree().catch(() => {})
}

/**
 * Captures a display for the canvas. A display with no pixels on screen yet
 * shows the capture's ThumbHash preview first, so a new connection paints at
 * once; later captures replace the previous pixels when they load.
 */
async function captureLiveFrame(backend: Backend, displayId: string): Promise<void> {
  const frame = await backend.captureDisplay(displayId, { logical: true })
  const capturedAt = Date.now()
  const shown = usePlayground.getState().liveFrames[displayId]
  const preview = shown?.bitmap ? undefined : await decodeThumbHash(frame.thumbhash)
  if (preview)
    actions.setLiveFrame(displayId, { bounds: frame.bounds, capturedAt, preview })
  const bitmap = await decodeBitmap(backend, frame)
  actions.setLiveFrame(displayId, { bitmap, bounds: frame.bounds, capturedAt, preview, revealedAt: preview ? performance.now() : undefined })
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
