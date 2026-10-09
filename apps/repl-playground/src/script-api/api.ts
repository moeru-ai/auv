// Script-facing playground API: the globals cells get beside `auv` (the
// `@auv-js/sdk` Runner client) and `sdk` (the module).
//
// NOTICE(script-api-single-source): this file is both imported by the
// execution worker and loaded as raw text into the editor's TypeScript
// language service, so it must contain types only (no runtime code) and
// must not import anything.

/**
 * A rectangle you define relative to something on screen, in logical screen
 * coordinates. Areas are plain values computed in the script: creating one
 * never talks to the device. Every method returns a new area.
 */
export interface Area extends Rect {
  /** The strip of `height` above this area, `gap` away. */
  above: (height: number, gap?: number) => Area
  /** Point at a fractional position: `at(0, 0)` is the top-left, `at(0.5, 0.5)` the center. */
  at: (fx: number, fy: number) => Point
  /** The strip of `height` below this area, `gap` away. */
  below: (height: number, gap?: number) => Area
  /** The area itself as screen bounds, so SDK clicks and scrolls take it at its center. */
  readonly bounds: Rect
  readonly center: Point
  /** Whether a point or rectangle lies fully inside. */
  contains: (target: Point | Rect) => boolean
  /** The resource the area was first derived from, e.g. `window:42`. */
  readonly from?: string
  /** Shrinks by the same amount on every side, or per side. */
  inset: (by: number | Partial<Record<'bottom' | 'left' | 'right' | 'top', number>>) => Area
  readonly kind: 'area'
  readonly label?: string
  /** The strip of `width` left of this area, `gap` away. */
  leftOf: (width: number, gap?: number) => Area
  /** Same area with a label for the canvas, console and hover cards. */
  named: (label: string) => Area
  offset: (dx: number, dy: number) => Area
  /**
   * A box inside this area by distances from its edges. Per axis give two
   * of `left`/`right`/`width` (`top`/`bottom`/`height`); a missing start
   * edge is 0, a missing size fills the rest. Percent strings are relative
   * to this area, so `left: '25%'` follows window resizes.
   */
  region: (edges: AreaEdges) => Area
  /** The strip of `width` right of this area, `gap` away. */
  rightOf: (width: number, gap?: number) => Area
}

export interface AreaEdges {
  bottom?: AreaLength
  height?: AreaLength
  left?: AreaLength
  right?: AreaLength
  top?: AreaLength
  width?: AreaLength
}

/** Logical points, or a percentage of the parent area such as `'50%'`. */
export type AreaLength = `${number}%` | number

/**
 * Anything with screen-space bounds an area can start from: a rectangle, an
 * SDK `Window` or `Display` (`frame`), a `WindowClient` (`window.frame`), a
 * text match or capture (`bounds`), or another area.
 */
export type AreaSource = Rect | { bounds: Rect } | { frame: Rect } | { window: { frame?: Rect } }

/** Center point of a rectangle. */
export type CenterOf = (rect: Rect) => Point

export interface FocusOptions {
  /**
   * Zoom out mid-way on long jumps and descend onto the target (a smooth
   * zoom-and-pan arc). `false` moves straight. Default `true`.
   */
  autoZoomOut?: boolean
  /** Zoom so the target fills the view; `false` only pans. Default `true`. */
  zoom?: boolean
}

/** A point in logical screen coordinates (points, not pixels). */
export interface Point {
  x: number
  y: number
}

/** Anything with a screen position the canvas camera can move to. */
export type Positional = AreaSource | Point

/** A rectangle in logical screen coordinates. */
export interface Rect {
  height: number
  width: number
  x: number
  y: number
}

declare global {
  /** Starts an area from a window, display, capture, text match or rectangle. */
  function area(source: AreaSource, label?: string): Area
  /** Center point of a screen-space rectangle. */
  const centerOf: CenterOf
  /**
   * Editor only: draws an area, rectangle or point on the desktop canvas at
   * this moment of the run. Nothing is sent to the device. Returns `target`.
   */
  function draw<T extends Point | Rect>(target: T, label?: string): T
  /**
   * Editor only: eases the desktop canvas camera onto a window, area, text
   * match, capture or point at this moment of the run. Nothing
   * is sent to the device and the script does not wait. Returns `target`.
   */
  function focus<T extends Positional>(target: T, options?: FocusOptions): T
  /** Records a value in the inspector without printing it. */
  function show<T>(value: T, label?: string): T
  function sleep(ms: number): Promise<void>
}
