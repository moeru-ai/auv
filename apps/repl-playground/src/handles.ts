import type { Point, Rect } from './script-api/api'
import type { Resource } from './store'

// What the playground records for the canvas, inspector and timeline, from
// SDK responses (`runtime/rpc-resources.ts`). Scripts never hold these: they
// hold SDK values, which `refOf` maps back to them.

/** A display (monitor) of the controlled Device. */
export interface DisplayHandle {
  readonly $ref: `display:${string}`
  /** Logical bounds of the display on the virtual desktop. */
  readonly frame: Rect
  readonly id: string
  readonly kind: 'display'
  readonly name?: string
  readonly primary: boolean
  /** Physical pixels per logical point. */
  readonly scale: number
}

/** A captured image; its pixels load on the host by capture reference. */
export interface FrameHandle {
  readonly $ref: `frame:${string}`
  /** Logical screen-space bounds represented by the pixels. */
  readonly bounds: Rect
  /** Physical pixel height. */
  readonly height: number
  readonly kind: 'frame'
  readonly scale: number
  /** What produced the frame, e.g. `display:1` or `window:42`. */
  readonly source: string
  /** Physical pixel width. */
  readonly width: number
}

/** Receipt of a delivered input action. Delivery is not semantic success. */
export interface InputHandle {
  readonly $ref: `input:${string}`
  readonly action: 'activate' | 'click' | 'key' | 'scroll' | 'type'
  /** Scroll amount for `scroll` receipts, in logical pixels. */
  readonly delta?: ScrollDelta
  readonly kind: 'input'
  /** Delivery path chosen by the driver, e.g. `window-targeted`. */
  readonly path?: string
  /** Screen point for pointer actions. */
  readonly point?: Point
}

/**
 * Scroll amount in logical pixels: positive `dy` scrolls toward later content
 * (down), positive `dx` scrolls right, like DOM `WheelEvent`.
 */
export interface ScrollDelta {
  dx?: number
  dy?: number
}

/** Result of OCR over a capture, a window or a display. */
export interface TextHandle {
  readonly $ref: `text:${string}`
  /** The frame the recognizer looked at, when the driver returned it. */
  readonly frame?: FrameHandle
  readonly kind: 'text'
  readonly matches: readonly TextMatch[]
  readonly query?: string
  /** Full recognized text, for `recognizeText`. */
  readonly text?: string
  /** Screen-space area the recognizer was limited to. */
  readonly within?: Rect
}

export interface TextMatch {
  /** Logical screen-space bounds of the matched text. */
  readonly bounds: Rect
  readonly confidence: number
  readonly text: string
}

/** A top-level window as AUV last reported it. */
export interface WindowHandle {
  readonly $ref: `window:${string}`
  readonly app?: string
  readonly bundleId?: string
  /** Logical screen-space frame of the window. */
  readonly frame: Rect
  readonly id: string
  readonly kind: 'window'
  readonly pid?: number
  readonly title?: string
}

// Resources are keyed by the IDs AUV gives them: `window:<windowId>`,
// `display:<displayId>` and `frame:<captureId>`. SDK messages carry the same
// IDs (`WindowRef`, `CaptureRef`, a `Position`'s window), so lineage, hover and
// pins work on plain SDK values. Script handles still carry `$ref` until the
// `auv` global becomes the SDK client.
// NOTICE(lineage-without-ids): text results and input receipts have no Runner
// ID; a value holding one links to its producing call only, not to the
// resource.

const ID_PREFIX: Record<string, string> = { captureId: 'frame', displayId: 'display', windowId: 'window' }

/** The protobuf messages that stand for one resource, and that resource's key. */
const DENOTED: Record<string, (message: Record<string, any>) => string | undefined> = {
  'auv.api.driver.v1.CapturedFrame': message => idRef('captureId', message.ref?.captureId),
  'auv.api.driver.v1.CaptureRef': message => idRef('captureId', message.captureId),
  'auv.api.driver.v1.Display': message => idRef('displayId', message.displayId),
  'auv.api.driver.v1.Window': message => idRef('windowId', message.ref?.windowId),
  'auv.api.driver.v1.WindowRef': message => idRef('windowId', message.windowId),
}

/** Every resource key a value mentions at any depth: script handles and SDK IDs. */
export function mentionedRefs(value: unknown, into = new Set<string>(), depth = 0): Set<string> {
  if (depth > 8 || value === null || typeof value !== 'object')
    return into
  const object = value as Record<string, unknown>
  if (typeof object.$ref === 'string')
    into.add(object.$ref)
  // A protobuf-es oneof such as `Position.coordinateSpace`: `{ case: 'windowId', value }`.
  const oneof = typeof object.case === 'string' ? idRef(object.case, object.value) : undefined
  if (oneof)
    into.add(oneof)
  for (const [key, item] of Object.entries(object)) {
    const ref = idRef(key, item)
    if (ref)
      into.add(ref)
    else
      mentionedRefs(item, into, depth + 1)
  }
  return into
}

/** Short human hint for a handle, so a list of refs is readable without hovering. */
export function refHint(resource: Resource | undefined): string | undefined {
  switch (resource?.kind) {
    case 'display':
      return resource.handle.name
    case 'frame':
      return `${resource.handle.width}×${resource.handle.height}`
    case 'input':
      return resource.handle.action
    case 'text':
      if (resource.handle.matches.length === 0)
        return 'no match'
      return `${resource.handle.matches.length} match${resource.handle.matches.length === 1 ? '' : 'es'}`
    case 'window':
      return resource.handle.app || resource.handle.title || undefined
  }
  return undefined
}

/**
 * The resource a value stands for, if it is one: a script handle, a window,
 * display or captured frame message, a bare `WindowRef` / `CaptureRef`
 * (protobuf or ProtoJSON), or an SDK `WindowClient`.
 */
export function refOf(value: unknown): string | undefined {
  if (value === null || typeof value !== 'object')
    return undefined
  const object = value as Record<string, any>
  if (typeof object.$ref === 'string')
    return object.$ref
  const typeName = typeof object.$typeName === 'string' ? object.$typeName : undefined
  if (typeName)
    return DENOTED[typeName]?.(object)
  // A `WindowClient` crosses from the worker as `{ id, window, …methods }`.
  if (typeof object.id === 'string' && object.window?.$typeName === 'auv.api.driver.v1.Window')
    return idRef('windowId', object.id)
  const keys = Object.keys(object)
  return keys.length === 1 && (keys[0] === 'windowId' || keys[0] === 'captureId') ? idRef(keys[0], object[keys[0]]) : undefined
}

// NOTICE(focus-point-neighbourhood): a point has no size; focusing one shows
// this much of the desktop around it (logical points).
const POINT_VIEW = { height: 150, width: 240 }

/**
 * Screen-space rectangle of anything positional a script passes to
 * `focus()`: handles by ref (windows, displays, frames, OCR results, clicks),
 * areas and rectangles, OCR matches (`bounds`), or points.
 */
export function boundsOf(value: unknown, resources: Record<string, Resource>): Rect | undefined {
  const object = value as null | Record<string, unknown> | undefined
  if (!object || typeof object !== 'object')
    return undefined
  const ref = refOf(object)
  const resource = ref ? resources[ref] : undefined
  switch (resource?.kind) {
    case 'display':
    case 'window':
      return resource.handle.frame
    case 'frame':
      return resource.handle.bounds
    case 'input':
      return resource.handle.point ? around(resource.handle.point) : undefined
    case 'text': {
      const rects = resource.handle.matches.map(match => match.bounds)
      if (rects.length > 0)
        return union(rects)
      return resource.handle.within ?? resource.handle.frame?.bounds
    }
  }
  if (isRect(object))
    return { height: object.height, width: object.width, x: object.x, y: object.y }
  if (isRect(object.bounds))
    return object.bounds
  if (typeof object.x === 'number' && typeof object.y === 'number')
    return around({ x: object.x, y: object.y })
  return undefined
}

function around(point: Point): Rect {
  return { ...POINT_VIEW, x: point.x - POINT_VIEW.width / 2, y: point.y - POINT_VIEW.height / 2 }
}

function idRef(key: string, id: unknown): string | undefined {
  const prefix = ID_PREFIX[key]
  return prefix && typeof id === 'string' && id.length > 0 ? `${prefix}:${id}` : undefined
}

function isRect(value: unknown): value is Rect {
  const rect = value as null | Partial<Rect> | undefined
  return typeof rect?.x === 'number' && typeof rect.y === 'number' && typeof rect.width === 'number' && typeof rect.height === 'number'
}

function union(rects: Rect[]): Rect {
  const x = Math.min(...rects.map(rect => rect.x))
  const y = Math.min(...rects.map(rect => rect.y))
  const right = Math.max(...rects.map(rect => rect.x + rect.width))
  const bottom = Math.max(...rects.map(rect => rect.y + rect.height))
  return { height: bottom - y, width: right - x, x, y }
}
