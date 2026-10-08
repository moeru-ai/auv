import type { MethodPresentation } from '@auv-js/sdk'

import type { AxNode, Backend, CapturedFrame, DisplayInfo } from './backend/types'
import type { RevealedImage } from './preview'
import type { LogLevel, RunOutcome, WireValue } from './runtime/protocol'
import type { DisplayHandle, FrameHandle, InputHandle, Point, Rect, TextHandle, WindowHandle } from './script-api/api'
import type { TimingSnapshot } from './stepper/timing'

import { create } from 'zustand'

export interface BindEvent {
  at: number
  hit: number
  line: number
  seq: number
  values: Record<string, WireValue>
}

export interface CallRecord {
  args: WireValue[]
  effect: Effect
  endedAt?: number
  /** Event-log position when the call settled. */
  endSeq?: number
  error?: string
  /** 1-based count of how often the calling line had run, e.g. loop iteration. */
  hit: number
  id: number
  line: null | number
  method: string
  /** Resource refs produced by this call, in creation order. */
  refs: string[]
  result?: WireValue
  /** A direct SDK call: its gRPC path and how the method presents itself. */
  rpc?: { path: string, presentation?: MethodPresentation }
  /** Event-log position when the call started. */
  seq: number
  startedAt: number
  status: 'error' | 'ok' | 'pending'
}

/** Whether a binding call only observes the device or delivers input to it. */
export type Effect = 'input' | 'read'

/** A camera move recorded by `focus()`; the canvas eases to the latest one at the cursor. */
export interface FocusEvent {
  /** Epoch ms when the script asked for it. */
  at: number
  autoZoomOut: boolean
  id: number
  line: null | number
  rect: Rect
  seq: number
  zoom: boolean
}

export type InspectorTab = 'ax' | 'call' | 'handle'

/** The latest display capture in live mode; a ThumbHash preview until its pixels load. */
export interface LiveFrame extends RevealedImage {
  bounds: CapturedFrame['bounds']
  capturedAt: number
}

export interface LogEntry {
  at: number
  id: number
  label?: string
  level: LogLevel
  line: null | number
  seq: number
  values: WireValue[]
}

/** An editor-only overlay recorded by `draw()`; shown on the canvas from its `seq` on. */
export interface Mark {
  id: number
  label?: string
  line: null | number
  seq: number
  /** Screen-space rectangle, or a point drawn as a target marker. */
  shape: { point: Point } | { rect: Rect }
}

/** A hover card pinned as a floating panel; it shows one moment, not the latest value. */
export interface Pin {
  hit?: number
  id: string
  label: string
  line?: null | number
  ref?: string
  seq: number
  value?: WireValue
  x: number
  y: number
}

export interface PlaygroundState {
  axTree: AxNode | null
  backend: Backend | null
  binds: BindEvent[]
  breakpoints: number[]
  calls: CallRecord[]
  /** Time cursor as an event-log position; `null` follows the latest event. */
  cursor: null | number
  displays: DisplayInfo[]
  /** Lines whose hooks fired during the current run. */
  executedLines: number[]
  /** `focus()` camera moves of the current run. */
  focuses: FocusEvent[]
  /** Whether a recording of the last live run is available for offline replay. */
  hasRecording: boolean
  hoveredCallId: null | number
  /** Screen-space rectangle highlighted from the AX tree or elsewhere. */
  hoveredRect: null | Rect
  hoveredRef: null | string
  inspectorTab: InspectorTab
  live: boolean
  liveFrames: Record<string, LiveFrame>
  logs: LogEntry[]
  /** `draw()` overlays of the current run. */
  marks: Mark[]
  /** Debugger pauses of the current run (epoch ms); excluded from the time strip. */
  pauses: Array<{ end?: number, start: number }>
  pins: Pin[]
  resources: Record<string, Resource>
  run: RunState
  /** Increments per run; scopes seq-based queries to the current run. */
  runIndex: number
  selectedAxPath: null | string
  /** Call shown in the inspector's Call tab; per run. */
  selectedCallId: null | number
  selectedRef: null | string
  /** Wall-clock time (epoch ms) of every event, indexed by seq. */
  seqAt: number[]
  steps: StepEvent[]
  timings: TimingSnapshot
  /** Latest summarized values of persistent top-level names. */
  vars: Record<string, WireValue>
}

/**
 * Host-side record behind a script handle. Pixels never cross into the
 * worker. `seq` is the event-log position of the call that produced it.
 */
export type Resource = (
  | (RevealedImage & {
    // `bitmap` is a display copy at logical resolution, loaded after the call
    // returns; `preview` shows meanwhile.
    capturedAt: number
    frame: CapturedFrame
    handle: FrameHandle
    kind: 'frame'
  })
  | { handle: DisplayHandle, kind: 'display' }
  | { handle: InputHandle, kind: 'input' }
  | { handle: TextHandle, kind: 'text' }
  | { handle: WindowData, kind: 'window' }
) & {
  callId?: number
  /** Run that produced the resource; seqs are only comparable within one run. */
  run: number
  seq: number
}

export interface RunState {
  currentLine: null | number
  endedAt?: number
  errorLine?: number
  errorMessage?: string
  /** `replay` answers bindings from the previous run's recording. */
  mode: 'live' | 'replay'
  outcome?: RunOutcome
  /** AUV Run ID when the backend opened one. */
  runId?: string
  startedAt?: number
  status: RunStatus
}

export type RunStatus = 'compiling' | 'done' | 'error' | 'idle' | 'paused' | 'running' | 'stopped'

export interface StepEvent {
  at: number
  /** 1-based count of how often this line had been reached in the run. */
  hit: number
  line: number
  seq: number
}

export type WindowData = Omit<WindowHandle, 'capture' | 'click' | 'findText' | 'pressKey' | 'scroll' | 'scrollUntil' | 'typeText'>

const emptyTimings: TimingSnapshot = { lines: [], statements: [] }

export const usePlayground = create<PlaygroundState>(() => ({
  axTree: null,
  backend: null,
  binds: [],
  breakpoints: [],
  calls: [],
  cursor: null,
  displays: [],
  executedLines: [],
  focuses: [],
  hasRecording: false,
  hoveredCallId: null,
  hoveredRect: null,
  hoveredRef: null,
  inspectorTab: 'handle',
  live: false,
  liveFrames: {},
  logs: [],
  marks: [],
  pauses: [],
  pins: [],
  resources: {},
  run: { currentLine: null, mode: 'live', status: 'idle' },
  runIndex: 0,
  selectedAxPath: null,
  selectedCallId: null,
  selectedRef: null,
  seqAt: [],
  steps: [],
  timings: emptyTimings,
  vars: {},
}))

const set = usePlayground.setState
const get = usePlayground.getState

let nextCallId = 1
let nextLogId = 1
let nextMarkId = 1
let nextPinId = 1

/** Epoch milliseconds with sub-millisecond precision; the worker uses the same basis. */
export function nowMs(): number {
  return performance.timeOrigin + performance.now()
}

export const actions = {
  addFocus(focus: Omit<FocusEvent, 'id'>): void {
    set(state => ({ focuses: [...state.focuses, { ...focus, id: nextMarkId++ }] }))
  },

  addLog(entry: Omit<LogEntry, 'id'>): void {
    set(state => ({ logs: [...state.logs.slice(-999), { ...entry, id: nextLogId++ }] }))
  },

  addMark(mark: Omit<Mark, 'id'>): void {
    set(state => ({ marks: [...state.marks, { ...mark, id: nextMarkId++ }] }))
  },

  addPin(pin: Omit<Pin, 'id' | 'x' | 'y'> & Partial<Pick<Pin, 'x' | 'y'>>): void {
    const offset = (get().pins.length % 6) * 28
    set(state => ({ pins: [...state.pins, { x: 120 + offset, y: 120 + offset, ...pin, id: `pin-${nextPinId++}` }] }))
  },

  /** Appends a batch of step/bind events and their timestamps. */
  appendEvents(steps: StepEvent[], binds: BindEvent[], seqAt: Array<[number, number]>): void {
    set((state) => {
      const times = state.seqAt.slice()
      for (const [seq, at] of seqAt)
        times[seq] = at
      return {
        binds: binds.length > 0 ? [...state.binds, ...binds] : state.binds,
        seqAt: times,
        steps: steps.length > 0 ? [...state.steps, ...steps] : state.steps,
      }
    })
  },

  beginCall(call: Omit<CallRecord, 'id' | 'refs' | 'status'>): number {
    const id = nextCallId++
    set(state => ({ calls: [...state.calls, { ...call, id, refs: [], status: 'pending' }] }))
    return id
  },

  /** Clears per-run records but keeps resources so earlier handles stay inspectable. */
  beginRun(mode: RunState['mode']): void {
    set(state => ({
      binds: [],
      calls: [],
      cursor: null,
      executedLines: [],
      focuses: [],
      hoveredCallId: null,
      marks: [],
      pauses: [],
      run: { currentLine: null, mode, startedAt: Date.now(), status: 'compiling' },
      runIndex: state.runIndex + 1,
      selectedCallId: null,
      seqAt: [],
      steps: [],
      timings: emptyTimings,
    }))
  },

  clearConsole(): void {
    set({ logs: [] })
  },

  clearPins(): void {
    set({ pins: [] })
  },

  endCall(id: number, patch: Pick<CallRecord, 'endSeq' | 'error' | 'refs' | 'result' | 'status'>): void {
    set(state => ({ calls: state.calls.map(call => call.id === id ? { ...call, ...patch, endedAt: nowMs() } : call) }))
  },

  movePin(id: string, x: number, y: number): void {
    set(state => ({ pins: state.pins.map(pin => pin.id === id ? { ...pin, x, y } : pin) }))
  },

  patchRun(patch: Partial<RunState>): void {
    set(state => ({ run: { ...state.run, ...patch } }))
  },

  pauseAt(at: number): void {
    set(state => ({ pauses: [...state.pauses, { start: at }] }))
  },

  putResource(ref: string, resource: Resource): void {
    set(state => ({ resources: { ...state.resources, [ref]: resource } }))
  },

  removePin(id: string): void {
    set(state => ({ pins: state.pins.filter(pin => pin.id !== id) }))
  },

  resetSession(): void {
    for (const resource of Object.values(get().resources)) {
      if (resource.kind === 'frame') {
        resource.bitmap?.close()
        resource.preview?.close()
      }
    }
    set({
      binds: [],
      calls: [],
      cursor: null,
      executedLines: [],
      focuses: [],
      logs: [],
      marks: [],
      pins: [],
      resources: {},
      run: { currentLine: null, mode: 'live', status: 'idle' },
      selectedCallId: null,
      selectedRef: null,
      seqAt: [],
      steps: [],
      timings: emptyTimings,
      vars: {},
    })
  },

  resumeAt(at: number): void {
    set(state => ({ pauses: state.pauses.map((pause, index) => index === state.pauses.length - 1 && pause.end === undefined ? { ...pause, end: at } : pause) }))
  },

  /** Shows a call in the inspector and focuses its newest handle on the canvas. */
  selectCall(id: number): void {
    const call = get().calls.find(candidate => candidate.id === id)
    set(state => ({ inspectorTab: 'call', selectedCallId: id, selectedRef: call?.refs.at(-1) ?? state.selectedRef }))
  },

  selectRef(ref: string): void {
    set({ inspectorTab: 'handle', selectedRef: ref })
  },

  setCursor(cursor: null | number): void {
    set({ cursor })
  },

  setLiveFrame(displayId: string, frame: LiveFrame): void {
    const previous = get().liveFrames[displayId]
    set(state => ({ liveFrames: { ...state.liveFrames, [displayId]: frame } }))
    // A frame can keep its predecessor's preview while its pixels load.
    if (previous?.bitmap !== frame.bitmap)
      previous?.bitmap?.close()
    if (previous?.preview !== frame.preview)
      previous?.preview?.close()
  },

  // TODO(breakpoint-anchors): breakpoints are plain line numbers and do not
  // follow edits; map them through document changes if that becomes annoying.
  toggleBreakpoint(line: number): number[] {
    const current = new Set(get().breakpoints)
    if (current.has(line))
      current.delete(line)
    else
      current.add(line)
    const breakpoints = [...current].sort((a, b) => a - b)
    set({ breakpoints })
    return breakpoints
  },
}
