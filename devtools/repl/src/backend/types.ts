import type { ClickOptions, Point, Rect, TextMatch, WindowSelector } from '../script-api/api'

/**
 * One accessibility element. `path` is a stable-within-a-snapshot address
 * like `0/2/1`; `frame` is in logical screen coordinates.
 */
export interface AxNode {
  actions?: string[]
  children: AxNode[]
  frame?: Rect
  label?: string
  path: string
  role: string
  value?: string
}

/**
 * Host-side device capabilities used by script bindings. Implementations:
 * the real AUV daemon through `@auv-js/sdk`, and an in-browser mock desktop.
 */
export interface Backend {
  /**
   * Accessibility tree of the desktop, when the backend can provide one.
   * TODO(auv-ax-tree): AUV exposes no AX snapshot RPC yet; only the mock
   * desktop implements this until a driver capability lands.
   */
  accessibilityTree?: () => Promise<AxNode | null>
  /** Brings an application to the foreground by bundle ID. */
  activateApp: (bundleId: string) => Promise<InputReceipt>
  /** Opens a correlation scope for one script execution, when supported. */
  beginRun: () => Promise<string | undefined>
  captureDisplay: (displayId?: string) => Promise<CapturedFrame>
  captureWindow: (windowId: string) => Promise<CapturedFrame>
  clickScreen: (point: Point, options?: ClickOptions) => Promise<InputReceipt>
  clickWindow: (windowId: string, point: Point, options?: ClickOptions) => Promise<InputReceipt>
  dispose: () => Promise<void>
  endRun: (outcome: RunOutcomeKind) => Promise<void>
  findDisplayText: (query: string, displayId?: string, region?: NormalizedRect) => Promise<TextSearchResult>
  findWindowText: (windowId: string, query: string, region?: NormalizedRect) => Promise<TextSearchResult>
  readonly kind: 'auv' | 'mock' | 'replay'
  readonly label: string
  listDisplays: () => Promise<DisplayInfo[]>
  listWindows: () => Promise<WindowInfo[]>
  pressKey: (key: string) => Promise<InputReceipt>
  recognizeText: (frame: CapturedFrame, region?: NormalizedRect) => Promise<TextSearchResult>
  resolveWindow: (selector: WindowSelector) => Promise<WindowInfo>
  typeText: (text: string) => Promise<InputReceipt>
}

/** Pixels plus their logical placement, as returned by a capture. */
export interface CapturedFrame {
  bounds: Rect
  height: number
  /** Backend-native frame (e.g. the AUV `CapturedFrame` message) for round-trips like OCR. */
  native?: unknown
  /** Tightly packed RGBA, `width * height * 4` bytes. */
  rgba: Uint8Array
  scale: number
  source: string
  width: number
}

export interface DisplayInfo {
  frame: Rect
  id: string
  name?: string
  primary: boolean
  scale: number
}

export interface InputReceipt {
  path?: string
  /** Screen point actually targeted, when known. */
  point?: Point
}

/**
 * Part of a source image as fractions of its size, inside `[0, 1]` (mirrors
 * `auv.api.image.v1.NormalizedRect`). Same shape as `Rect`, different space.
 */
export type NormalizedRect = Rect

export type RunOutcomeKind = 'canceled' | 'failed' | 'succeeded'

export interface TextSearchResult {
  capture?: CapturedFrame
  matches: TextMatch[]
  text?: string
}

export interface WindowInfo {
  app?: string
  bundleId?: string
  frame: Rect
  id: string
  pid?: number
  title?: string
}
