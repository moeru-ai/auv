import type { AuvClient, AuvConnection, Device, DiscoveredRunner, RunnerClient, Transport, WindowClient } from '@auv-js/sdk'

import type { ClickOptions, KeyboardOptions, Point, Rect, ScrollDelta, ScrollUntilUpdate, WindowSelector } from '../script-api/api'
import type { Backend, CapturedFrame, DisplayInfo, InputReceipt, RunOutcomeKind, ScrollUntilOutcome, ScrollUntilRequest, SdkAccess, TextSearchResult, WindowInfo } from './types'

import { AuvRemoteError, CaptureResolution, connect, createAuv, createHttpTransport, discoverRunner, ImageEncoding, InputDeliveryPath, InputPolicy, MouseButton, pairDevice, ScrollUntilStopReason } from '@auv-js/sdk'

type CaptureResponse = Awaited<ReturnType<RunnerClient['displays']['capture']>>
type NativeAction = Awaited<ReturnType<RunnerClient['input']['typeText']>>['action']
type NativeDisplay = Awaited<ReturnType<RunnerClient['displays']['list']>>[number]
type NativeFrame = NonNullable<CaptureResponse['capture']>
type NativeRecognized = Awaited<ReturnType<RunnerClient['recognizeText']>>
type NativeScrollUpdate = Parameters<NonNullable<NonNullable<Parameters<WindowClient['scrollUntil']>[2]>['onUpdate']>>[0]
type NativeWindow = WindowClient['window']

const RUNNER_CLASS = 'auv.core.local'

const MOUSE_BUTTONS = { left: MouseButton.LEFT, middle: MouseButton.MIDDLE, right: MouseButton.RIGHT } as const

const STOP_REASONS: Partial<Record<ScrollUntilStopReason, ScrollUntilOutcome['reason']>> = {
  [ScrollUntilStopReason.BUDGET_EXHAUSTED]: 'budget',
  [ScrollUntilStopReason.END_BY_NO_VISUAL_PROGRESS]: 'end',
  [ScrollUntilStopReason.PREDICATE_SATISFIED]: 'until',
  [ScrollUntilStopReason.TEXT_VISIBLE]: 'text-visible',
}

export interface AuvBackendOptions {
  /** Paired Device credential; omit for a local loopback daemon without a pairing store. */
  credential?: string
  /** Stable Device ID to route operations to; the daemon's local Device when omitted. */
  deviceId?: string
  endpoint: string
}

/** How a backend reaches its Runner, beyond the AUV connection itself. */
interface BackendOptions {
  /** Desktop accessibility tree, for a backend with no AX snapshot RPC. */
  accessibilityTree?: Backend['accessibilityTree']
  credential?: string
  kind?: Backend['kind']
  label?: string
  /** The transport scripts' direct SDK calls are forwarded to. */
  transport: Transport
}

/** Screen point of a window-local point, when the response reported the window frame. */
function screenPoint(frame: Parameters<typeof toRect>[0], point: Point): Point | undefined {
  const rect = toRect(frame)
  return rect ? { x: rect.x + point.x, y: rect.y + point.y } : undefined
}

class AuvBackend implements Backend {
  readonly accessibilityTree?: Backend['accessibilityTree']
  readonly kind: Backend['kind']
  readonly label: string
  #discovered?: Promise<DiscoveredRunner | undefined>
  #runId?: string
  #runner: RunnerClient

  constructor(
    private readonly connection: AuvConnection,
    private readonly client: AuvClient,
    private readonly device: Device,
    private readonly http: BackendOptions,
  ) {
    this.accessibilityTree = http.accessibilityTree
    this.kind = http.kind ?? 'auv'
    // NOTICE(device-name): a local daemon may report an empty Device name.
    this.label = http.label ?? `${device.name || device.labels.hostname || device.id.slice(0, 12)} (${device.platform})`
    this.#runner = this.#bind()
  }

  async activateApp(bundleId: string): Promise<InputReceipt> {
    const response = await this.#runner.macos.applications.activateBundleId({ bundleId })
    return { path: response.verification?.verification.case ?? 'unverified' }
  }

  async beginRun(): Promise<string | undefined> {
    try {
      const run = await this.client.runs.create({ deviceIds: [this.device.id] })
      this.#runId = run.id
    }
    catch (error) {
      // NOTICE(playground-run-optional): Run creation is correlation only; a
      // daemon that refuses it should not block interactive execution.
      console.warn('AUV Run creation failed; continuing without a Run', error)
      this.#runId = undefined
    }
    this.#runner = this.#bind()
    return this.#runId
  }

  async captureDisplay(displayId?: string, options?: { logical?: boolean }): Promise<CapturedFrame> {
    const selector = displayId ? { selector: { case: 'display' as const, value: { displayId } } } : undefined
    const response = await this.#runner.displays.capture(selector, options?.logical ? { resolution: CaptureResolution.LOGICAL } : undefined)
    return toFrame(response.capture, `display:${response.display?.displayId ?? displayId ?? 'primary'}`)
  }

  async captureImage(frame: CapturedFrame, maxSize: { height: number, width: number }): Promise<Blob> {
    // JPEG keeps a logical-resolution Retina window around 0.3–1 MB; PNG text
    // edges are crisper but several times larger and slower to encode.
    const image = await this.#runner.captures.image(frame.ref, { encoding: ImageEncoding.JPEG, maxSize })
    return new Blob([image.data as Uint8Array<ArrayBuffer>], { type: 'image/jpeg' })
  }

  async captureWindow(windowId: string): Promise<CapturedFrame> {
    const window = this.#runner.windows.from(windowId)
    const captured = await window.capture()
    return toFrame(captured.capture, `window:${windowId}`)
  }

  async clickScreen(point: { x: number, y: number }, options?: ClickOptions): Promise<InputReceipt> {
    const response = await this.#runner.input.click(point, {
      button: MOUSE_BUTTONS[options?.button ?? 'left'],
      click: toClick(options),
    })
    return { path: deliveryPath(response.action), point }
  }

  async clickWindow(windowId: string, point: { x: number, y: number }, options?: ClickOptions): Promise<InputReceipt> {
    const window = this.#runner.windows.from(windowId)
    const response = await window.click(point, {
      button: MOUSE_BUTTONS[options?.button ?? 'left'],
      click: toClick(options),
    })
    const delivered = response.screenPoint
    return { path: deliveryPath(response.action), point: delivered && { x: delivered.x, y: delivered.y } }
  }

  async dispose(): Promise<void> {
    await this.connection.close()
  }

  async endRun(outcome: RunOutcomeKind): Promise<void> {
    const runId = this.#runId
    this.#runId = undefined
    this.#runner = this.#bind()
    if (runId)
      await this.client.runs.stop({ outcome, runId }).catch(error => console.warn('AUV Run stop failed', error))
  }

  async findDisplayText(query: string, displayId?: string, area?: Rect): Promise<TextSearchResult> {
    const selector = displayId ? { case: 'display' as const, value: { displayId } } : undefined
    const response = await this.#runner.displays.findText(selector ? { selector } : undefined, query, area ? { screenRegion: area } : undefined)
    return {
      capture: response.capture ? toFrame(response.capture, `display:${response.display?.displayId ?? 'primary'}`) : undefined,
      matches: response.matches.map(match => ({ bounds: toRect(match.bounds)!, confidence: match.confidence, text: match.text })),
    }
  }

  async findWindowText(windowId: string, query: string, area?: Rect): Promise<TextSearchResult> {
    const window = this.#runner.windows.from(windowId)
    const response = await window.findText(query, area ? { screenRegion: area } : undefined)
    return {
      capture: response.capture ? toFrame(response.capture, `window:${windowId}`) : undefined,
      matches: response.matches.map(match => ({ bounds: toRect(match.bounds)!, confidence: match.confidence, text: match.text })),
    }
  }

  async listDisplays(): Promise<DisplayInfo[]> {
    return (await this.#runner.displays.list()).map(toDisplay)
  }

  async listWindows(): Promise<WindowInfo[]> {
    return (await this.#runner.windows.list()).map(window => toWindow(window.window))
  }

  async pressKey(key: string): Promise<InputReceipt> {
    const response = await this.#runner.input.pressKey(key)
    return { path: deliveryPath(response.action) }
  }

  async pressKeyWindow(windowId: string, key: string, options?: KeyboardOptions): Promise<InputReceipt> {
    const action = await this.#runner.windows.from(windowId).pressKeys(key, { policy: keyboardPolicy(options) })
    return { path: deliveryPath(action) }
  }

  async recognizeText(frame: CapturedFrame, area?: Rect): Promise<TextSearchResult> {
    return toRecognized(await this.#runner.recognizeText(frame.ref, area ? { screenRegion: area } : undefined))
  }

  async resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    return toWindow((await this.#runner.windows.resolve(selector)).window)
  }

  async scrollWindow(windowId: string, point: Point, delta: ScrollDelta): Promise<InputReceipt> {
    const window = this.#runner.windows.from(windowId)
    const response = await window.scroll(point, { deltaX: delta.dx ?? 0, deltaY: delta.dy ?? 0 })
    return { path: deliveryPath(response.action), point: screenPoint(response.window?.frame, point) }
  }

  async scrollWindowUntil(windowId: string, point: Point, request: ScrollUntilRequest, decide?: (update: ScrollUntilUpdate) => Promise<boolean>): Promise<ScrollUntilOutcome> {
    const window = this.#runner.windows.from(windowId)
    let last: NativeScrollUpdate | undefined
    const completed = await window.scrollUntil(point, {
      condition: request.text ? { case: 'textVisible', value: { query: request.text } } : { case: 'end', value: {} },
      maxSteps: request.maxSteps,
      noMotionConfirmations: request.confirmations,
      settle: toDuration(request.settleMs),
      step: { case: 'instant', value: { deltaX: request.delta.dx ?? 0, deltaY: request.delta.dy ?? 0 } },
    }, {
      onUpdate: (update) => {
        last = update
      },
      until: decide && (update => decide({
        moved: update.motion ? !update.motion.noMotion : false,
        steps: update.steps,
        text: update.text?.text ?? '',
      })),
    })
    const match = completed.textMatch
    return {
      capture: last?.capture ? toFrame(last.capture, `window:${windowId}`) : undefined,
      match: match ? { bounds: toRect(match.bounds)!, confidence: 1, text: match.text } : undefined,
      reason: STOP_REASONS[completed.reason] ?? 'end',
      receipt: completed.action ? { path: deliveryPath(completed.action) } : undefined,
      recognized: last?.text ? toRecognized(last.text) : undefined,
      steps: completed.steps,
    }
  }

  sdk(): SdkAccess {
    // Reflection runs once per backend; inspection falls back to bare method
    // names if the Runner does not answer it.
    this.#discovered ??= discoverRunner(this.connection, { deviceId: this.device.id, runnerClass: RUNNER_CLASS })
      .catch((error: unknown) => {
        console.warn('AUV Runner reflection failed; SDK calls are recorded without effects', error)
        return undefined
      })
    const discovered = this.#discovered
    return {
      describe: async method => (await discovered)?.describeMethod(method),
      route: { deviceId: this.device.id, runId: this.#runId, runnerClass: RUNNER_CLASS },
      target: {
        headers: this.http.credential === undefined ? undefined : { authorization: `Bearer ${this.http.credential}` },
        transport: this.http.transport,
      },
    }
  }

  async typeText(text: string): Promise<InputReceipt> {
    const response = await this.#runner.input.typeText(text)
    return { path: deliveryPath(response.action) }
  }

  async typeTextWindow(windowId: string, text: string, options?: KeyboardOptions): Promise<InputReceipt> {
    const action = await this.#runner.windows.from(windowId).typeText(text, { policy: keyboardPolicy(options) })
    return { path: deliveryPath(action) }
  }

  /** Window references are Device resources; each call takes a client for the current Run with `windows.from(id)`. */
  #bind(): RunnerClient {
    return this.client.runner({ deviceId: this.device.id, runId: this.#runId, runnerClass: RUNNER_CLASS })
  }
}

/**
 * A backend over any AUV transport: a daemon over HTTP, or an in-memory mock
 * Runner. `auv.*` bindings and scripts' direct SDK calls both go through it.
 */
export async function connectBackend(options: BackendOptions, deviceId?: string): Promise<{ backend: Backend, device: Device, devices: readonly Device[] }> {
  const connection = await connect({ credential: options.credential, transport: options.transport })
  const client = createAuv(connection)
  const devices = await client.devices.list()
  const device = devices.find(candidate => candidate.id === deviceId)
    ?? devices.find(candidate => candidate.local)
    ?? devices[0]
  if (!device)
    throw new Error('The daemon reported no Devices')
  return { backend: withReadableErrors(new AuvBackend(connection, client, device, options)), device, devices }
}

export async function createAuvBackend(options: AuvBackendOptions): Promise<{ backend: Backend, device: Device, devices: readonly Device[] }> {
  return await connectBackend({ credential: options.credential, transport: createHttpTransport({ endpoint: options.endpoint }) }, options.deviceId)
}

/** Delivery path for display, e.g. `window-targeted-mouse` for `WINDOW_TARGETED_MOUSE`. */
export function deliveryPath(action: NativeAction): string | undefined {
  const name = action ? InputDeliveryPath[action.selectedPath] : undefined
  return name && name !== 'UNSPECIFIED' ? name.toLowerCase().replaceAll('_', '-') : undefined
}

/** Pairs this browser with a daemon using a one-time token from `auv devices pair create-token`. */
export async function pairBrowser(endpoint: string, token: string, label = 'AUV Playground'): Promise<string> {
  const bootstrap = await connect({ transport: createHttpTransport({ endpoint }) })
  try {
    const enrollment = await pairDevice(bootstrap, { label, token: { expiresAt: undefined, value: token } })
    return enrollment.credential
  }
  finally {
    await bootstrap.close()
  }
}

export function toDisplay(display: NativeDisplay): DisplayInfo {
  return {
    frame: toRect(display.frame) ?? { height: 0, width: 0, x: 0, y: 0 },
    id: display.displayId,
    name: display.name,
    primary: display.primary,
    scale: display.scaleFactor || 1,
  }
}

/** A capture's reference and placement, from AUV's `CapturedFrame`. */
export function toFrame(capture: NativeFrame | undefined, source: string): CapturedFrame {
  if (!capture?.ref)
    throw new Error('AUV returned a capture without a reference')
  const { height = 0, width = 0 } = capture.pixelSize ?? {}
  return {
    bounds: toRect(capture.bounds) ?? { height, width, x: 0, y: 0 },
    height,
    ref: capture.ref.captureId,
    scale: capture.scaleFactor || 1,
    source,
    thumbhash: capture.thumbhash.length > 0 ? capture.thumbhash : undefined,
    width,
  }
}

/** OCR response as playground matches. */
export function toRecognized(response: NativeRecognized): TextSearchResult {
  return {
    // Region bounds are screen rectangles, like TextMatch bounds (see
    // "Capture Frame" in docs/TERMS_AND_CONCEPTS.md).
    matches: response.regions.map(region => ({ bounds: toRect(region.bounds)!, confidence: region.confidence ?? 1, text: region.text })),
    text: response.text,
  }
}

export function toRect(rect: undefined | { height: number, width: number, x: number, y: number }): Rect | undefined {
  return rect ? { height: rect.height, width: rect.width, x: rect.x, y: rect.y } : undefined
}

export function toWindow(window: NativeWindow): WindowInfo {
  return {
    app: window.applicationName,
    bundleId: window.applicationBundleId,
    frame: toRect(window.frame) ?? { height: 0, width: 0, x: 0, y: 0 },
    id: window.ref?.windowId ?? '',
    pid: window.processId,
    title: window.title,
  }
}

function keyboardPolicy(options: KeyboardOptions | undefined): InputPolicy {
  return options?.background ? InputPolicy.BACKGROUND_ONLY : InputPolicy.FOREGROUND_PREFERRED
}

/**
 * NOTICE(grpc-message-percent-encoding): gRPC percent-encodes `grpc-message`,
 * and AUV 0.0.28's REST proxy copies it into the problem `detail` without
 * decoding (`crates/auv-api-server/src/rest.rs`, `invoke_unary`), so messages
 * arrive as `visible%20application%20%22Todos%22`. Remove once the daemon
 * decodes it.
 */
function readableError(error: unknown): unknown {
  if (!(error instanceof AuvRemoteError) || !/%[0-9a-f]{2}/i.test(error.message))
    return error
  try {
    error.message = decodeURIComponent(error.message)
  }
  catch {}
  return error
}

/**
 * AUV `Click`: the count, plus the interval AUV requires for repeated clicks
 * (`input.proto` `Click.interval`; it rejects a multi-click without one).
 * NOTICE(click-interval-default): 80 ms when the script gives none, the
 * value AUV's own invoke tests use for double clicks.
 */
function toClick(options: ClickOptions | undefined) {
  const count = options?.count ?? 1
  if (count <= 1)
    return { count }
  const ms = options?.interval ?? 80
  if (!(ms > 0))
    throw new RangeError('click interval must be a positive number of milliseconds')
  return { count, interval: toDuration(ms) }
}

/** `google.protobuf.Duration` from milliseconds. */
function toDuration(ms: number) {
  return { nanos: Math.round((ms % 1000) * 1e6), seconds: BigInt(Math.floor(ms / 1000)) }
}

/**
 * Rethrows daemon errors with a readable message from every async backend
 * method, so scripts, the call list and the run bar all see the same text.
 */
function withReadableErrors(backend: AuvBackend): Backend {
  return new Proxy(backend, {
    get(target, key) {
      const value = Reflect.get(target, key, target) as unknown
      if (typeof value !== 'function')
        return value
      // Methods run on the target itself: class private fields are not reachable through a proxy.
      return (...args: unknown[]) => {
        const result = (value as (...args: unknown[]) => unknown).apply(target, args)
        if (!(result instanceof Promise))
          return result
        return result.catch((error: unknown) => {
          throw readableError(error)
        })
      }
    },
  })
}
