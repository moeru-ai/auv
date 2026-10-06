import type { AuvClient, AuvConnection, Device, RunnerClient } from '@auv-js/sdk'

import type { ClickOptions, Rect, WindowSelector } from '../script-api/api'
import type { Backend, CapturedFrame, DisplayInfo, InputReceipt, NormalizedRect, RunOutcomeKind, TextSearchResult, WindowInfo } from './types'

import { AuvRemoteError, connect, createAuv, createHttpTransport, pairDevice } from '@auv-js/sdk'

type CaptureResponse = Awaited<ReturnType<RunnerClient['displays']['capture']>>
type NativeAction = Awaited<ReturnType<RunnerClient['input']['typeText']>>['action']
type NativeDisplay = Awaited<ReturnType<RunnerClient['displays']['list']>>[number]
type NativeFrame = NonNullable<CaptureResponse['capture']>
type NativeWindow = Awaited<ReturnType<RunnerClient['windows']['list']>>[number]
type WindowClient = Awaited<ReturnType<RunnerClient['windows']['resolve']>>

const RUNNER_CLASS = 'auv.core.local'

// Mirrors `auv.api.driver.v1.MouseButton`; the enum is not re-exported by the SDK entry.
const MOUSE_BUTTON = { left: 1, middle: 3, right: 2 } as const

// Mirrors `auv.api.driver.v1.InputDeliveryPath` for display only.
const DELIVERY_PATHS: Record<number, string> = {
  1: 'noop',
  2: 'ax-press',
  3: 'ax-focus',
  4: 'ax-set-value',
  5: 'ax-scroll',
  6: 'ax-selected-text',
  7: 'window-targeted-mouse',
  8: 'window-targeted-wheel',
  9: 'window-targeted-keyboard',
  10: 'window-targeted-keyboard-scroll',
  11: 'clipboard-paste',
  12: 'foreground-system-events',
  13: 'unsupported',
}

export interface AuvBackendOptions {
  /** Paired Device credential; omit for a local loopback daemon without a pairing store. */
  credential?: string
  /** Stable Device ID to route operations to; the daemon's local Device when omitted. */
  deviceId?: string
  endpoint: string
}

class AuvBackend implements Backend {
  readonly kind = 'auv'
  readonly label: string
  #runId?: string
  #runner: RunnerClient
  readonly #windows = new Map<string, WindowClient>()

  constructor(
    private readonly connection: AuvConnection,
    private readonly client: AuvClient,
    private readonly device: Device,
  ) {
    // NOTICE(device-name): a local daemon may report an empty Device name.
    this.label = `${device.name || device.labels.hostname || device.id.slice(0, 12)} (${device.platform})`
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

  async captureDisplay(displayId?: string): Promise<CapturedFrame> {
    const response = await this.#runner.displays.capture(displayId ? { selector: { case: 'display', value: { displayId } } } : undefined)
    return toFrame(response.capture, `display:${response.display?.displayId ?? displayId ?? 'primary'}`)
  }

  async captureWindow(windowId: string): Promise<CapturedFrame> {
    const window = await this.#windowClient(windowId)
    const captured = await window.capture()
    return toFrame(captured.capture, `window:${windowId}`)
  }

  async clickScreen(point: { x: number, y: number }, options?: ClickOptions): Promise<InputReceipt> {
    const response = await this.#runner.input.clickScreenPoint(point, {
      button: MOUSE_BUTTON[options?.button ?? 'left'],
      click: toClick(options),
    })
    return { path: deliveryPath(response.action), point }
  }

  async clickWindow(windowId: string, point: { x: number, y: number }, options?: ClickOptions): Promise<InputReceipt> {
    const window = await this.#windowClient(windowId)
    const response = await window.click(point, {
      button: MOUSE_BUTTON[options?.button ?? 'left'],
      click: toClick(options),
    })
    const frame = toRect(response.window?.frame)
    return {
      path: deliveryPath(response.action),
      point: frame ? { x: frame.x + point.x, y: frame.y + point.y } : undefined,
    }
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

  async findDisplayText(query: string, displayId?: string, region?: NormalizedRect): Promise<TextSearchResult> {
    const selector = displayId ? { case: 'display' as const, value: { displayId } } : undefined
    const response = await this.#runner.displays.findText(selector ? { selector } : undefined, query, region ? { region } : undefined)
    return {
      capture: response.capture ? toFrame(response.capture, `display:${response.display?.displayId ?? 'primary'}`) : undefined,
      matches: response.matches.map(match => ({ bounds: toRect(match.bounds)!, confidence: match.confidence, text: match.text })),
    }
  }

  async findWindowText(windowId: string, query: string, region?: NormalizedRect): Promise<TextSearchResult> {
    const window = await this.#windowClient(windowId)
    const response = await window.findText(query, region ? { region } : undefined)
    return {
      capture: response.capture ? toFrame(response.capture, `window:${windowId}`) : undefined,
      matches: response.matches.map(match => ({ bounds: toRect(match.bounds)!, confidence: match.confidence, text: match.text })),
    }
  }

  async listDisplays(): Promise<DisplayInfo[]> {
    return (await this.#runner.displays.list()).map(toDisplay)
  }

  async listWindows(): Promise<WindowInfo[]> {
    return (await this.#runner.windows.list()).map(toWindow)
  }

  async pressKey(key: string): Promise<InputReceipt> {
    const response = await this.#runner.input.pressKey(key)
    return { path: deliveryPath(response.action) }
  }

  async recognizeText(frame: CapturedFrame, region?: NormalizedRect): Promise<TextSearchResult> {
    if (!frame.native)
      throw new Error('This frame did not come from AUV and cannot be sent to the recognizer')
    const response = await this.#runner.recognizeText(frame.native as NativeFrame, region ? { region } : undefined)
    return {
      // NOTICE(ocr-region-space): regions are treated as screen-space like
      // TextMatch bounds. Revisit if RecognizeTextResponse.origin reports a
      // non-screen space for a frame.
      matches: response.regions.map(region => ({ bounds: toRect(region.bounds)!, confidence: region.confidence ?? 1, text: region.text })),
      text: response.text,
    }
  }

  async resolveWindow(selector: WindowSelector): Promise<WindowInfo> {
    const resolved = await this.#runner.windows.resolve(toSelector(selector))
    this.#windows.set(resolved.id, resolved)
    const window = (await this.#runner.windows.list()).find(candidate => candidate.ref?.windowId === resolved.id)
    return window ? toWindow(window) : { frame: { height: 0, width: 0, x: 0, y: 0 }, id: resolved.id }
  }

  async typeText(text: string): Promise<InputReceipt> {
    const response = await this.#runner.input.typeText(text)
    return { path: deliveryPath(response.action) }
  }

  #bind(): RunnerClient {
    // Window clients carry the route they were resolved with; a new Run needs new clients.
    this.#windows.clear()
    return this.client.runner({ deviceId: this.device.id, runId: this.#runId, runnerClass: RUNNER_CLASS })
  }

  async #windowClient(windowId: string): Promise<WindowClient> {
    const cached = this.#windows.get(windowId)
    if (cached)
      return cached
    // NOTICE(window-client-by-id): the SDK builds a WindowClient only through
    // `windows.resolve`. A window first seen in `windows.list` is re-resolved
    // by process ID and exact title; ambiguous titles may pick a sibling.
    const target = (await this.#runner.windows.list()).find(candidate => candidate.ref?.windowId === windowId)
    if (!target)
      throw new Error(`Window ${windowId} is no longer available`)
    const resolved = await this.#runner.windows.resolve({
      application: target.processId ? { case: 'processId', value: target.processId } : { case: 'frontmostApplication', value: true },
      window: target.title ? { case: 'titleExact', value: target.title } : { case: 'mainVisible', value: true },
    })
    this.#windows.set(windowId, resolved)
    return resolved
  }
}

export async function createAuvBackend(options: AuvBackendOptions): Promise<{ backend: Backend, devices: readonly Device[] }> {
  const connection = await connect({
    credential: options.credential,
    transport: createHttpTransport({ endpoint: options.endpoint }),
  })
  const client = createAuv(connection)
  const devices = await client.devices.list()
  const device = devices.find(candidate => candidate.id === options.deviceId)
    ?? devices.find(candidate => candidate.local)
    ?? devices[0]
  if (!device)
    throw new Error('The daemon reported no Devices')
  return { backend: withReadableErrors(new AuvBackend(connection, client, device)), devices }
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

function deliveryPath(action: NativeAction): string | undefined {
  return action ? DELIVERY_PATHS[action.selectedPath] : undefined
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
  return { count, interval: { nanos: Math.round((ms % 1000) * 1e6), seconds: BigInt(Math.floor(ms / 1000)) } }
}

function toDisplay(display: NativeDisplay): DisplayInfo {
  return {
    frame: toRect(display.frame) ?? { height: 0, width: 0, x: 0, y: 0 },
    id: display.displayId,
    name: display.name,
    primary: display.primary,
    scale: display.scaleFactor || 1,
  }
}

function toFrame(capture: NativeFrame | undefined, source: string): CapturedFrame {
  if (!capture?.image)
    throw new Error('AUV returned a capture without pixels')
  const { data, height, width } = capture.image
  return {
    bounds: toRect(capture.bounds) ?? { height, width, x: 0, y: 0 },
    height,
    native: capture,
    rgba: data,
    scale: capture.scaleFactor || 1,
    source,
    width,
  }
}

function toRect(rect: undefined | { height: number, width: number, x: number, y: number }): Rect | undefined {
  return rect ? { height: rect.height, width: rect.width, x: rect.x, y: rect.y } : undefined
}

function toSelector(selector: WindowSelector): Parameters<RunnerClient['windows']['resolve']>[0] {
  const application = selector.bundleId
    ? { case: 'applicationBundleId' as const, value: selector.bundleId }
    : selector.appName
      ? { case: 'applicationName' as const, value: selector.appName }
      : selector.pid
        ? { case: 'processId' as const, value: selector.pid }
        : { case: 'frontmostApplication' as const, value: true }
  const window = selector.title
    ? { case: 'titleExact' as const, value: selector.title }
    : selector.titleContains
      ? { case: 'titleContains' as const, value: selector.titleContains }
      : { case: 'mainVisible' as const, value: true }
  return { application, window }
}

function toWindow(window: NativeWindow): WindowInfo {
  return {
    app: window.applicationName,
    bundleId: window.applicationBundleId,
    frame: toRect(window.frame) ?? { height: 0, width: 0, x: 0, y: 0 },
    id: window.ref?.windowId ?? '',
    pid: window.processId,
    title: window.title,
  }
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
