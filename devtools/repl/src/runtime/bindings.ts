import type { Backend, CapturedFrame, DisplayInfo, InputReceipt, NormalizedRect, TextSearchResult, WindowInfo } from '../backend/types'
import type { ClickOptions, DisplayHandle, FrameHandle, InputHandle, Point, Rect, TextHandle, WindowSelector } from '../script-api/api'
import type { Effect, Resource, WindowData } from '../store'
import type { WireValue } from './protocol'

import { actions, nowMs, usePlayground } from '../store'

let nextHandle = 1

// NOTICE(raw-frame-budget): raw RGBA is only needed to send a frame back to
// OCR (`auv.text.recognize`). A Retina window capture is ~25 MB and a display
// ~80 MB, so only the newest ~256 MB stay raw; older frames keep a display
// bitmap. The last live run's replay recording still holds its own frames.
const RAW_FRAME_BUDGET = 256 * 1024 * 1024

// TODO(auv-resource-handles): resources (including full RGBA frames) are held
// in the browser because AUV returns pixels inline. Switch to daemon-side
// handles plus a resource fetch API once AUV exposes one.

interface Binding {
  effect: Effect
  run: (backend: Backend, scope: CallScope, args: WireValue[]) => Promise<WireValue>
}

type ResourceBody = Resource extends infer R ? R extends unknown ? Omit<R, 'callId' | 'run' | 'seq'> : never : never

/** Tracks the resource refs one binding call produced. */
class CallScope {
  readonly refs: string[] = []

  constructor(readonly callId: number, readonly seq: number) {}

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
    actions.releaseRawFrames(RAW_FRAME_BUDGET)
    void decodeBitmap(frame).then((bitmap) => {
      const current = usePlayground.getState().resources[handle.$ref]
      if (current?.kind === 'frame')
        actions.putResource(handle.$ref, { ...current, bitmap })
      else
        bitmap.close()
    })
    return handle
  }

  input(action: InputHandle['action'], receipt: InputReceipt): InputHandle {
    const handle: InputHandle = { $ref: `input:${nextHandle++}`, action, kind: 'input', path: receipt.path, point: receipt.point }
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
    return scope.text(await backend.findDisplayText(String(query), displayId(display), within?.region), String(query), undefined, within?.area)
  }),
  'displays.list': read(async (backend, scope) => (await backend.listDisplays()).map(info => scope.display(info))),
  'input.click': input(async (backend, scope, [target, options]) => scope.input('click', await backend.clickScreen(clickPoint(target), options as ClickOptions | undefined))),
  'input.pressKey': input(async (backend, scope, [key]) => scope.input('key', await backend.pressKey(String(key)))),
  'input.typeText': input(async (backend, scope, [text]) => scope.input('type', await backend.typeText(String(text)))),
  'text.recognize': read(async (backend, scope, [frame, options]) => {
    const source = resolveFrame(frame)
    const within = searchArea(options, source.handle.bounds)
    return scope.text(await backend.recognizeText(source.frame, within?.region), undefined, source.handle, within?.area)
  }),
  'windows.capture': read(async (backend, scope, [window]) => scope.frame(await backend.captureWindow(windowId(window)))),
  'windows.click': input(async (backend, scope, [window, point, options]) => scope.input('click', await backend.clickWindow(windowId(window), asPoint(point), options as ClickOptions | undefined))),
  'windows.findText': read(async (backend, scope, [window, query, options]) => {
    const within = searchArea(options, windowBounds(window))
    return scope.text(await backend.findWindowText(windowId(window), String(query), within?.region), String(query), undefined, within?.area)
  }),
  'windows.list': read(async (backend, scope) => (await backend.listWindows()).map(info => scope.window(info))),
  'windows.resolve': read(async (backend, scope, [selector]) => scope.window(await backend.resolveWindow((selector ?? {}) as WindowSelector))),
}

export interface CallContext {
  /** 1-based hit count of the calling line (loop iteration). */
  hit: number
  line: null | number
  /** Allocates the next event-log position. */
  nextSeq: () => number
}

/**
 * Display bitmap for a capture at logical resolution (pixels ÷ `scale`): the
 * canvas and previews draw in logical points, and a Retina capture decoded at
 * full size costs 4× the memory and a slow high-quality downscale per redraw.
 * NOTICE(bitmap-logical-resolution): magnifiers (ClickLoupe, zoomed canvas)
 * show logical-resolution detail; the raw frame stays available for OCR.
 */
export async function decodeBitmap(frame: CapturedFrame): Promise<ImageBitmap> {
  // NOTICE(rgba-copy): ImageData needs a Uint8ClampedArray over a non-shared
  // ArrayBuffer whose length is exactly width*height*4; copy when the view is
  // offset into a larger protobuf buffer.
  const exact = frame.rgba.byteOffset === 0 && frame.rgba.byteLength === frame.rgba.buffer.byteLength
  const bytes = exact ? frame.rgba : frame.rgba.slice()
  const pixels = new Uint8ClampedArray(bytes.buffer as ArrayBuffer, bytes.byteOffset, frame.width * frame.height * 4)
  const scale = frame.scale > 1 ? frame.scale : 1
  return await createImageBitmap(new ImageData(pixels, frame.width, frame.height), {
    resizeHeight: Math.max(1, Math.round(frame.height / scale)),
    resizeQuality: 'high',
    resizeWidth: Math.max(1, Math.round(frame.width / scale)),
  })
}

/** Executes one script binding call and records it in the event log. */
export async function invokeBinding(backend: Backend | null, method: string, args: WireValue[], context: CallContext): Promise<WireValue> {
  const binding = BINDINGS[method]
  const seq = context.nextSeq()
  const callId = actions.beginCall({ args, effect: binding?.effect ?? 'read', hit: context.hit, line: context.line, method, seq, startedAt: nowMs() })
  const scope = new CallScope(callId, seq)
  try {
    if (!backend)
      throw new Error('No device connected. Connect to an AUV daemon or use the mock desktop.')
    if (!binding)
      throw new Error(`Unknown binding: auv.${method}`)
    const result = await binding.run(backend, scope, args)
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
  if (resource.released)
    throw new RangeError(`${resource.handle.$ref} is an older capture whose pixels were released to save memory; capture again`)
  return { frame: resource.frame, handle: resource.handle }
}

/**
 * `within` from text-search options, clipped to `bounds` (the searched image)
 * and expressed as the fractions AUV expects (`NormalizedRect`).
 */
function searchArea(options: WireValue, bounds: Rect): undefined | { area: Rect, region: NormalizedRect } {
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
  const area = { height: bottom - y, width: right - x, x, y }
  return {
    area,
    region: { height: area.height / bounds.height, width: area.width / bounds.width, x: (x - bounds.x) / bounds.width, y: (y - bounds.y) / bounds.height },
  }
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
