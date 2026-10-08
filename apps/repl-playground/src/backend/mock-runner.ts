import type { MockRouter, Transport } from '@auv-js/sdk'

import type { Point, Rect, TextMatch } from '../script-api/api'
import type { MockDesktop } from './mock-desktop'
import type { DisplayInfo, WindowInfo } from './types'

import {
  ApplicationService,
  AuvRpcError,
  CaptureResolution,
  CaptureService,
  contains,
  createMockTransport,
  DisplayService,
  ImageEncoding,
  InputDeliveryPath,
  InputPolicy,
  InputService,
  intersect,
  ScrollUntilStopReason,
  serveMockDaemon,
  TextRecognitionService,
  WindowClickStrategy,
  WindowService,
} from '@auv-js/sdk'
import { rgbaToThumbHash } from 'thumbhash'

// Serves the mock desktop through the Runner API, with the Runner's rules:
// positions in any coordinate space, input inside the target window, global
// clicks without window options, and captures held by reference. Scripts,
// `auv.*` bindings, call recording and replay then run the same code paths as
// on a device. See docs/ai/references/session-api/2026-10-08-mock-runner-design.md.
// TODO(mock-runner-tasks): seeded scene variation, task goal checks and
// episode export (the benchmark direction in that design) are not built yet.

/** The Device the mock daemon reports. */
export const MOCK_DEVICE = { id: 'mock-desktop', name: 'Mock desktop' }

// gRPC status codes.
const NOT_FOUND = 5
const INVALID_ARGUMENT = 3
const UNIMPLEMENTED = 12

/**
 * NOTICE(mock-capture-store): like the Runner's capture store, keep only
 * recent captures; live mode captures a display every 900 ms.
 */
const KEPT_CAPTURES = 64

/** Latency per kind of call, so the time strip and stepping look live. */
const LATENCY_MS = { capture: 35, click: 25, key: 15, list: 5, recognize: 100, scroll: 20 }

/** Encoded image MIME types by `ImageEncoding`; RGBA is raw rows. */
const IMAGE_TYPES: Partial<Record<ImageEncoding, string>> = {
  [ImageEncoding.JPEG]: 'image/jpeg',
  [ImageEncoding.PNG]: 'image/png',
  [ImageEncoding.WEBP]: 'image/webp',
}

type PositionMessage = undefined | { coordinateSpace: { case: string | undefined, value?: unknown }, x: number, y: number }

interface StoredCapture {
  bounds: Rect
  canvas: OffscreenCanvas
}

/** A Runner transport over `desktop`, with a mock daemon that reports one Device. */
export function mockRunner(desktop: MockDesktop): Transport {
  const captures = new Map<string, StoredCapture>()
  let captured = 0

  const capture = (bounds: Rect, scale: number, origin: undefined | { case: 'displayId' | 'windowId', value: string }) => {
    const canvas = desktop.render(bounds, scale)
    const captureId = `mock-cap-${++captured}`
    captures.set(captureId, { bounds: { ...bounds }, canvas })
    for (const old of captures.keys()) {
      if (captures.size <= KEPT_CAPTURES)
        break
      captures.delete(old)
    }
    return {
      backend: 'mock',
      bounds,
      origin: origin && { coordinateSpace: origin, x: 0, y: 0 },
      pixelSize: { height: canvas.height, width: canvas.width },
      ref: { captureId },
      scaleFactor: scale,
      thumbhash: thumbHashOf(canvas),
    }
  }
  const stored = (captureId: string | undefined): StoredCapture => {
    const found = captureId ? captures.get(captureId) : undefined
    if (!found)
      throw new AuvRpcError(NOT_FOUND, `capture ${captureId ?? ''} was not found: it was evicted; capture again`)
    return found
  }
  const window = (windowId: string | undefined): WindowInfo => {
    const found = desktop.windows().find(candidate => candidate.id === windowId)
    if (!found)
      throw new AuvRpcError(NOT_FOUND, `unknown Window: ${windowId ?? ''}`)
    return found
  }
  const display = (selector: undefined | { selector: { case: string | undefined, value?: unknown } }): DisplayInfo => {
    const displays = desktop.displays()
    const wanted = selector?.selector
    const found = wanted?.case === 'display'
      ? displays.find(candidate => candidate.id === (wanted.value as { displayId: string }).displayId)
      : wanted?.case === 'name'
        ? displays.find(candidate => candidate.name === wanted.value)
        : displays.find(candidate => candidate.primary)
    if (!found)
      throw new AuvRpcError(NOT_FOUND, `no display matches ${JSON.stringify(wanted?.value ?? 'the primary display')}`)
    return found
  }
  const scaleOf = (resolution: CaptureResolution, bounds: Rect) => resolution === CaptureResolution.LOGICAL ? 1 : desktop.displayAt(bounds).scale
  const screenPoint = (position: PositionMessage): Point => {
    if (position?.coordinateSpace.case === 'screen')
      return { x: position.x, y: position.y }
    if (position?.coordinateSpace.case === 'displayId') {
      const origin = display({ selector: { case: 'display', value: { displayId: position.coordinateSpace.value } } }).frame
      return { x: origin.x + position.x, y: origin.y + position.y }
    }
    throw new AuvRpcError(INVALID_ARGUMENT, 'position requires an explicit screen or display coordinate space')
  }
  /** A position in `target`'s space, checked to lie inside it, like the Runner. */
  const windowPoint = (target: WindowInfo, position: PositionMessage): Point => {
    let point: Point
    if (position?.coordinateSpace.case === 'windowId') {
      if (position.coordinateSpace.value !== target.id)
        throw new AuvRpcError(INVALID_ARGUMENT, `position is in Window ${String(position.coordinateSpace.value)}, but the target is Window ${target.id}`)
      point = { x: position.x, y: position.y }
    }
    else {
      const screen = screenPoint(position)
      point = { x: screen.x - target.frame.x, y: screen.y - target.frame.y }
    }
    if (!contains({ height: target.frame.height, width: target.frame.width, x: 0, y: 0 }, point))
      throw new AuvRpcError(INVALID_ARGUMENT, 'point must be inside the current Window bounds')
    return point
  }
  /** The screen area a search reads: a screen region, or fractions of `bounds`. */
  const searchArea = (bounds: Rect, screenRegion: Rect | undefined, region: Rect | undefined): Rect | undefined => {
    if (screenRegion) {
      const area = intersect(screenRegion, bounds)
      if (!area)
        throw new AuvRpcError(INVALID_ARGUMENT, 'screen_region does not overlap the image')
      return area
    }
    return region && { height: region.height * bounds.height, width: region.width * bounds.width, x: bounds.x + region.x * bounds.width, y: bounds.y + region.y * bounds.height }
  }
  const matchesOf = (bounds: Rect, area: Rect | undefined, query: string) =>
    desktop.text(bounds, area).filter(match => match.text.toLowerCase().includes(query.toLowerCase())).map(textMatch)

  return createMockTransport((router: MockRouter) => {
    const { service } = router
    serveMockDaemon(router, MOCK_DEVICE)

    service(DisplayService, {
      listDisplays: () => ({ displays: desktop.displays().map(displayMessage) }),
    })

    service(WindowService, {
      listWindows: () => ({ windows: desktop.windows().map(windowMessage) }),
      resolveWindow: async (request) => {
        await delay(LATENCY_MS.list)
        const application = request.selector?.application
        const title = request.selector?.window
        const found = desktop.windows().find(candidate =>
          (application?.case !== 'applicationBundleId' || candidate.bundleId === application.value)
          && (application?.case !== 'applicationName' || candidate.app === application.value)
          && (application?.case !== 'processId' || candidate.pid === application.value)
          && (title?.case !== 'titleExact' || candidate.title === title.value)
          && (title?.case !== 'titleContains' || (candidate.title ?? '').includes(title.value)))
        if (!found)
          throw new AuvRpcError(NOT_FOUND, `no mock window matches ${JSON.stringify(request.selector ?? {})}`)
        return { window: windowMessage(found) }
      },
    })

    service(CaptureService, {
      captureDisplay: async (request) => {
        await delay(LATENCY_MS.capture)
        const target = display(request.selector)
        return { capture: capture(target.frame, scaleOf(request.resolution, target.frame), { case: 'displayId', value: target.id }), display: displayMessage(target) }
      },
      captureRegion: async (request) => {
        await delay(LATENCY_MS.capture)
        const region = request.region ?? { height: 0, width: 0, x: 0, y: 0 }
        const target = request.selector ? display(request.selector) : desktop.displayAt(region)
        return { capture: capture(region, scaleOf(request.resolution, region), undefined), display: displayMessage(target) }
      },
      captureWindow: async (request) => {
        await delay(LATENCY_MS.capture)
        const target = window(request.window?.windowId)
        return { capture: capture(target.frame, scaleOf(request.resolution, target.frame), { case: 'windowId', value: target.id }), window: windowMessage(target) }
      },
      getCaptureImage: async (request) => {
        const source = stored(request.capture?.captureId)
        const area = searchArea(source.bounds, request.screenRegion, request.region) ?? source.bounds
        const scale = source.canvas.width / source.bounds.width
        const crop = { height: area.height * scale, width: area.width * scale, x: (area.x - source.bounds.x) * scale, y: (area.y - source.bounds.y) * scale }
        const max = request.maxSize && request.maxSize.width > 0 && request.maxSize.height > 0 ? request.maxSize : undefined
        const fit = max ? Math.min(1, max.width / crop.width, max.height / crop.height) : 1
        const out = new OffscreenCanvas(Math.max(1, Math.round(crop.width * fit)), Math.max(1, Math.round(crop.height * fit)))
        const ctx = out.getContext('2d')!
        ctx.drawImage(source.canvas, crop.x, crop.y, crop.width, crop.height, 0, 0, out.width, out.height)
        const encoding = request.encoding === ImageEncoding.UNSPECIFIED ? ImageEncoding.RGBA : request.encoding
        const data = encoding === ImageEncoding.RGBA
          ? new Uint8Array(ctx.getImageData(0, 0, out.width, out.height).data.buffer)
          : new Uint8Array(await (await out.convertToBlob({ type: IMAGE_TYPES[encoding] ?? 'image/png' })).arrayBuffer())
        return { image: { data, encoding, height: out.height, width: out.width } }
      },
    })

    service(TextRecognitionService, {
      findDisplayText: async (request) => {
        await delay(LATENCY_MS.recognize)
        const target = display(request.selector)
        const frame = capture(target.frame, target.scale, { case: 'displayId', value: target.id })
        return { capture: frame, display: displayMessage(target), matches: matchesOf(target.frame, searchArea(target.frame, request.screenRegion, request.region), request.query) }
      },
      findWindowText: async (request) => {
        await delay(LATENCY_MS.recognize)
        const target = window(request.window?.windowId)
        const frame = capture(target.frame, desktop.displayAt(target.frame).scale, { case: 'windowId', value: target.id })
        return { capture: frame, matches: matchesOf(target.frame, searchArea(target.frame, request.screenRegion, request.region), request.query), window: windowMessage(target) }
      },
      recognizeText: async (request) => {
        await delay(LATENCY_MS.recognize)
        // TODO(mock-runner-image-ocr): caller-owned images (`source.image`)
        // have no scene to read text from; recognize them when a mock test needs it.
        if (request.source.case !== 'captureRef')
          throw new AuvRpcError(UNIMPLEMENTED, 'the mock Runner recognizes text in its own captures only')
        const source = stored(request.source.value.captureId)
        return recognized(desktop.text(source.bounds, searchArea(source.bounds, request.screenRegion, request.region)))
      },
    })

    service(InputService, {
      clickPoint: async (request) => {
        await delay(LATENCY_MS.click)
        const position = request.position as PositionMessage
        const targetId = request.window?.windowId ?? (position?.coordinateSpace.case === 'windowId' ? String(position.coordinateSpace.value) : undefined)
        if (targetId === undefined) {
          // A global click has no target window, so it must not carry window
          // delivery options the Runner would silently ignore.
          if (request.options?.policy !== undefined && request.options.policy !== InputPolicy.UNSPECIFIED)
            throw new AuvRpcError(INVALID_ARGUMENT, 'options.policy requires a target window; omit it for a screen or display position')
          if (request.options?.windowStrategy !== undefined && request.options.windowStrategy !== WindowClickStrategy.UNSPECIFIED)
            throw new AuvRpcError(INVALID_ARGUMENT, 'options.window_strategy requires a target window; omit it for a screen or display position')
          const point = screenPoint(position)
          desktop.click(point)
          return { action: delivered(InputDeliveryPath.FOREGROUND_SYSTEM_EVENTS), screenPoint: point }
        }
        const target = window(targetId)
        const local = windowPoint(target, position)
        const point = { x: target.frame.x + local.x, y: target.frame.y + local.y }
        desktop.click(point)
        return { action: delivered(InputDeliveryPath.WINDOW_TARGETED_MOUSE), screenPoint: point, window: windowMessage(target), windowPoint: local }
      },
      inputKeyboard: async (request) => {
        const recipient = request.target?.recipient
        if (recipient?.case === 'window')
          window(recipient.value.ref?.windowId)
        else if (recipient?.case === 'applicationBundleId' && !desktop.hasApplication(recipient.value))
          throw new AuvRpcError(NOT_FOUND, `No running application with bundle ID ${recipient.value}`)
        const path = recipient?.case === 'window' ? InputDeliveryPath.WINDOW_TARGETED_KEYBOARD : InputDeliveryPath.FOREGROUND_SYSTEM_EVENTS
        const actions = []
        for (const input of request.inputs) {
          await delay(LATENCY_MS.key)
          if (request.dryRun)
            continue
          if (input.action.case === 'press')
            desktop.pressKey((input.action.value.options?.keys ?? []).join('+'))
          else if (input.action.case === 'typeText')
            desktop.typeText(input.action.value.text)
          else if (input.action.case === 'pasteText')
            desktop.typeText(input.action.value.text)
          actions.push(delivered(input.action.case === 'pasteText' ? InputDeliveryPath.CLIPBOARD_PASTE : path))
        }
        return { actions }
      },
      pressKey: async (request) => {
        await delay(LATENCY_MS.key)
        desktop.pressKey(request.key)
        return { action: delivered(InputDeliveryPath.FOREGROUND_SYSTEM_EVENTS) }
      },
      async* scrollUntil(requests) {
        const iterator = requests[Symbol.asyncIterator]()
        const first = (await iterator.next()).value
        if (first?.event.case !== 'begin')
          throw new AuvRpcError(INVALID_ARGUMENT, 'begin must be the first ScrollUntil event')
        const begin = first.event.value
        const target = window(begin.window?.windowId)
        const local = windowPoint(target, begin.point as PositionMessage)
        const step = begin.step.case === 'instant' ? begin.step.value : begin.step.case === 'motion' ? begin.step.value.total : undefined
        if (!step || (step.deltaX !== 0 && step.deltaY !== 0) || (step.deltaX === 0 && step.deltaY === 0))
          throw new AuvRpcError(INVALID_ARGUMENT, 'scroll-until scrolls along one axis by a non-zero delta')
        const settle = Number(begin.settle?.seconds ?? 0n) * 1000 + (begin.settle?.nanos ?? 0) / 1e6
        const query = begin.condition.case === 'textVisible' ? begin.condition.value.query : undefined
        const action = delivered(InputDeliveryPath.WINDOW_TARGETED_WHEEL)
        let streak = 0
        for (let steps = 1; ; steps++) {
          await delay(LATENCY_MS.scroll)
          const moved = desktop.scroll(target.id, local, { dx: step.deltaX, dy: step.deltaY })
          await delay(Math.min(settle, 60))
          const frame = capture(target.frame, desktop.displayAt(target.frame).scale, { case: 'windowId', value: target.id })
          const regions = desktop.text(target.frame)
          streak = moved ? 0 : streak + 1
          const match = query ? regions.find(region => region.text.toLowerCase().includes(query.toLowerCase())) : undefined
          const stop = match
            ? ScrollUntilStopReason.TEXT_VISIBLE
            : streak >= begin.noMotionConfirmations
              ? ScrollUntilStopReason.END_BY_NO_VISUAL_PROGRESS
              : steps >= begin.maxSteps ? ScrollUntilStopReason.BUDGET_EXHAUSTED : ScrollUntilStopReason.UNSPECIFIED
          const motion = { estimatedShift: moved ? Math.round(step.deltaY || step.deltaX) : 0, noMotion: !moved, normalizedDiff: moved ? 1 : 0 }
          const total = { deltaX: step.deltaX * steps, deltaY: step.deltaY * steps }
          const awaitingDecision = begin.awaitDecisions && stop === ScrollUntilStopReason.UNSPECIFIED
          yield {
            event: {
              case: 'update',
              value: { awaitingDecision, capture: frame, delivered: total, motion, noMotionStreak: streak, steps, stop, text: begin.output?.omitText ? undefined : recognized(regions) },
            },
          }
          const completed = (reason: ScrollUntilStopReason) => ({
            event: { case: 'completed' as const, value: { action, delivered: total, lastMotion: motion, reason, steps, textMatch: match && { bounds: match.bounds, text: match.text } } },
          })
          if (stop !== ScrollUntilStopReason.UNSPECIFIED) {
            yield completed(stop)
            return
          }
          if (awaitingDecision) {
            const next = (await iterator.next()).value
            if (next?.event.case === 'decision' && next.event.value.stop) {
              yield completed(ScrollUntilStopReason.PREDICATE_SATISFIED)
              return
            }
          }
        }
      },
      scrollWindowPoint: async (request) => {
        await delay(LATENCY_MS.scroll)
        const target = window(request.window?.windowId)
        const local = windowPoint(target, request.point as PositionMessage)
        const scroll = request.scroll ?? { deltaX: 0, deltaY: 0 }
        desktop.scroll(target.id, local, { dx: scroll.deltaX, dy: scroll.deltaY })
        return { action: delivered(InputDeliveryPath.WINDOW_TARGETED_WHEEL), point: local, scroll, window: windowMessage(target) }
      },
      typeText: async (request) => {
        await delay(LATENCY_MS.key * Math.max(1, request.text.length / 4))
        desktop.typeText(request.text)
        return { action: delivered(InputDeliveryPath.FOREGROUND_SYSTEM_EVENTS) }
      },
    })

    service(ApplicationService, {
      activateBundleId: async (request) => {
        await delay(LATENCY_MS.key)
        if (!desktop.hasApplication(request.bundleId))
          throw new AuvRpcError(NOT_FOUND, `No running application with bundle ID ${request.bundleId}`)
        return { requestedBundleId: request.bundleId, verification: { verification: { case: 'verifiedForeground', value: { observedBundleId: request.bundleId } } } }
      },
    })
  })
}

async function delay(ms: number): Promise<void> {
  await new Promise(resolve => setTimeout(resolve, ms))
}

function delivered(path: InputDeliveryPath) {
  return { attempts: [{ path, succeeded: true }], selectedPath: path }
}

function displayMessage(display: DisplayInfo) {
  return { displayId: display.id, frame: display.frame, name: display.name, primary: display.primary, scaleFactor: display.scale }
}

function recognized(regions: TextMatch[]) {
  return { regions: regions.map(textMatch), text: regions.map(region => region.text).join('\n') }
}

function textMatch(match: TextMatch) {
  return { bounds: match.bounds, confidence: match.confidence, text: match.text }
}

/** A ThumbHash of a capture, like the Runner's: at most 100×100 pixels in. */
function thumbHashOf(canvas: OffscreenCanvas): Uint8Array {
  const fit = Math.min(1, 100 / Math.max(canvas.width, canvas.height))
  const small = new OffscreenCanvas(Math.max(1, Math.round(canvas.width * fit)), Math.max(1, Math.round(canvas.height * fit)))
  const ctx = small.getContext('2d')!
  ctx.drawImage(canvas, 0, 0, small.width, small.height)
  return rgbaToThumbHash(small.width, small.height, ctx.getImageData(0, 0, small.width, small.height).data)
}

function windowMessage(window: WindowInfo) {
  return {
    applicationBundleId: window.bundleId,
    applicationName: window.app,
    frame: window.frame,
    isMain: true,
    isVisible: true,
    processId: window.pid,
    ref: { windowId: window.id },
    title: window.title,
  }
}
