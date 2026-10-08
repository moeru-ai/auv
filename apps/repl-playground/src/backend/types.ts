import type { DescribedRpcMethod } from '@auv-js/sdk'

import type { BridgeTarget } from '../runtime/sdk-bridge'
import type { ClickOptions, KeyboardOptions, Point, Rect, ScrollDelta, ScrollUntilResult, ScrollUntilUpdate, TextMatch, WindowSelector } from '../script-api/api'

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
  /**
   * `logical` captures one pixel per point: for display-only frames such as
   * live mode, a quarter of a Retina capture. Script captures stay native so
   * OCR reads full detail.
   */
  captureDisplay: (displayId?: string, options?: { logical?: boolean }) => Promise<CapturedFrame>
  /**
   * Encoded pixels of a capture, fit inside `maxSize` (never enlarged). The
   * only call that moves pixels; everything else passes `CapturedFrame.ref`.
   */
  captureImage: (frame: CapturedFrame, maxSize: { height: number, width: number }) => Promise<Blob>
  captureWindow: (windowId: string) => Promise<CapturedFrame>
  clickScreen: (point: Point, options?: ClickOptions) => Promise<InputReceipt>
  clickWindow: (windowId: string, point: Point, options?: ClickOptions) => Promise<InputReceipt>
  dispose: () => Promise<void>
  endRun: (outcome: RunOutcomeKind) => Promise<void>
  /** `area` limits the search to a logical screen rectangle. */
  findDisplayText: (query: string, displayId?: string, area?: Rect) => Promise<TextSearchResult>
  findWindowText: (windowId: string, query: string, area?: Rect) => Promise<TextSearchResult>
  readonly kind: 'auv' | 'mock' | 'replay'
  readonly label: string
  listDisplays: () => Promise<DisplayInfo[]>
  listWindows: () => Promise<WindowInfo[]>
  pressKey: (key: string) => Promise<InputReceipt>
  /** Presses a key or `+`-joined chord in a window, foreground first unless `background`. */
  pressKeyWindow: (windowId: string, key: string, options?: KeyboardOptions) => Promise<InputReceipt>
  recognizeText: (frame: CapturedFrame, area?: Rect) => Promise<TextSearchResult>
  resolveWindow: (selector: WindowSelector) => Promise<WindowInfo>
  /** Wheel-scrolls once at a window-local point. */
  scrollWindow: (windowId: string, point: Point, delta: ScrollDelta) => Promise<InputReceipt>
  /**
   * Scrolls at a window-local point in steps, observing after each, until the
   * request's condition, `decide`, the end, or the budget stops it.
   */
  scrollWindowUntil: (windowId: string, point: Point, request: ScrollUntilRequest, decide?: (update: ScrollUntilUpdate) => Promise<boolean>) => Promise<ScrollUntilOutcome>
  /**
   * Serves scripts' direct `@auv-js/sdk` calls. Absent on backends that cannot
   * answer encoded RPCs yet: TODO(playground-sdk-mock), TODO(playground-sdk-replay).
   */
  sdk?: () => SdkAccess
  typeText: (text: string) => Promise<InputReceipt>
  /** Types into a window, foreground first unless `background`. */
  typeTextWindow: (windowId: string, text: string, options?: KeyboardOptions) => Promise<InputReceipt>
}

/** A capture the backend holds: its reference plus logical placement, without pixels. */
export interface CapturedFrame {
  bounds: Rect
  /** Pixel height. */
  height: number
  /** Backend capture reference (AUV `CaptureRef.captureId`) for OCR and `captureImage`. */
  ref: string
  scale: number
  source: string
  /** Pixel width. */
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

export type RunOutcomeKind = 'canceled' | 'failed' | 'succeeded'

export interface ScrollUntilOutcome {
  /** The last update's capture and OCR, when the loop observed anything. */
  capture?: CapturedFrame
  match?: TextMatch
  reason: ScrollUntilResult['reason']
  /** Delivery evidence of the first step. */
  receipt?: InputReceipt
  recognized?: TextSearchResult
  steps: number
}

/** `scrollUntil` parameters with the playground's defaults applied. */
export interface ScrollUntilRequest {
  confirmations: number
  delta: ScrollDelta
  maxSteps: number
  settleMs: number
  /** Built-in text condition; the loop otherwise stops at the end. */
  text?: string
}

/** How the host forwards scripts' SDK calls to a backend, and how scripts route them. */
export interface SdkAccess {
  /** Effect and ProtoJSON decoding for a gRPC path, from the Runner's reflection. */
  describe: (method: string) => Promise<DescribedRpcMethod | undefined>
  /** The Device, Run and RunnerClass the script's `runner` targets. */
  route: { deviceId?: string, runId?: string, runnerClass: string }
  target: BridgeTarget
}

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
