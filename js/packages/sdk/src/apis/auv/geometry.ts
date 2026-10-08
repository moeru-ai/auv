import type { MessageInitShape } from '@bufbuild/protobuf'

import type { PositionSchema } from '../../gen/auv/api/driver/v1/geometry_pb'
import type { WindowTarget } from './driver'

// Small pure helpers for the geometry AUV returns. They take any object with
// the right fields, so `ScreenRect`, `RelativeRect`, a window `frame` or a
// text match's `bounds` work as is. Rectangles are logical screen space unless
// the caller's data says otherwise; the helpers never convert spaces.

/** A point in any space. */
export interface PointLike {
  x: number
  y: number
}

/** A rectangle in any space, its origin at the top-left. */
export interface RectLike {
  height: number
  width: number
  x: number
  y: number
}

/**
 * Builds the `Position` that input calls take: a point plus the space it is
 * in. A window position names the window, so a click there is delivered to
 * that window; screen and display positions are global.
 */
export const Position = {
  /** A point relative to a display's top-left; pass the display or its ID. */
  display(display: string | { displayId: string }, x: number, y: number): MessageInitShape<typeof PositionSchema> {
    const displayId = typeof display === 'string' ? display : display.displayId
    return { coordinateSpace: { case: 'displayId', value: displayId }, x, y }
  },

  /** A point in logical screen coordinates. */
  screen(x: number, y: number): MessageInitShape<typeof PositionSchema> {
    return { coordinateSpace: { case: 'screen', value: true }, x, y }
  },

  /** A point relative to a window's top-left; pass any window target. */
  window(window: WindowTarget, x: number, y: number): MessageInitShape<typeof PositionSchema> {
    return { coordinateSpace: { case: 'windowId', value: windowIdOf(window) }, x, y }
  },
}

/**
 * Where a `region()` box sits: distances from the parent's edges, and sizes.
 * Per axis give two of start, end and size; see `region`.
 */
export interface Edges {
  bottom?: Length
  height?: Length
  left?: Length
  right?: Length
  top?: Length
  width?: Length
}

/** Per-side amounts for `inset`. */
export interface Insets {
  bottom?: number
  left?: number
  right?: number
  top?: number
}

/** Logical points, or a percentage of the parent rectangle such as `'50%'`. */
export type Length = `${number}%` | number

/** The strip of `height` above `rect`, `gap` away. */
export function above(rect: RectLike, height: number, gap = 0): RectLike {
  return { height, width: rect.width, x: rect.x, y: rect.y - gap - height }
}

/** The point at a fraction of `rect`: `at(rect, 0, 0)` is the top-left, `at(rect, 0.5, 0.5)` the center. */
export function at(rect: RectLike, fx: number, fy: number): PointLike {
  return { x: rect.x + rect.width * fx, y: rect.y + rect.height * fy }
}

/** The strip of `height` below `rect`, `gap` away. */
export function below(rect: RectLike, height: number, gap = 0): RectLike {
  return { height, width: rect.width, x: rect.x, y: rect.y + rect.height + gap }
}

// The rectangle helpers below match Rust's `Rect` methods
// (`below(rect, 40)` here is `rect.below(40.0, 0.0)` there). Each returns a
// new rectangle in the same space.

/** The center of a rectangle. */
export function center(rect: RectLike): PointLike {
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

/** Whether a point, or a whole rectangle, lies inside `rect`; edges count as inside. */
export function contains(rect: RectLike, target: PointLike | RectLike): boolean {
  const width = 'width' in target ? target.width : 0
  const height = 'height' in target ? target.height : 0
  return target.x >= rect.x && target.y >= rect.y
    && target.x + width <= rect.x + rect.width && target.y + height <= rect.y + rect.height
}

/** Shrinks `rect` by the same amount on every side, or per side. */
export function inset(rect: RectLike, by: Insets | number): RectLike {
  const { bottom = 0, left = 0, right = 0, top = 0 } = typeof by === 'number' ? { bottom: by, left: by, right: by, top: by } : by
  return { height: rect.height - top - bottom, width: rect.width - left - right, x: rect.x + left, y: rect.y + top }
}

/**
 * The overlap of two rectangles, or `undefined` when they do not overlap
 * (touching edges do not). Clips a rectangle to a window or display frame.
 */
export function intersect(a: RectLike, b: RectLike): RectLike | undefined {
  const x = Math.max(a.x, b.x)
  const y = Math.max(a.y, b.y)
  const width = Math.min(a.x + a.width, b.x + b.width) - x
  const height = Math.min(a.y + a.height, b.y + b.height) - y
  return width > 0 && height > 0 ? { height, width, x, y } : undefined
}

/** The strip of `width` left of `rect`, `gap` away. */
export function leftOf(rect: RectLike, width: number, gap = 0): RectLike {
  return { height: rect.height, width, x: rect.x - gap - width, y: rect.y }
}

export function offset(rect: RectLike, dx: number, dy: number): RectLike {
  return { height: rect.height, width: rect.width, x: rect.x + dx, y: rect.y + dy }
}

/**
 * A box inside `rect` by distances from its edges. Per axis give two of
 * start, end and size (`left`/`right`/`width`, `top`/`bottom`/`height`): a
 * missing start is 0 and a missing size fills the rest. Percent strings are
 * of `rect`, so `left: '25%'` follows window resizes. All three on one axis
 * throws.
 */
export function region(rect: RectLike, edges: Edges): RectLike {
  const [x, width] = regionAxis('left, right and width', edges.left, edges.right, edges.width, rect.width)
  const [y, height] = regionAxis('top, bottom and height', edges.top, edges.bottom, edges.height, rect.height)
  return { height, width, x: rect.x + x, y: rect.y + y }
}

/** The strip of `width` right of `rect`, `gap` away. */
export function rightOf(rect: RectLike, width: number, gap = 0): RectLike {
  return { height: rect.height, width, x: rect.x + rect.width + gap, y: rect.y }
}

function lengthIn(value: Length | undefined, total: number): number | undefined {
  if (value === undefined)
    return undefined
  if (typeof value === 'number')
    return value
  const percent = Number.parseFloat(value)
  if (!value.endsWith('%') || Number.isNaN(percent))
    throw new TypeError(`Expected a number or a percentage such as '25%', got ${JSON.stringify(value)}`)
  return total * percent / 100
}

/** One axis of `region` as `[offset, size]` inside `total`. */
function regionAxis(names: string, startValue: Length | undefined, endValue: Length | undefined, sizeValue: Length | undefined, total: number): [number, number] {
  if (startValue !== undefined && endValue !== undefined && sizeValue !== undefined)
    throw new RangeError(`region(): ${names} over-constrain the box; give two of them`)
  const start = lengthIn(startValue, total)
  const end = lengthIn(endValue, total)
  const size = lengthIn(sizeValue, total)
  if (size !== undefined)
    return [start ?? (end === undefined ? 0 : total - end - size), size]
  const from = start ?? 0
  return [from, total - from - (end ?? 0)]
}

/** The window ID of any window target: an ID, a `WindowRef`, a `Window` or a client. */
function windowIdOf(window: WindowTarget): string {
  const id = typeof window === 'string'
    ? window
    : 'id' in window
      ? window.id
      : 'windowId' in window
        ? window.windowId
        : window.ref?.windowId
  if (!id)
    throw new TypeError('Position.window() needs a window with a window ID')
  return id
}
