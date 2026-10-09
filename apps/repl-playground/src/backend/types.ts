import type { DescribedRpcMethod } from '@auv-js/sdk'

import type { TextMatch } from '../handles'
import type { BridgeTarget } from '../runtime/sdk-bridge'
import type { Point, Rect } from '../script-api/api'

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
 * A selected device: the real AUV daemon through `@auv-js/sdk`, or the mock
 * desktop's mock Runner. Scripts reach it only through `sdk()`; the other
 * calls serve the canvas and the device picker outside script runs.
 */
export interface Backend extends RunBackend {
  /**
   * Accessibility tree of the desktop, when the backend can provide one.
   * TODO(auv-ax-tree): AUV exposes no AX snapshot RPC yet; only the mock
   * desktop implements this until a driver capability lands.
   */
  accessibilityTree?: () => Promise<AxNode | null>
  /**
   * `logical` captures one pixel per point: for display-only frames such as
   * live mode, a quarter of a Retina capture.
   */
  captureDisplay: (displayId?: string, options?: { logical?: boolean }) => Promise<CapturedFrame>
  dispose: () => Promise<void>
  listDisplays: () => Promise<DisplayInfo[]>
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
  /** ThumbHash of the whole capture, for a preview before the pixels load. */
  thumbhash?: Uint8Array
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

/** What one script run talks to: the selected device, or a replay of a recorded run. */
export interface RunBackend {
  /** Opens a correlation scope for one script execution, when supported. */
  beginRun: () => Promise<string | undefined>
  /**
   * Encoded pixels of a capture, fit inside `maxSize` (never enlarged), for
   * the canvas. Scripts fetch pixels themselves with `auv.captures.image`.
   */
  captureImage: (frame: CapturedFrame, maxSize: { height: number, width: number }) => Promise<Blob>
  endRun: (outcome: RunOutcomeKind) => Promise<void>
  readonly kind: 'auv' | 'mock' | 'replay'
  readonly label: string
  /** Serves the script's `auv` Runner client: a device, the mock Runner, or a replay. */
  sdk: () => SdkAccess
}

export type RunOutcomeKind = 'canceled' | 'failed' | 'succeeded'

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
