import type { DuplexCall, DuplexCallOptions, Transport, UnaryCall } from '@auv-js/sdk'

import type { BridgedError, Exchanged } from '../runtime/sdk-bridge'
import type { ClickOptions, KeyboardOptions, Point, Rect, ScrollDelta, ScrollUntilUpdate, WindowSelector } from '../script-api/api'
import type { AxNode, Backend, CapturedFrame, DisplayInfo, InputReceipt, RunOutcomeKind, ScrollUntilOutcome, ScrollUntilRequest, SdkAccess, TextSearchResult, WindowInfo } from './types'

import { bridgedError, remoteError } from '../runtime/sdk-bridge'

/** One device call of a live run: what was asked and what the device answered. */
export interface RecordedCall {
  error?: string
  /** Canonical argument key used to detect divergence on replay. */
  key: string
  method: RecordedMethod
  result?: unknown
}

/** One direct SDK call of a live run. */
export interface RecordedRpc {
  error?: BridgedError
  /** Request and response messages in the order they crossed. */
  exchange: Exchanged[]
  /** A JSON-encoded HTTP binding's response text, in place of a response message. */
  json?: string
  /** gRPC path, `/package.Service/Method`. */
  method: string
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
  /** Scripts' direct SDK calls, as encoded messages; see `ReplayTransport`. */
  rpc: RecordedRpc[]
  /** The live device's SDK access, for decoding and routing those calls offline. */
  sdk?: Omit<SdkAccess, 'target'>
}

type RecordedMethod = Exclude<keyof Backend, 'accessibilityTree' | 'beginRun' | 'captureImage' | 'dispose' | 'endRun' | 'kind' | 'label' | 'sdk'>

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
  readonly rpc: RecordedRpc[] = []
  sdk?: () => SdkAccess

  constructor(private readonly inner: Backend) {
    this.kind = inner.kind
    this.label = inner.label
    if (inner.accessibilityTree)
      this.accessibilityTree = () => inner.accessibilityTree!()
    if (inner.sdk) {
      const transports = new WeakMap<Transport, Transport>()
      this.sdk = () => {
        const access = inner.sdk!()
        let recording = transports.get(access.target.transport)
        if (!recording) {
          recording = recordingTransport(access.target.transport, this.rpc)
          transports.set(access.target.transport, recording)
        }
        return { ...access, target: { ...access.target, transport: recording } }
      }
    }
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

  findDisplayText(query: string, displayId?: string, area?: Rect): Promise<TextSearchResult> {
    return this.#record('findDisplayText', [query, displayId, area], () => this.inner.findDisplayText(query, displayId, area))
  }

  findWindowText(windowId: string, query: string, area?: Rect): Promise<TextSearchResult> {
    return this.#record('findWindowText', [windowId, query, area], () => this.inner.findWindowText(windowId, query, area))
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

  pressKeyWindow(windowId: string, key: string, options?: KeyboardOptions): Promise<InputReceipt> {
    return this.#record('pressKeyWindow', [windowId, key, options], () => this.inner.pressKeyWindow(windowId, key, options))
  }

  recognizeText(frame: CapturedFrame, area?: Rect): Promise<TextSearchResult> {
    return this.#record('recognizeText', [frame, area], () => this.inner.recognizeText(frame, area))
  }

  /** The recording of this run so far. */
  recording(): Recording {
    const access = this.inner.sdk?.()
    return {
      entries: this.entries,
      images: this.images,
      label: this.label,
      rpc: this.rpc,
      sdk: access && { describe: access.describe, route: access.route },
    }
  }

  resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    return this.#record('resolveWindow', [selector], () => this.inner.resolveWindow(selector))
  }

  scrollWindow(windowId: string, point: Point, delta: ScrollDelta): Promise<InputReceipt> {
    return this.#record('scrollWindow', [windowId, point, delta], () => this.inner.scrollWindow(windowId, point, delta))
  }

  scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (update: ScrollUntilUpdate) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    // The predicate itself is not recorded; replay returns the recorded outcome.
    return this.#record('scrollWindowUntil', [windowId, point, request, decide !== undefined], () => this.inner.scrollWindowUntil(windowId, point, request, decide))
  }

  typeText(text: string): Promise<InputReceipt> {
    return this.#record('typeText', [text], () => this.inner.typeText(text))
  }

  typeTextWindow(windowId: string, text: string, options?: KeyboardOptions): Promise<InputReceipt> {
    return this.#record('typeTextWindow', [windowId, text, options], () => this.inner.typeTextWindow(windowId, text, options))
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
  sdk?: () => SdkAccess
  #index = 0

  constructor(private readonly recording: Recording) {
    this.label = `Replay of ${recording.label}`
    const access = recording.sdk
    if (access) {
      const target = { transport: new ReplayTransport(recording.rpc) }
      this.sdk = () => ({ ...access, target })
    }
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

  findDisplayText(query: string, displayId?: string, area?: Rect): Promise<TextSearchResult> {
    return this.#next('findDisplayText', [query, displayId, area])
  }

  findWindowText(windowId: string, query: string, area?: Rect): Promise<TextSearchResult> {
    return this.#next('findWindowText', [windowId, query, area])
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

  pressKeyWindow(windowId: string, key: string, options?: KeyboardOptions): Promise<InputReceipt> {
    return this.#next('pressKeyWindow', [windowId, key, options])
  }

  recognizeText(frame: CapturedFrame, area?: Rect): Promise<TextSearchResult> {
    return this.#next('recognizeText', [frame, area])
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
  scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (update: ScrollUntilUpdate) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    return this.#next('scrollWindowUntil', [windowId, point, request, decide !== undefined])
  }

  typeText(text: string): Promise<InputReceipt> {
    return this.#next('typeText', [text])
  }

  typeTextWindow(windowId: string, text: string, options?: KeyboardOptions): Promise<InputReceipt> {
    return this.#next('typeTextWindow', [windowId, text, options])
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

/**
 * Answers scripts' direct SDK calls from a recording. A call matches the first
 * unused recorded call with the same method and request bytes, so calls a
 * script made concurrently replay in any order. Streams replay in lockstep:
 * each recorded request waits for the script's matching request.
 * A call or request the recording does not have raises `ReplayDivergence`.
 */
export class ReplayTransport implements Transport {
  readonly #used = new Set<number>()

  constructor(private readonly calls: readonly RecordedRpc[]) {}

  close(): void {}

  async connect(): Promise<void> {}

  async duplex(call: DuplexCallOptions): Promise<DuplexCall> {
    const sent: Array<Uint8Array | undefined> = []
    let wake: (() => void) | undefined
    const push = (body: Uint8Array | undefined) => {
      sent.push(body)
      wake?.()
    }
    const nextSent = async (): Promise<Uint8Array | undefined> => {
      while (sent.length === 0)
        await new Promise<void>((resolve) => { wake = resolve })
      return sent.shift()
    }
    const take = (method: string, body: Uint8Array) => this.#take(method, body)
    let open = true
    return {
      close: () => {
        open = false
        push(undefined)
      },
      halfClose: async () => push(undefined),
      responses: (async function* () {
        const first = await nextSent()
        if (!first)
          return
        const entry = take(call.method, first)
        for (const frame of entry.exchange.slice(1)) {
          if ('response' in frame) {
            await pace()
            yield frame.response
            continue
          }
          const body = await nextSent()
          if (!open)
            return
          if (!body || !equalBytes(body, frame.request))
            throw new ReplayDivergence(`${methodName(call.method)} diverged: the script sent a different stream message than the recording.`)
        }
        if (entry.error)
          throw remoteError(entry.error)
      })(),
      send: async body => push(body),
    }
  }

  async unary(call: UnaryCall): Promise<Uint8Array> {
    const entry = this.#take(call.method, call.body)
    await pace()
    if (entry.error)
      throw remoteError(entry.error)
    if (entry.json !== undefined)
      return call.decodeJson(entry.json)
    const response = entry.exchange.find(frame => 'response' in frame)
    if (!response || !('response' in response))
      throw new ReplayDivergence(`${methodName(call.method)} has no recorded response.`)
    return response.response
  }

  #take(method: string, body: Uint8Array): RecordedRpc {
    const index = this.calls.findIndex((entry, candidate) => !this.#used.has(candidate) && entry.method === method && requestOf(entry) !== undefined && equalBytes(requestOf(entry)!, body))
    if (index === -1) {
      const recorded = this.calls.some(entry => entry.method === method)
      throw new ReplayDivergence(recorded
        ? `${methodName(method)} diverged: the recording has no such call with these arguments; run live from here.`
        : `${methodName(method)} goes beyond the recording; run live from here.`)
    }
    this.#used.add(index)
    return this.calls[index]!
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

function equalBytes(a: Uint8Array, b: Uint8Array): boolean {
  return a.byteLength === b.byteLength && a.every((byte, index) => byte === b[index])
}

function methodName(method: string): string {
  return method.slice(method.lastIndexOf('/') + 1)
}

// Keep live-like pacing short so stepping and the time strip stay readable.
async function pace(): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, 4))
}

/** Wraps a live transport so every call is appended to `log` as it ends. */
function recordingTransport(inner: Transport, log: RecordedRpc[]): Transport {
  return {
    close: options => inner.close(options),
    connect: options => inner.connect(options),
    async duplex(call) {
      const entry: RecordedRpc = { exchange: [], method: call.method }
      log.push(entry)
      const stream = await inner.duplex(call)
      return {
        close: options => stream.close(options),
        halfClose: () => stream.halfClose(),
        responses: (async function* () {
          try {
            for await (const body of stream.responses) {
              entry.exchange.push({ response: body })
              yield body
            }
          }
          catch (error) {
            entry.error = bridgedError(error)
            throw error
          }
        })(),
        send: async (body) => {
          entry.exchange.push({ request: body })
          await stream.send(body)
        },
      }
    },
    async unary(call) {
      const entry: RecordedRpc = { exchange: [{ request: call.body }], method: call.method }
      log.push(entry)
      try {
        const body = await inner.unary({
          ...call,
          decodeJson: (text) => {
            entry.json = text
            return call.decodeJson(text)
          },
        })
        if (entry.json === undefined)
          entry.exchange.push({ response: body })
        return body
      }
      catch (error) {
        entry.error = bridgedError(error)
        throw error
      }
    },
  }
}

function requestOf(entry: RecordedRpc): Uint8Array | undefined {
  const first = entry.exchange[0]
  return first && 'request' in first ? first.request : undefined
}
