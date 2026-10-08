import type { Backend, CapturedFrame, DisplayInfo, InputReceipt, ScrollUntilRequest, TextSearchResult, WindowInfo } from '../backend/types'
import type { ClickOptions, DisplayHandle, FrameHandle, InputHandle, KeyboardOptions, Point, Rect, ScrollDelta, ScrollUntilUpdate, TextHandle, WindowSelector } from '../script-api/api'
import type { Effect, Resource, WindowData } from '../store'
import type { WireValue } from './protocol'

import { actions, nowMs, usePlayground } from '../store'

let nextHandle = 1

interface Binding {
  effect: Effect
  run: (backend: Backend, scope: CallScope, args: WireValue[], context: CallContext) => Promise<WireValue>
}

// NOTICE(scroll-until-defaults): the same defaults as `auv invoke input.scroll-until`
// (`crates/auv-cli-invoke/src/commands/input.rs`): 50 steps, 400 ms settle so lazy
// lists can render the next page, and 2 confirmations so one slow load does
// not end the scan. The protocol requires `maxSteps` and `confirmations` > 0.
const SCROLL_UNTIL_DEFAULTS = { confirmations: 2, maxSteps: 50, settleMs: 400 }

type ResourceBody = Resource extends infer R ? R extends unknown ? Omit<R, 'callId' | 'run' | 'seq'> : never : never

/** Tracks the resource refs one binding call produced. */
class CallScope {
  readonly refs: string[] = []

  constructor(readonly callId: number, readonly seq: number, readonly backend: Backend | null) {}

  display(info: DisplayInfo): DisplayHandle {
    const handle: DisplayHandle = { $ref: `display:${info.id}`, frame: info.frame, id: info.id, kind: 'display', name: info.name, primary: info.primary, scale: info.scale }
    this.#put(handle.$ref, { handle, kind: 'display' })
    return handle
  }

  frame(frame: CapturedFrame): FrameHandle {
    const handle: FrameHandle = {
      $ref: `frame:${nextHandle++}`,
      bounds: frame.bounds,
      height: frame.height,
      kind: 'frame',
      scale: frame.scale,
      source: frame.source,
      width: frame.width,
    }
    this.#put(handle.$ref, { capturedAt: nowMs(), frame, handle, kind: 'frame' })
    if (this.backend) {
      void decodeBitmap(this.backend, frame).then((bitmap) => {
        const current = usePlayground.getState().resources[handle.$ref]
        if (current?.kind === 'frame')
          actions.putResource(handle.$ref, { ...current, bitmap })
        else
          bitmap.close()
      }, error => console.warn(`Loading ${handle.$ref} pixels failed`, error))
    }
    return handle
  }

  input(action: InputHandle['action'], receipt: InputReceipt, delta?: ScrollDelta): InputHandle {
    const handle: InputHandle = { $ref: `input:${nextHandle++}`, action, delta, kind: 'input', path: receipt.path, point: receipt.point }
    this.#put(handle.$ref, { handle, kind: 'input' })
    return handle
  }

  /**
   * `source` is the frame the recognizer read when the driver does not return
   * one; `within` is the screen-space area the search was limited to.
   */
  text(result: TextSearchResult, query?: string, source?: FrameHandle, within?: Rect): TextHandle {
    const handle: TextHandle = {
      $ref: `text:${nextHandle++}`,
      frame: result.capture ? this.frame(result.capture) : source,
      kind: 'text',
      matches: result.matches,
      query,
      text: result.text,
      within,
    }
    this.#put(handle.$ref, { handle, kind: 'text' })
    return handle
  }

  window(info: WindowInfo): WindowData {
    const handle: WindowData = { $ref: `window:${info.id}`, app: info.app, bundleId: info.bundleId, frame: info.frame, id: info.id, kind: 'window', pid: info.pid, title: info.title }
    // A window ref is stable across calls; keep the first producer for lineage.
    const existing = usePlayground.getState().resources[handle.$ref]
    this.refs.push(handle.$ref)
    const run = usePlayground.getState().runIndex
    const sameRun = existing?.run === run
    actions.putResource(handle.$ref, { callId: sameRun ? existing.callId : this.callId, handle, kind: 'window', run, seq: sameRun ? existing.seq : this.seq })
    return handle
  }

  #put(ref: string, resource: ResourceBody): void {
    this.refs.push(ref)
    actions.putResource(ref, { ...resource, callId: this.callId, run: usePlayground.getState().runIndex, seq: this.seq } as Resource)
  }
}

const read = (run: Binding['run']): Binding => ({ effect: 'read', run })
const input = (run: Binding['run']): Binding => ({ effect: 'input', run })

const BINDINGS: Record<string, Binding> = {
  'apps.activate': input(async (backend, scope, [bundleId]) => scope.input('activate', await backend.activateApp(String(bundleId)))),
  'displays.capture': read(async (backend, scope, [display]) => scope.frame(await backend.captureDisplay(displayId(display)))),
  'displays.findText': read(async (backend, scope, [query, display, options]) => {
    const within = searchArea(options, displayBounds(display))
    return scope.text(await backend.findDisplayText(String(query), displayId(display), within?.area), String(query), undefined, within?.area)
  }),
  'displays.list': read(async (backend, scope) => (await backend.listDisplays()).map(info => scope.display(info))),
  'input.click': input(async (backend, scope, [target, options]) => scope.input('click', await backend.clickScreen(clickPoint(target), options as ClickOptions | undefined))),
  'input.pressKey': input(async (backend, scope, [key]) => scope.input('key', await backend.pressKey(String(key)))),
  'input.typeText': input(async (backend, scope, [text]) => scope.input('type', await backend.typeText(String(text)))),
  'text.recognize': read(async (backend, scope, [frame, options]) => {
    const source = resolveFrame(frame)
    const within = searchArea(options, source.handle.bounds)
    return scope.text(await backend.recognizeText(source.frame, within?.area), undefined, source.handle, within?.area)
  }),
  'windows.capture': read(async (backend, scope, [window]) => scope.frame(await backend.captureWindow(windowId(window)))),
  'windows.click': input(async (backend, scope, [window, point, options]) => scope.input('click', await backend.clickWindow(windowId(window), asPoint(point), options as ClickOptions | undefined))),
  'windows.findText': read(async (backend, scope, [window, query, options]) => {
    const within = searchArea(options, windowBounds(window))
    return scope.text(await backend.findWindowText(windowId(window), String(query), within?.area), String(query), undefined, within?.area)
  }),
  'windows.list': read(async (backend, scope) => (await backend.listWindows()).map(info => scope.window(info))),
  'windows.pressKey': input(async (backend, scope, [window, key, options]) => scope.input('key', await backend.pressKeyWindow(windowId(window), String(key), options as KeyboardOptions | undefined))),
  'windows.resolve': read(async (backend, scope, [selector]) => scope.window(await backend.resolveWindow((selector ?? {}) as WindowSelector))),
  'windows.scroll': input(async (backend, scope, [window, at, delta]) => {
    const local = windowPoint(window, at)
    const amount = scrollDelta(delta)
    const receipt = await backend.scrollWindow(windowId(window), local.point, amount)
    return scope.input('scroll', { ...receipt, point: receipt.point ?? local.screen }, amount)
  }),
  'windows.scrollUntil': input(async (backend, scope, [window, at, options], context) => {
    const local = windowPoint(window, at)
    const { decide, request } = scrollUntilRequest(options, context)
    // TODO(repl-scroll-updates): only the last update's capture and
    // OCR become resources; record every step (with its own seq) when the
    // timeline should replay a scan step by step.
    const outcome = await backend.scrollWindowUntil(windowId(window), local.point, request, decide)
    const frame = outcome.capture ? scope.frame(outcome.capture) : undefined
    return {
      frame,
      input: outcome.receipt ? scope.input('scroll', { ...outcome.receipt, point: outcome.receipt.point ?? local.screen }, request.delta) : undefined,
      match: outcome.match,
      reason: outcome.reason,
      steps: outcome.steps,
      text: outcome.recognized ? scope.text(outcome.recognized, request.text, frame) : undefined,
    }
  }),
  'windows.typeText': input(async (backend, scope, [window, text, options]) => scope.input('type', await backend.typeTextWindow(windowId(window), String(text), options as KeyboardOptions | undefined))),
}

export interface CallContext {
  /** Asks the script worker's `scrollUntil` predicate `predicateId` about an update. */
  decide?: (predicateId: number, update: ScrollUntilUpdate) => Promise<boolean>
  /** 1-based hit count of the calling line (loop iteration). */
  hit: number
  line: null | number
  /** Allocates the next event-log position. */
  nextSeq: () => number
}

/**
 * Display bitmap for a capture at logical resolution (pixels ÷ `scale`): the
 * canvas and previews draw in logical points, and a Retina capture at full
 * size costs 4× the transfer, memory and a slow downscale per redraw. The
 * backend encodes the bounded image; `createImageBitmap` decodes it off the
 * main thread. OCR keeps reading the full-resolution capture by reference.
 * NOTICE(bitmap-logical-resolution): magnifiers (ClickLoupe, zoomed canvas)
 * show logical-resolution detail.
 */
export async function decodeBitmap(backend: Backend, frame: CapturedFrame): Promise<ImageBitmap> {
  const scale = frame.scale > 1 ? frame.scale : 1
  const maxSize = { height: Math.max(1, Math.round(frame.height / scale)), width: Math.max(1, Math.round(frame.width / scale)) }
  return await createImageBitmap(await backend.captureImage(frame, maxSize))
}

/** Executes one script binding call and records it in the event log. */
export async function invokeBinding(backend: Backend | null, method: string, args: WireValue[], context: CallContext): Promise<WireValue> {
  const binding = BINDINGS[method]
  const seq = context.nextSeq()
  const callId = actions.beginCall({ args, effect: binding?.effect ?? 'read', hit: context.hit, line: context.line, method, seq, startedAt: nowMs() })
  const scope = new CallScope(callId, seq, backend)
  try {
    if (!backend)
      throw new Error('No device connected. Connect to an AUV daemon or use the mock desktop.')
    if (!binding)
      throw new Error(`Unknown binding: auv.${method}`)
    const result = await binding.run(backend, scope, args, context)
    actions.endCall(callId, { endSeq: context.nextSeq(), refs: scope.refs, result, status: 'ok' })
    return result
  }
  catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    actions.endCall(callId, { endSeq: context.nextSeq(), error: message, refs: scope.refs, status: 'error' })
    throw new Error(message)
  }
}

function asPoint(value: WireValue): Point {
  const point = value as Partial<Point> | undefined
  if (typeof point?.x !== 'number' || typeof point.y !== 'number')
    throw new TypeError('Expected a point { x, y }')
  return { x: point.x, y: point.y }
}

/**
 * Screen point for `auv.input.click`: a point as is, or the center of an area
 * or rectangle. Before this, a rectangle silently clicked its top-left corner.
 */
function clickPoint(value: WireValue): Point {
  const rect = value as Partial<Rect> | undefined
  if (typeof rect?.width === 'number' && typeof rect.height === 'number' && typeof rect.x === 'number' && typeof rect.y === 'number')
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
  return asPoint(value)
}

/** Logical bounds of the display a display search reads: the given one, else the primary. */
function displayBounds(value: WireValue): Rect {
  const id = displayId(value)
  const { displays } = usePlayground.getState()
  const display = id ? displays.find(candidate => candidate.id === id) : displays.find(candidate => candidate.primary) ?? displays[0]
  if (!display)
    throw new Error(id ? `Unknown display ${id}` : 'No display is known yet')
  return display.frame
}

function displayId(value: WireValue): string | undefined {
  if (typeof value === 'string')
    return value
  const handle = value as Partial<DisplayHandle> | undefined
  return handle?.id
}

function refOf(value: WireValue): string | undefined {
  return typeof value === 'object' && value !== null ? (value as { $ref?: string }).$ref : undefined
}

function resolveFrame(value: WireValue): { frame: CapturedFrame, handle: FrameHandle } {
  const ref = refOf(value)
  const resource = ref ? usePlayground.getState().resources[ref] : undefined
  if (resource?.kind !== 'frame')
    throw new TypeError('Expected a frame handle from capture()')
  return { frame: resource.frame, handle: resource.handle }
}

/** Validated scroll amount: finite numbers, not both zero. */
function scrollDelta(value: WireValue): ScrollDelta {
  const { dx = 0, dy = 0 } = (value ?? {}) as ScrollDelta
  if (typeof dx !== 'number' || typeof dy !== 'number' || !Number.isFinite(dx) || !Number.isFinite(dy))
    throw new TypeError('Expected a scroll amount { dx?, dy? } in logical pixels')
  if (dx === 0 && dy === 0)
    throw new RangeError('scroll: dx and dy cannot both be zero')
  return { dx, dy }
}

/**
 * `scrollUntil` options with defaults applied; a `{ $predicate }` placeholder
 * from the worker becomes a `decide` callback back into the script.
 */
function scrollUntilRequest(value: WireValue, context: CallContext): { decide?: (update: ScrollUntilUpdate) => Promise<boolean>, request: ScrollUntilRequest } {
  const options = (value ?? {}) as ScrollDelta & { confirmations?: number, maxSteps?: number, settle?: number, text?: string, until?: { $predicate?: number } }
  const delta = scrollDelta(options)
  if (delta.dx !== 0 && delta.dy !== 0)
    throw new RangeError('scrollUntil: scroll along one axis only (dx or dy)')
  const predicateId = options.until?.$predicate
  const decide = context.decide
  return {
    decide: predicateId === undefined || !decide ? undefined : update => decide(predicateId, update),
    request: {
      confirmations: options.confirmations ?? SCROLL_UNTIL_DEFAULTS.confirmations,
      delta,
      maxSteps: options.maxSteps ?? SCROLL_UNTIL_DEFAULTS.maxSteps,
      settleMs: options.settle ?? SCROLL_UNTIL_DEFAULTS.settleMs,
      text: options.text || undefined,
    },
  }
}

/**
 * `within` from text-search options: a screen area clipped to `bounds` (the
 * searched image).
 */
function searchArea(options: WireValue, bounds: Rect): undefined | { area: Rect } {
  const within = (options as undefined | { within?: Partial<Rect> })?.within
  if (within === undefined)
    return undefined
  if (typeof within.x !== 'number' || typeof within.y !== 'number' || typeof within.width !== 'number' || typeof within.height !== 'number')
    throw new TypeError('within: expected an area or { x, y, width, height }')
  const x = Math.max(within.x, bounds.x)
  const y = Math.max(within.y, bounds.y)
  const right = Math.min(within.x + within.width, bounds.x + bounds.width)
  const bottom = Math.min(within.y + within.height, bounds.y + bounds.height)
  if (right <= x || bottom <= y)
    throw new RangeError('within: the area does not overlap the searched window, display or frame')
  // The device takes the screen area directly (`screenRegion`); clipping here
  // gives the same area for drawing the search on the canvas.
  return { area: { height: bottom - y, width: right - x, x, y } }
}

/** Screen frame of a window handle as last reported by the device. */
function windowBounds(value: WireValue): Rect {
  const resource = usePlayground.getState().resources[`window:${windowId(value)}`]
  if (resource?.kind !== 'window')
    throw new TypeError('Expected a window handle from auv.windows.resolve()')
  return resource.handle.frame
}

function windowId(value: WireValue): string {
  const ref = refOf(value)
  if (!ref?.startsWith('window:'))
    throw new TypeError('Expected a window handle from auv.windows.resolve()')
  return ref.slice('window:'.length)
}

/**
 * Window-local point for a screen-space `at` (a point, or an area or
 * rectangle's center); scrolling outside the window is rejected.
 */
function windowPoint(window: WireValue, at: WireValue): { point: Point, screen: Point } {
  const screen = clickPoint(at)
  const frame = windowBounds(window)
  const point = { x: screen.x - frame.x, y: screen.y - frame.y }
  if (point.x < 0 || point.y < 0 || point.x > frame.width || point.y > frame.height)
    throw new RangeError(`scroll: (${Math.round(screen.x)}, ${Math.round(screen.y)}) is outside the window`)
  return { point, screen }
}
