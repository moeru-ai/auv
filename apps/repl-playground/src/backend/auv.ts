import type { AuvClient, AuvConnection, Device, DiscoveredRunner, RunnerClient, Transport, WindowClient } from '@auv-js/sdk'

import type { Rect } from '../script-api/api'
import type { Backend, CapturedFrame, DisplayInfo, RunOutcomeKind, SdkAccess, TextSearchResult, WindowInfo } from './types'

import { CaptureResolution, connect, createAuv, createHttpTransport, discoverRunner, ImageEncoding, InputDeliveryPath, pairDevice } from '@auv-js/sdk'

type CaptureResponse = Awaited<ReturnType<RunnerClient['displays']['capture']>>
type NativeAction = Awaited<ReturnType<RunnerClient['input']['typeText']>>['action']
type NativeDisplay = Awaited<ReturnType<RunnerClient['displays']['list']>>[number]
type NativeFrame = NonNullable<CaptureResponse['capture']>
type NativeRecognized = Awaited<ReturnType<RunnerClient['recognizeText']>>
type NativeWindow = WindowClient['window']

const RUNNER_CLASS = 'auv.core.local'

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
  /** The transport scripts' `auv` calls are forwarded to. */
  transport: Transport
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

  async listDisplays(): Promise<DisplayInfo[]> {
    return (await this.#runner.displays.list()).map(toDisplay)
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

  /** The host's own Runner client for the current Run: canvas captures and pixels. */
  #bind(): RunnerClient {
    return this.client.runner({ deviceId: this.device.id, runId: this.#runId, runnerClass: RUNNER_CLASS })
  }
}

/**
 * A backend over any AUV transport: a daemon over HTTP, or an in-memory mock
 * Runner. Scripts' `auv` calls go through `sdk()`.
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
  return { backend: new AuvBackend(connection, client, device, options), device, devices }
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
