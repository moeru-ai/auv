import type { ExecWorkerApi, HostApi, LogLevel, ResumeMode, RunOutcome, RunRequest, WireValue } from '../runtime/protocol'
import type { Point, Rect } from '../script-api/api'
/// <reference lib="webworker" />
import type { StepSite } from '../stepper/compile'

import { createContext as createHostChannel } from '@moeru/eventa/adapters/webworkers/worker'
import { createBirpc } from 'birpc'

import * as sdk from '@auv-js/sdk'

import { HOST_EVENTS } from '../runtime/protocol'
import { createBridgeTransport, SDK_PORT_MESSAGE } from '../runtime/sdk-bridge'
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

const globals = globalThis as unknown as Record<string, unknown>
const persistentNames = new Set<string>()

const now = () => performance.timeOrigin + performance.now()

const api: ExecWorkerApi = {
  resume(next) {
    resumeWaiter?.(next)
  },
  async run(request) {
    globals.auv = await runnerFor(request.sdkRoute)
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
  on: fn => addEventListener('message', (event) => {
    if ((event.data as null | { type?: unknown })?.type !== SDK_PORT_MESSAGE)
      fn(event.data)
  }),
  post: data => postMessage(data),
  timeout: 10 * 60_000,
})

// ---- SDK -------------------------------------------------------------------
// `auv` is the `@auv-js/sdk` Runner client for the selected Device and the
// current Run, the same object a Node script gets from
// `createAuv(await connect(...)).runner(route)`, and `sdk` is the module. Calls
// cross to the host encoded, on their own port (`runtime/sdk-bridge.ts`),
// where they are recorded for the timeline and replay.

let sdkConnection: Promise<sdk.AuvConnection> | undefined

addEventListener('message', (event) => {
  if ((event.data as null | { type?: unknown })?.type !== SDK_PORT_MESSAGE)
    return
  const port = event.ports[0]
  if (!port)
    return
  const { context } = createHostChannel({ messagePort: port as unknown as Worker })
  sdkConnection = sdk.connect({ transport: createBridgeTransport(context, () => currentStep) })
})

async function runnerFor(route: RunRequest['sdkRoute']): Promise<sdk.RunnerClient> {
  if (!route || !sdkConnection) {
    // Fails on first use, with the reason, rather than as `auv is undefined`.
    return new Proxy({} as sdk.RunnerClient, {
      get() {
        throw new Error('`auv` needs a device, the mock desktop, or a replay of a run that made the same calls')
      },
    })
  }
  return sdk.createAuv(await sdkConnection).runner(route)
}

// ---- Step hooks -------------------------------------------------------------

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

function errorLine(error: Error): number | undefined {
  const match = error.stack?.match(/cell-[^:]+\.ts\.js:(\d+):\d+/)
  if (match)
    return Number(match[1])
  return currentStep === null ? undefined : steps[currentStep]?.line
}

// ---- Values crossing to the host ---------------------------------------------

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

/** Structured-clone-safe summary of a script value; keeps protobuf `$typeName`s for lineage. */
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

globals.sdk = sdk
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
