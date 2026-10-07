import type { ExecWorkerApi, HostApi, LogLevel, ResumeMode, RunOutcome, RunRequest, WireValue } from '../runtime/protocol'
import type { AuvScriptApi, ClickOptions, Point, Rect, ScrollDelta, ScrollUntilUpdate, ScrollUntilOptions, TextSearchOptions, WindowHandle } from '../script-api/api'
/// <reference lib="webworker" />
import type { StepSite } from '../stepper/compile'

import { createBirpc } from 'birpc'

import { HOST_EVENTS } from '../runtime/protocol'
import { areaOf } from './area'

// NOTICE(exec-worker-isolation): this worker only isolates the playground
// page from runaway loops (it can be terminated) and keeps the DOM out of
// reach. It shares the page origin, so it is not a sandbox for untrusted
// code; see README "Sandboxing".

// NOTICE(stop-signal): stopping throws from the next hook; a user `try/catch`
// around that statement can swallow it. Reset (worker termination) always works.
class StopSignal extends Error {
  constructor() {
    super('Execution stopped')
    this.name = 'StopSignal'
  }
}

let steps: StepSite[] = []
let mode: 'run' | 'step' = 'run'
let breakpoints = new Set<number>()
let resumeWaiter: ((next: 'stop' | ResumeMode) => void) | undefined
let stopRequested = false
let currentStep: null | number = null
const persistentNames = new Set<string>()

const now = () => performance.timeOrigin + performance.now()

// `scrollUntil` predicates by id while their call runs; the host asks via `decide`.
const predicates = new Map<number, (update: ScrollUntilUpdate) => boolean | Promise<boolean>>()
let nextPredicate = 1

const api: ExecWorkerApi = {
  async decide(predicateId, update) {
    const predicate = predicates.get(predicateId)
    if (!predicate)
      throw new Error(`scrollUntil predicate ${predicateId} is no longer running`)
    return Boolean(await predicate(update))
  },
  resume(next) {
    resumeWaiter?.(next)
  },
  async run(request) {
    return await runCell(request)
  },
  setBreakpoints(lines) {
    breakpoints = new Set(lines)
  },
  stop() {
    stopRequested = true
    resumeWaiter?.('stop')
  },
}

const host = createBirpc<HostApi, ExecWorkerApi>(api, {
  eventNames: [...HOST_EVENTS],
  on: fn => addEventListener('message', event => fn(event.data)),
  post: data => postMessage(data),
  // Script bindings can legitimately take long (OCR, slow apps).
  timeout: 10 * 60_000,
})

// ---- Step hooks -------------------------------------------------------------

const globals = globalThis as unknown as Record<string, unknown>

globals.__step = async (id: number) => {
  if (stopRequested)
    throw new StopSignal()
  currentStep = id
  const site = steps[id]
  host.onStep(id, now())
  if (site?.top)
    publishVars()
  if (site && (mode === 'step' || breakpoints.has(site.line))) {
    host.onPause(id, now())
    const next = await new Promise<'stop' | ResumeMode>((resolve) => {
      resumeWaiter = resolve
    })
    resumeWaiter = undefined
    if (next === 'stop')
      throw new StopSignal()
    host.onResume(now())
    mode = next === 'step' ? 'step' : 'run'
  }
}

globals.__stepSync = (id: number) => {
  currentStep = id
  host.onStep(id, now())
}

globals.__bind = (id: number, values: Record<string, unknown>) => {
  const wire: Record<string, WireValue> = {}
  for (const [name, value] of Object.entries(values))
    wire[name] = toWire(value)
  host.onBind(id, wire, now())
}

function attachWindowMethods(window: WindowHandle): void {
  const ref = { $ref: window.$ref, kind: 'window' }
  Object.defineProperties(window, {
    capture: { value: () => call('windows.capture', ref) },
    click: { value: (point: Point, options?: ClickOptions) => call('windows.click', ref, point, options) },
    findText: { value: (query: string, options?: TextSearchOptions) => call('windows.findText', ref, query, options) },
    scroll: { value: (at: Point | Rect, delta: ScrollDelta) => call('windows.scroll', ref, at, delta) },
    scrollUntil: {
      value: async (at: Point | Rect, options: ScrollUntilOptions) => {
        const { until, ...rest } = options ?? {}
        if (!until)
          return await call('windows.scrollUntil', ref, at, rest)
        // Functions cannot cross to the host; send a placeholder it calls back through `decide`.
        const predicateId = nextPredicate++
        predicates.set(predicateId, until)
        try {
          return await call('windows.scrollUntil', ref, at, { ...rest, until: { $predicate: predicateId } })
        }
        finally {
          predicates.delete(predicateId)
        }
      },
    },
  })
}

async function call<T>(method: string, ...args: unknown[]): Promise<T> {
  const result = await host.call(method, args.map(arg => toWire(arg)), currentStep)
  return hydrate(result) as T
}

function errorLine(error: Error): number | undefined {
  const match = error.stack?.match(/cell-[^:]+\.ts\.js:(\d+):\d+/)
  if (match)
    return Number(match[1])
  return currentStep === null ? undefined : steps[currentStep]?.line
}

// ---- Values crossing to the host ---------------------------------------------

/** Re-attaches methods to handles returned by the host. */
function hydrate(value: unknown): unknown {
  if (Array.isArray(value))
    return value.map(hydrate)
  if (!value || typeof value !== 'object')
    return value
  const object = value as Record<string, unknown>
  for (const key of Object.keys(object))
    object[key] = hydrate(object[key])
  if (object.kind === 'window' && typeof object.$ref === 'string')
    attachWindowMethods(object as unknown as WindowHandle)
  return object
}

function publishVars(): void {
  const vars: Record<string, WireValue> = {}
  for (const name of persistentNames) {
    if (name in globals)
      vars[name] = toWire(globals[name])
  }
  host.onVars(vars)
}

async function runCell(request: RunRequest): Promise<RunOutcome> {
  steps = request.steps
  mode = request.mode
  breakpoints = new Set(request.breakpoints)
  stopRequested = false
  currentStep = null
  request.hoisted.forEach(name => persistentNames.add(name))
  try {
    // Indirect eval runs in global scope: `var` declarations become global
    // properties, which is how top-level declarations persist across cells.
    // oxlint-disable-next-line no-eval -- cells run as scripts in global scope by design.
    ;(0, eval)(request.prelude)
    // oxlint-disable-next-line no-eval -- see above.
    const value: unknown = await (0, eval)(request.code)
    publishVars()
    return { status: 'ok', value: toWire(value) }
  }
  catch (error) {
    publishVars()
    if (error instanceof StopSignal)
      return { status: 'stopped' }
    const err = error instanceof Error ? error : new Error(String(error))
    return { line: errorLine(err), message: `${err.name}: ${err.message}`, stack: err.stack, status: 'error' }
  }
}

/** Structured-clone-safe summary of a script value; keeps resource handles intact. */
function toWire(value: unknown, depth = 0, seen = new WeakSet<object>()): WireValue {
  if (value === null || value === undefined)
    return value
  switch (typeof value) {
    case 'bigint':
      return `${value}n`
    case 'boolean':
    case 'number':
    case 'string':
      return value
    case 'function':
      return { $fn: value.name || 'anonymous' }
    case 'symbol':
      return value.toString()
  }
  const object = value as object
  if (seen.has(object))
    return { $circular: true }
  if (depth > 6)
    return { $truncated: true }
  seen.add(object)
  if (object instanceof Error)
    return { $error: `${object.name}: ${object.message}` }
  if (ArrayBuffer.isView(object))
    return { $bytes: object.byteLength }
  if (object instanceof Map)
    return { $map: [...object.entries()].slice(0, 200).map(([k, v]) => [toWire(k, depth + 1, seen), toWire(v, depth + 1, seen)]) }
  if (object instanceof Set)
    return { $set: [...object].slice(0, 200).map(v => toWire(v, depth + 1, seen)) }
  if (Array.isArray(object))
    return object.slice(0, 500).map(item => toWire(item, depth + 1, seen))
  const out: Record<string, WireValue> = {}
  for (const [key, item] of Object.entries(object))
    out[key] = toWire(item, depth + 1, seen)
  return out
}

// ---- Script globals -------------------------------------------------------------

const auv: AuvScriptApi = {
  apps: {
    activate: bundleId => call('apps.activate', bundleId),
  },
  displays: {
    capture: display => call('displays.capture', display),
    findText: (query, display, options) => call('displays.findText', query, display, options),
    list: () => call('displays.list'),
  },
  input: {
    click: (point, options) => call('input.click', point, options),
    pressKey: key => call('input.pressKey', key),
    typeText: text => call('input.typeText', text),
  },
  text: {
    recognize: (frame, options) => call('text.recognize', frame, options),
  },
  windows: {
    list: () => call('windows.list'),
    resolve: selector => call('windows.resolve', selector),
  },
}

globals.auv = auv
globals.area = areaOf
globals.focus = <T>(target: T, options?: { autoZoomOut?: boolean, zoom?: boolean }): T => {
  host.onFocus(toWire(target), options, currentStep, now())
  return target
}
globals.draw = <T>(target: T, label?: string): T => {
  host.onDraw(toWire(target), label, currentStep)
  return target
}
globals.centerOf = (rect: Rect): Point => ({ x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 })
globals.sleep = async (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
globals.show = <T>(value: T, label?: string): T => {
  host.onLog('show', [toWire(value)], currentStep, label)
  return value
}

for (const level of ['error', 'info', 'log', 'warn'] satisfies LogLevel[]) {
  // Mirror cell console output into the playground console with its line.
  // eslint-disable-next-line no-console -- replaces, not calls, the worker console.
  console[level] = (...values: unknown[]) => host.onLog(level, values.map(value => toWire(value)), currentStep)
}
