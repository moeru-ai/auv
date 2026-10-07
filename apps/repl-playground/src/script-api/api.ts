// Script-facing AUV API.
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
  readonly center: Point
  /** Whether a point or rectangle lies fully inside. */
  contains: (target: Point | Rect) => boolean
  /** `$ref` of the handle the area was first derived from, e.g. `window:42`. */
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

/** Anything with screen-space bounds an area can start from. */
export type AreaSource = DisplayHandle | FrameHandle | Rect | TextMatch | WindowHandle

export interface AuvScriptApi {
  apps: {
    /**
     * Brings an application to the foreground by bundle ID (macOS). The
     * receipt's `path` reports whether AUV verified the app is frontmost.
     */
    activate: (bundleId: string) => Promise<InputHandle>
  }
  displays: {
    /** Captures a display; the primary display when omitted. */
    capture: (display?: DisplayHandle | string) => Promise<FrameHandle>
    /** Finds text on a display with OCR. */
    findText: (query: string, display?: DisplayHandle | string, options?: TextSearchOptions) => Promise<TextHandle>
    list: () => Promise<DisplayHandle[]>
  }
  input: {
    /** Clicks a logical screen point, or the center of an area or rectangle. */
    click: (target: Point | Rect, options?: ClickOptions) => Promise<InputHandle>
    /** Presses a key or chord, e.g. `Return` or `cmd+a`. */
    pressKey: (key: string) => Promise<InputHandle>
    typeText: (text: string) => Promise<InputHandle>
  }
  text: {
    /** Runs OCR over a previously captured frame. */
    recognize: (frame: FrameHandle, options?: TextSearchOptions) => Promise<TextHandle>
  }
  windows: {
    list: () => Promise<WindowHandle[]>
    resolve: (selector: WindowSelector) => Promise<WindowHandle>
  }
}

/** Center point of a rectangle. */
export type CenterOf = (rect: Rect) => Point

export interface ClickOptions {
  button?: 'left' | 'middle' | 'right'
  /** Clicks in a row: 2 is a double click. */
  count?: number
  /** Milliseconds between clicks when `count` > 1; defaults to 80. */
  interval?: number
}

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

export interface FocusOptions {
  /**
   * Zoom out mid-way on long jumps and descend onto the target (a smooth
   * zoom-and-pan arc). `false` moves straight. Default `true`.
   */
  autoZoomOut?: boolean
  /** Zoom so the target fills the view; `false` only pans. Default `true`. */
  zoom?: boolean
}

/** A captured image. Pixels stay on the host; scripts hold only this handle. */
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

/** A point in logical screen coordinates (points, not pixels). */
export interface Point {
  x: number
  y: number
}

/** Anything with a screen position the canvas camera can move to. */
export type Positional = AreaSource | InputHandle | Point | TextHandle

/** A rectangle in logical screen coordinates. */
export interface Rect {
  height: number
  width: number
  x: number
  y: number
}

/**
 * Scroll amount in logical pixels: positive `dy` scrolls toward later content
 * (down), positive `dx` scrolls right, like DOM `WheelEvent`.
 */
export interface ScrollDelta {
  dx?: number
  dy?: number
}

/** What `scrollUntil` saw after one step, for an `until` predicate. */
export interface ScrollUntilUpdate {
  /** Whether the viewport moved on this step (false before the first step). */
  readonly moved: boolean
  readonly steps: number
  /** Full recognized window text after this step. */
  readonly text: string
}

/** One step per update; scroll along one axis only. */
export interface ScrollUntilOptions extends ScrollDelta {
  /** Consecutive no-motion steps that count as the end, 1–10. Default 2. */
  confirmations?: number
  /** Step budget, 1–1000. Default 50. */
  maxSteps?: number
  /** Milliseconds to wait after each step before observing, ≤ 10000. Default 400. */
  settle?: number
  /** Stop when recognized window text contains this (case-insensitive). */
  text?: string
  /** Stop when this returns `true` for an update. */
  until?: (update: ScrollUntilUpdate) => boolean | Promise<boolean>
}

export interface ScrollUntilResult {
  /** The last update's capture, unless nothing was observed. */
  readonly frame?: FrameHandle
  /** Delivery receipt of the first step; absent if the loop stopped before scrolling. */
  readonly input?: InputHandle
  /** Where `text` was found, in screen space. */
  readonly match?: TextMatch
  readonly reason: 'budget' | 'end' | 'text-visible' | 'until'
  readonly steps: number
  /** The last update's OCR result. */
  readonly text?: TextHandle
}

/** Result of OCR over a frame, a window or a display. */
export interface TextHandle {
  readonly $ref: `text:${string}`
  /** The frame the recognizer looked at, when the driver returned it. */
  readonly frame?: FrameHandle
  readonly kind: 'text'
  readonly matches: readonly TextMatch[]
  readonly query?: string
  /** Full recognized text, for `auv.text.recognize`. */
  readonly text?: string
  /** Screen-space area the recognizer was limited to, when `within` was given. */
  readonly within?: Rect
}

export interface TextMatch {
  /** Logical screen-space bounds of the matched text. */
  readonly bounds: Rect
  readonly confidence: number
  readonly text: string
}

export interface TextSearchOptions {
  /**
   * Only read text inside this screen-space area (e.g. an `area()`); it is
   * clipped to the searched window, display or frame.
   */
  within?: Rect
}

/** A resolved top-level window. */
export interface WindowHandle {
  readonly $ref: `window:${string}`
  readonly app?: string
  readonly bundleId?: string
  /** Captures the window's pixels. */
  capture: () => Promise<FrameHandle>
  /** Clicks a window-relative point (origin at the window's top-left). */
  click: (point: Point, options?: ClickOptions) => Promise<InputHandle>
  /** Finds text in the window with OCR; match bounds are screen-space. */
  findText: (query: string, options?: TextSearchOptions) => Promise<TextHandle>
  /** Logical screen-space frame of the window. */
  readonly frame: Rect
  readonly id: string
  readonly kind: 'window'
  readonly pid?: number
  /**
   * Wheel-scrolls once at `at`: a screen-space point, or the center of an area
   * or rectangle (unlike `click`, which takes a window-relative point). The
   * receipt is delivery evidence only; it does not prove the content moved.
   */
  scroll: (at: Point | Rect, delta: ScrollDelta) => Promise<InputHandle>
  /**
   * Scrolls at `at` in steps, observing (capture + OCR) after each step, until
   * `text` appears, `until` returns `true`, no visual motion remains, or
   * `maxSteps` runs out. An `end` stop is not proof that no content is left.
   */
  scrollUntil: (at: Point | Rect, options: ScrollUntilOptions) => Promise<ScrollUntilResult>
  // TODO(repl-scroll-motion): timed (`scrollMotion`) and live velocity
  // (`scrollStream` / `scrollWith`) scrolling exist in `@auv-js/sdk` but are not
  // exposed; add them when a script needs eased or continuous scrolling.
  readonly title?: string
}

export interface WindowSelector {
  appName?: string
  bundleId?: string
  /** Select the frontmost application. */
  frontmost?: boolean
  pid?: number
  /** Exact window title. */
  title?: string
  /** Window title substring. */
  titleContains?: string
}

declare global {
  const auv: AuvScriptApi
  /** Starts an area from a window, display, frame, OCR match or rectangle. */
  function area(source: AreaSource, label?: string): Area
  /** Center point of a screen-space rectangle. */
  const centerOf: CenterOf
  /**
   * Editor only: draws an area, rectangle or point on the desktop canvas at
   * this moment of the run. Nothing is sent to the device. Returns `target`.
   */
  function draw<T extends Point | Rect>(target: T, label?: string): T
  /**
   * Editor only: eases the desktop canvas camera onto a window, area, OCR
   * result, match, frame, click or point at this moment of the run. Nothing
   * is sent to the device and the script does not wait. Returns `target`.
   */
  function focus<T extends Positional>(target: T, options?: FocusOptions): T
  /** Records a value in the inspector without printing it. */
  function show<T>(value: T, label?: string): T
  function sleep(ms: number): Promise<void>
}
