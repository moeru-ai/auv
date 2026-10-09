import type { CapturedFrame, DisplayInfo, InputReceipt, RunBackend, TextSearchResult, WindowInfo } from '../backend/types'
import type { DisplayHandle, FrameHandle, InputHandle, ScrollDelta, TextHandle, WindowHandle } from '../handles'
import type { Rect } from '../script-api/api'
import type { Resource } from '../store'

import { deliveryPath, toDisplay, toFrame, toRecognized, toRect, toWindow } from '../backend/auv'
import { decodeThumbHash, revealStart } from '../preview'
import { actions, nowMs, usePlayground } from '../store'

// Turns an SDK call's messages into the playground's resources (frames,
// text, input receipts, windows, displays), so the canvas, inspector and
// timeline show it. Messages are matched by protobuf
// type name, not by RPC: a new RPC that returns a known type is shown with no
// new code. See `docs/ai/references/inspect/2026-10-08-playground-sdk-transport-design.md`.

let nextHandle = 1

/** A decoded protobuf message (`fromBinary` result), read structurally. */
type Message = Record<string, unknown> & { $typeName: string }

type ResourceBody = Resource extends infer R ? R extends unknown ? Omit<R, 'callId' | 'run' | 'seq'> : never : never

/** Tracks the resource refs one SDK call produced. */
export class CallScope {
  readonly refs: string[] = []

  constructor(readonly callId: number, readonly seq: number, readonly backend: null | RunBackend) {}

  display(info: DisplayInfo): DisplayHandle {
    const handle: DisplayHandle = { $ref: `display:${info.id}`, frame: info.frame, id: info.id, kind: 'display', name: info.name, primary: info.primary, scale: info.scale }
    this.#put(handle.$ref, { handle, kind: 'display' })
    return handle
  }

  frame(frame: CapturedFrame): FrameHandle {
    // Keyed by the capture ID, so SDK values holding its `CaptureRef` link to
    // it. A capture seen again in the same run keeps its first producer.
    const ref = `frame:${frame.ref}` as const
    const existing = usePlayground.getState().resources[ref]
    if (existing?.kind === 'frame' && existing.run === usePlayground.getState().runIndex) {
      this.refs.push(ref)
      return existing.handle
    }
    // A replayed recording reuses its capture IDs; free the earlier run's pixels.
    if (existing?.kind === 'frame') {
      existing.bitmap?.close()
      existing.preview?.close()
    }
    const handle: FrameHandle = {
      $ref: ref,
      bounds: frame.bounds,
      height: frame.height,
      kind: 'frame',
      scale: frame.scale,
      source: frame.source,
      width: frame.width,
    }
    this.#put(handle.$ref, { capturedAt: nowMs(), frame, handle, kind: 'frame' })
    const current = () => {
      const resource = usePlayground.getState().resources[handle.$ref]
      return resource?.kind === 'frame' ? resource : undefined
    }
    // The ThumbHash preview decodes locally and shows before the pixels load.
    void decodeThumbHash(frame.thumbhash).then((preview) => {
      const resource = current()
      if (preview && resource && !resource.bitmap)
        actions.putResource(handle.$ref, { ...resource, preview })
      else
        preview?.close()
    })
    if (this.backend) {
      void decodeBitmap(this.backend, frame).then((bitmap) => {
        const resource = current()
        if (resource)
          actions.putResource(handle.$ref, { ...resource, bitmap, revealedAt: revealStart(resource) })
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

  window(info: WindowInfo): WindowHandle {
    const handle: WindowHandle = { $ref: `window:${info.id}`, app: info.app, bundleId: info.bundleId, frame: info.frame, id: info.id, kind: 'window', pid: info.pid, title: info.title }
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

/**
 * Display bitmap for a capture at logical resolution (pixels ÷ `scale`): the
 * canvas and previews draw in logical points, and a Retina capture at full
 * size costs 4× the transfer, memory and a slow downscale per redraw. The
 * backend encodes the bounded image; `createImageBitmap` decodes it off the
 * main thread. OCR keeps reading the full-resolution capture by reference.
 * NOTICE(bitmap-logical-resolution): magnifiers (ClickLoupe, zoomed canvas)
 * show logical-resolution detail.
 */
export async function decodeBitmap(backend: RunBackend, frame: CapturedFrame): Promise<ImageBitmap> {
  const scale = frame.scale > 1 ? frame.scale : 1
  const maxSize = { height: Math.max(1, Math.round(frame.height / scale)), width: Math.max(1, Math.round(frame.width / scale)) }
  return await createImageBitmap(await backend.captureImage(frame, maxSize))
}

const TYPE = {
  capturedFrame: 'auv.api.driver.v1.CapturedFrame',
  display: 'auv.api.driver.v1.Display',
  inputActionResult: 'auv.api.driver.v1.InputActionResult',
  recognizeTextResponse: 'auv.api.driver.v1.RecognizeTextResponse',
  textMatch: 'auv.api.driver.v1.TextMatch',
  window: 'auv.api.driver.v1.Window',
}

interface Context {
  action?: InputHandle['action']
  query?: string
  /** What produced frames in this message, e.g. `window:42`. */
  source: string
}

/**
 * Registers the resources in an RPC's response messages with `scope`.
 * `method` (`/package.Service/Method`) and `request` only label them: which
 * window a frame shows, what a search looked for, what kind of input ran.
 */
export function recordRpcResources(scope: CallScope, method: string, request: Message | undefined, responses: readonly Message[]): void {
  const context: Context = {
    action: inputAction(method, request),
    query: typeof request?.query === 'string' ? request.query : undefined,
    source: sourceOf(request),
  }
  for (const response of responses)
    visit(scope, response, context)
}

/**
 * Kind of input a method delivers. The playground's receipts know five kinds;
 * TODO(playground-sdk-input-kinds): mouse primitives (down, up, drag, move)
 * and held keys record no receipt until the script API names their kinds.
 */
function inputAction(method: string, request: Message | undefined): InputHandle['action'] | undefined {
  const name = method.slice(method.lastIndexOf('/') + 1)
  if (name.startsWith('Click'))
    return 'click'
  if (name.startsWith('Scroll') || name === 'StreamScroll')
    return 'scroll'
  if (name === 'TypeText' || name === 'PasteText')
    return 'type'
  if (name === 'PressKey' || name === 'PressKeys')
    return 'key'
  if (name.startsWith('Activate'))
    return 'activate'
  if (name === 'InputKeyboard') {
    const first = (request?.inputs as Array<{ action?: { case?: string } }> | undefined)?.[0]?.action?.case
    return first === 'typeText' || first === 'pasteText' ? 'type' : 'key'
  }
  return undefined
}

function isMessage(value: unknown, typeName?: string): value is Message {
  return typeof value === 'object' && value !== null && '$typeName' in value && (typeName === undefined || (value as Message).$typeName === typeName)
}

/** Screen point of a pointer action: reported directly, or a window point plus the window's origin. */
function pointerPoint(message: Message, window: Message | undefined): undefined | { x: number, y: number } {
  const screen = message.screenPoint as undefined | { x: number, y: number }
  if (screen)
    return { x: screen.x, y: screen.y }
  const local = message.point as undefined | { x: number, y: number }
  const frame = window?.frame as undefined | { x: number, y: number }
  return local && frame ? { x: frame.x + local.x, y: frame.y + local.y } : undefined
}

/**
 * The window or display a request targets, labelling the frames it returns.
 * A stream's request is its first message, whose `begin` event names it.
 */
function sourceOf(request: Message | undefined): string {
  const event = request?.event as undefined | { case?: string, value?: unknown }
  if (event?.case === 'begin' && isMessage(event.value))
    return sourceOf(event.value)
  const window = request?.window as undefined | { windowId?: string }
  if (window?.windowId)
    return `window:${window.windowId}`
  const selector = request?.selector as undefined | { selector?: { case?: string, value?: unknown } }
  if (selector?.selector?.case === 'display')
    return `display:${(selector.selector.value as { displayId?: string }).displayId}`
  return 'capture'
}

function visit(scope: CallScope, message: Message, outer: Context): void {
  const window = isMessage(message.window, TYPE.window) ? message.window : undefined
  const display = isMessage(message.display, TYPE.display) ? message.display : undefined
  const context: Context = {
    ...outer,
    source: window?.ref ? `window:${(window.ref as { windowId: string }).windowId}` : display ? `display:${display.displayId as string}` : outer.source,
  }
  if (message.$typeName === TYPE.window) {
    scope.window(toWindow(message as never))
    return
  }
  if (message.$typeName === TYPE.display) {
    scope.display(toDisplay(message as never))
    return
  }

  // A capture, and the text recognized in it, belong to the same message.
  const frame = isMessage(message.capture, TYPE.capturedFrame) ? scope.frame(toFrame(message.capture as never, context.source)) : undefined
  const matches = Array.isArray(message.matches) && message.matches.every(match => isMessage(match, TYPE.textMatch)) ? message.matches as Message[] : undefined
  if (matches)
    scope.text({ matches: matches.map(match => ({ bounds: toRect(match.bounds as never)!, confidence: match.confidence as number, text: match.text as string })) }, context.query, frame)
  if (isMessage(message.text, TYPE.recognizeTextResponse))
    scope.text(toRecognized(message.text as never), context.query, frame)
  if (message.$typeName === TYPE.recognizeTextResponse)
    scope.text(toRecognized(message as never), context.query)

  const action = isMessage(message.action, TYPE.inputActionResult)
    ? message.action
    : Array.isArray(message.actions) && isMessage(message.actions[0], TYPE.inputActionResult) ? message.actions[0] as Message : undefined
  if (action && context.action) {
    const scroll = message.scroll as undefined | { deltaX: number, deltaY: number }
    const delta: ScrollDelta | undefined = scroll ? { dx: scroll.deltaX, dy: scroll.deltaY } : undefined
    scope.input(context.action, { path: deliveryPath(action as never), point: pointerPoint(message, window) }, delta)
  }

  // Nested messages (oneof events, repeated results) carry their own resources.
  for (const [field, value] of Object.entries(message)) {
    if (field === 'capture' || field === 'text' || field === 'matches' || field === 'action' || field === 'actions')
      continue
    for (const nested of Array.isArray(value) ? value : [value]) {
      if (isMessage(nested))
        visit(scope, nested, context)
      else if (typeof nested === 'object' && nested !== null && 'case' in nested && isMessage((nested as { value?: unknown }).value))
        visit(scope, (nested as { value: Message }).value, context)
    }
  }
}
