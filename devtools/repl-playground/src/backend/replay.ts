import type { ClickOptions, Point, WindowSelector } from '../script-api/api'
import type { AxNode, Backend, CapturedFrame, DisplayInfo, InputReceipt, NormalizedRect, RunOutcomeKind, TextSearchResult, WindowInfo } from './types'

/** One device call of a live run: what was asked and what the device answered. */
export interface RecordedCall {
  error?: string
  /** Canonical argument key used to detect divergence on replay. */
  key: string
  method: RecordedMethod
  result?: unknown
}

type RecordedMethod = Exclude<keyof Backend, 'accessibilityTree' | 'beginRun' | 'dispose' | 'endRun' | 'kind' | 'label'>

/**
 * Wraps a live backend and records every device call with its result, so the
 * same script can later run offline against the recording.
 */
export class RecordingBackend implements Backend {
  accessibilityTree?: () => Promise<AxNode | null>
  readonly entries: RecordedCall[] = []
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

  captureDisplay(displayId?: string): Promise<CapturedFrame> {
    return this.#record('captureDisplay', [displayId], () => this.inner.captureDisplay(displayId))
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

  constructor(private readonly entries: readonly RecordedCall[], source: string) {
    this.label = `Replay of ${source}`
  }

  activateApp(bundleId: string): Promise<InputReceipt> {
    return this.#next('activateApp', [bundleId])
  }

  async beginRun(): Promise<string | undefined> {
    return undefined
  }

  captureDisplay(displayId?: string): Promise<CapturedFrame> {
    return this.#next('captureDisplay', [displayId])
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

  typeText(text: string): Promise<InputReceipt> {
    return this.#next('typeText', [text])
  }

  async #next<T>(method: RecordedMethod, args: unknown[]): Promise<T> {
    const index = this.#index++
    const entry = this.entries[index]
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

/** Canonical form of call arguments; frames are identified by source and bounds, not pixels. */
function argumentKey(args: unknown[]): string {
  // Omitted optional arguments (e.g. no OCR `region`) do not show as `null`.
  const end = args.findLastIndex(arg => arg !== undefined) + 1
  return JSON.stringify(args.slice(0, end), (_key, value: unknown) => {
    if (value && typeof value === 'object' && 'rgba' in value) {
      const frame = value as CapturedFrame
      return { bounds: frame.bounds, source: frame.source }
    }
    return value
  }) ?? '[]'
}
