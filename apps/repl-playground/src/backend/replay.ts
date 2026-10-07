import type { ClickOptions, Point, ScrollDelta, ScrollObservation, WindowSelector } from '../script-api/api'
import type { AxNode, Backend, CapturedFrame, DisplayInfo, InputReceipt, NormalizedRect, RunOutcomeKind, ScrollUntilOutcome, ScrollUntilRequest, TextSearchResult, WindowInfo } from './types'

/** One device call of a live run: what was asked and what the device answered. */
export interface RecordedCall {
  error?: string
  /** Canonical argument key used to detect divergence on replay. */
  key: string
  method: RecordedMethod
  result?: unknown
}

/** A live run's device calls, plus the capture images loaded during it. */
export interface Recording {
  entries: RecordedCall[]
  /**
   * Images by capture reference. NOTICE(replay-capture-images): bitmaps load
   * asynchronously after a capture call returns, so image fetches are kept
   * out of the ordered call log and looked up by reference instead.
   */
  images: Map<string, Blob>
  label: string
}

type RecordedMethod = Exclude<keyof Backend, 'accessibilityTree' | 'beginRun' | 'captureImage' | 'dispose' | 'endRun' | 'kind' | 'label'>

/**
 * Wraps a live backend and records every device call with its result, so the
 * same script can later run offline against the recording.
 */
export class RecordingBackend implements Backend {
  accessibilityTree?: () => Promise<AxNode | null>
  readonly entries: RecordedCall[] = []
  readonly images = new Map<string, Blob>()
  readonly kind: Backend['kind']

  readonly label: string

  constructor(private readonly inner: Backend) {
    this.kind = inner.kind
    this.label = inner.label
    if (inner.accessibilityTree)
      this.accessibilityTree = () => inner.accessibilityTree!()
  }

  activateApp(bundleId: string): Promise<InputReceipt> {
    return this.#record('activateApp', [bundleId], () => this.inner.activateApp(bundleId))
  }

  beginRun(): Promise<string | undefined> {
    return this.inner.beginRun()
  }

  captureDisplay(displayId?: string, options?: { logical?: boolean }): Promise<CapturedFrame> {
    return this.#record('captureDisplay', [displayId, options], () => this.inner.captureDisplay(displayId, options))
  }

  async captureImage(frame: CapturedFrame, maxSize: { height: number, width: number }): Promise<Blob> {
    const image = await this.inner.captureImage(frame, maxSize)
    this.images.set(frame.ref, image)
    return image
  }

  captureWindow(windowId: string): Promise<CapturedFrame> {
    return this.#record('captureWindow', [windowId], () => this.inner.captureWindow(windowId))
  }

  clickScreen(point: Point, options?: ClickOptions): Promise<InputReceipt> {
    return this.#record('clickScreen', [point, options], () => this.inner.clickScreen(point, options))
  }

  clickWindow(windowId: string, point: Point, options?: ClickOptions): Promise<InputReceipt> {
    return this.#record('clickWindow', [windowId, point, options], () => this.inner.clickWindow(windowId, point, options))
  }

  dispose(): Promise<void> {
    return this.inner.dispose()
  }

  endRun(outcome: RunOutcomeKind): Promise<void> {
    return this.inner.endRun(outcome)
  }

  findDisplayText(query: string, displayId?: string, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#record('findDisplayText', [query, displayId, region], () => this.inner.findDisplayText(query, displayId, region))
  }

  findWindowText(windowId: string, query: string, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#record('findWindowText', [windowId, query, region], () => this.inner.findWindowText(windowId, query, region))
  }

  listDisplays(): Promise<DisplayInfo[]> {
    return this.#record('listDisplays', [], () => this.inner.listDisplays())
  }

  listWindows(): Promise<WindowInfo[]> {
    return this.#record('listWindows', [], () => this.inner.listWindows())
  }

  pressKey(key: string): Promise<InputReceipt> {
    return this.#record('pressKey', [key], () => this.inner.pressKey(key))
  }

  recognizeText(frame: CapturedFrame, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#record('recognizeText', [frame, region], () => this.inner.recognizeText(frame, region))
  }

  resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    return this.#record('resolveWindow', [selector], () => this.inner.resolveWindow(selector))
  }

  scrollWindow(windowId: string, point: Point, delta: ScrollDelta): Promise<InputReceipt> {
    return this.#record('scrollWindow', [windowId, point, delta], () => this.inner.scrollWindow(windowId, point, delta))
  }

  scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (observation: ScrollObservation) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    // The predicate itself is not recorded; replay returns the recorded outcome.
    return this.#record('scrollWindowUntil', [windowId, point, request, decide !== undefined], () => this.inner.scrollWindowUntil(windowId, point, request, decide))
  }

  typeText(text: string): Promise<InputReceipt> {
    return this.#record('typeText', [text], () => this.inner.typeText(text))
  }

  async #record<T>(method: RecordedMethod, args: unknown[], run: () => Promise<T>): Promise<T> {
    const entry: RecordedCall = { key: argumentKey(args), method }
    this.entries.push(entry)
    try {
      const result = await run()
      entry.result = result
      return result
    }
    catch (error) {
      entry.error = error instanceof Error ? error.message : String(error)
      throw error
    }
  }
}

/**
 * Answers device calls from a recording in order. A call that differs from
 * the recording (method or arguments) raises `ReplayDivergence`, marking where
 * the edited script needs a live device again.
 *
 * TODO(replay-determinism): `Date.now()`, `Math.random()` and `sleep()` are not
 * recorded, so scripts that branch on them may diverge spuriously. Record them
 * in the worker when that shows up in practice.
 */
export class ReplayBackend implements Backend {
  readonly kind = 'replay'
  readonly label: string
  #index = 0

  constructor(private readonly recording: Recording) {
    this.label = `Replay of ${recording.label}`
  }

  activateApp(bundleId: string): Promise<InputReceipt> {
    return this.#next('activateApp', [bundleId])
  }

  async beginRun(): Promise<string | undefined> {
    return undefined
  }

  captureDisplay(displayId?: string, options?: { logical?: boolean }): Promise<CapturedFrame> {
    return this.#next('captureDisplay', [displayId, options])
  }

  async captureImage(frame: CapturedFrame): Promise<Blob> {
    const image = this.recording.images.get(frame.ref)
    if (!image)
      throw new Error(`The recording has no image for capture ${frame.ref}`)
    return image
  }

  captureWindow(windowId: string): Promise<CapturedFrame> {
    return this.#next('captureWindow', [windowId])
  }

  clickScreen(point: Point, options?: ClickOptions): Promise<InputReceipt> {
    return this.#next('clickScreen', [point, options])
  }

  clickWindow(windowId: string, point: Point, options?: ClickOptions): Promise<InputReceipt> {
    return this.#next('clickWindow', [windowId, point, options])
  }

  async dispose(): Promise<void> {}

  async endRun(): Promise<void> {}

  findDisplayText(query: string, displayId?: string, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#next('findDisplayText', [query, displayId, region])
  }

  findWindowText(windowId: string, query: string, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#next('findWindowText', [windowId, query, region])
  }

  listDisplays(): Promise<DisplayInfo[]> {
    return this.#next('listDisplays', [])
  }

  listWindows(): Promise<WindowInfo[]> {
    return this.#next('listWindows', [])
  }

  pressKey(key: string): Promise<InputReceipt> {
    return this.#next('pressKey', [key])
  }

  recognizeText(frame: CapturedFrame, region?: NormalizedRect): Promise<TextSearchResult> {
    return this.#next('recognizeText', [frame, region])
  }

  resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    return this.#next('resolveWindow', [selector])
  }

  scrollWindow(windowId: string, point: Point, delta: ScrollDelta): Promise<InputReceipt> {
    return this.#next('scrollWindow', [windowId, point, delta])
  }

  // NOTICE(replay-scroll-predicate): replay returns the recorded outcome
  // without running an `until` predicate, so an edited predicate does not
  // diverge; only whether one is present is compared.
  scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (observation: ScrollObservation) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    return this.#next('scrollWindowUntil', [windowId, point, request, decide !== undefined])
  }

  typeText(text: string): Promise<InputReceipt> {
    return this.#next('typeText', [text])
  }

  async #next<T>(method: RecordedMethod, args: unknown[]): Promise<T> {
    const index = this.#index++
    const entry = this.recording.entries[index]
    const key = argumentKey(args)
    if (!entry)
      throw new ReplayDivergence(`Device call #${index + 1} ${method}(${key}) goes beyond the recording; run live from here.`)
    if (entry.method !== method || entry.key !== key)
      throw new ReplayDivergence(`Device call #${index + 1} diverged: recorded ${entry.method}(${entry.key}), script called ${method}(${key}).`)
    // Keep live-like pacing short so stepping and the time strip stay readable.
    await new Promise(resolve => setTimeout(resolve, 4))
    if (entry.error !== undefined)
      throw new Error(entry.error)
    return entry.result as T
  }
}

export class ReplayDivergence extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ReplayDivergence'
  }
}

/** Canonical form of call arguments; frames are identified by source and bounds, not their per-run reference. */
function argumentKey(args: unknown[]): string {
  // Omitted optional arguments (e.g. no OCR `region`) do not show as `null`.
  const end = args.findLastIndex(arg => arg !== undefined) + 1
  return JSON.stringify(args.slice(0, end), (_key, value: unknown) => {
    if (value && typeof value === 'object' && 'ref' in value && 'source' in value) {
      const frame = value as CapturedFrame
      return { bounds: frame.bounds, source: frame.source }
    }
    return value
  }) ?? '[]'
}
