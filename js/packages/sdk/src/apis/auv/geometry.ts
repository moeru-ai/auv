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
