import type {
  DescMessage,
  DescMethodBiDiStreaming,
  DescMethodServerStreaming,
  DescMethodUnary,
  MessageInitShape,
  MessageShape,
} from '@bufbuild/protobuf'
import type { DurationSchema } from '@bufbuild/protobuf/wkt'

import type { FocusTextRequestSchema } from '../../gen/auv/api/driver/macos/v1/accessibility_pb'
import type { ActivateBundleIdRequestSchema } from '../../gen/auv/api/driver/macos/v1/application_pb'
import type { CaptureRefSchema, GetCaptureImageRequestSchema, ImageFrameSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import type { Display, DisplaySelectorSchema } from '../../gen/auv/api/driver/v1/display_pb'
import type { ScreenPointSchema, ScreenRectSchema, WindowPointSchema } from '../../gen/auv/api/driver/v1/geometry_pb'
import type {
  ClickOptionsSchema,
  MoveMouseRequestSchema,
  MoveMouseStreamResponse,
  PasteTextOptionsSchema,
  ScreenClickOptionsSchema,
  ScrollMotionSchema,
  ScrollOptionsSchema,
  ScrollSchema,
  ScrollUntilBeginSchema,
  ScrollUntilCompleted,
  ScrollUntilObservation,
  ScrollVelocitySchema,
  ScrollWindowPointMotionResponse,
  StreamScrollBeginSchema,
  StreamScrollCompleted,
  StreamScrollResponse,
  TypeTextOptionsSchema,
} from '../../gen/auv/api/driver/v1/input_pb'
import type { ShowOverlayRequestSchema } from '../../gen/auv/api/driver/v1/overlay_pb'
import type {
  FindDisplayTextRequestSchema,
  FindWindowTextRequestSchema,
  RecognizeTextRequestSchema,
} from '../../gen/auv/api/driver/v1/text_recognition_pb'
import type { Window, WindowRef, WindowSelectorSchema } from '../../gen/auv/api/driver/v1/window_pb'
import type { ImageEncoding } from '../../gen/auv/api/image/v1/image_pb'
import type { AuvConnection, TypedDuplexCall } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import { create } from '@bufbuild/protobuf'

import { AccessibilityService } from '../../gen/auv/api/driver/macos/v1/accessibility_pb'
import { ApplicationService } from '../../gen/auv/api/driver/macos/v1/application_pb'
import { MediaControlService } from '../../gen/auv/api/driver/macos/v1/media_control_pb'
import { PermissionService } from '../../gen/auv/api/driver/macos/v1/permission_pb'
import { CaptureService } from '../../gen/auv/api/driver/v1/capture_pb'
import { DisplayService } from '../../gen/auv/api/driver/v1/display_pb'
import { InputService } from '../../gen/auv/api/driver/v1/input_pb'
import { OverlayService } from '../../gen/auv/api/driver/v1/overlay_pb'
import { TextRecognitionService } from '../../gen/auv/api/driver/v1/text_recognition_pb'
import { WindowSchema, WindowService } from '../../gen/auv/api/driver/v1/window_pb'
import { AuvProtocolError, AuvRpcError } from '../../transport/errors'
import { invokeDuplex, invokeServerStream, invokeUnary } from './invoke'

/** Pixels fetched from the Runner. `data` is RGBA8 rows for `ImageEncoding.RGBA`, otherwise PNG, JPEG or lossless WebP bytes. */
export interface CaptureImage {
  data: Uint8Array
  encoding: ImageEncoding
  height: number
  width: number
}
/** How `captures.image` shapes pixels: crop to `region`, fit inside `maxSize`, then encode (RGBA by default). */
export interface CaptureImageOptions extends InputFields<typeof GetCaptureImageRequestSchema, 'capture'>, OperationOptions {}
/**
 * A capture held by the Runner: its ID, its `CaptureRef`, or a `CapturedFrame`
 * returned by a capture, find-text, or scroll-until call (structurally typed,
 * so plain objects work).
 */
export type CaptureTarget = string | { captureId: string } | { ref?: { captureId: string } }

export interface FindDisplayTextOptions extends InputFields<typeof FindDisplayTextRequestSchema, 'query' | 'selector'>, OperationOptions {}

export interface FindWindowTextOptions extends InputFields<typeof FindWindowTextRequestSchema, 'query' | 'window'>, OperationOptions {}

export interface PressKeyOptions extends OperationOptions {
  settle?: Init<typeof DurationSchema>
}

/** What `recognizeText` reads: a Runner-held capture, or a caller-owned image (`frame`, pixels are sent). */
export type RecognitionSource = CaptureTarget | { frame: Init<typeof ImageFrameSchema> }

export interface RecognizeTextOptions extends InputFields<typeof RecognizeTextRequestSchema, 'source'>, OperationOptions {}

export interface RunnerClient {
  /**
   * Explicit pixel access. Captures return a reference and metadata; pixels
   * leave the Runner only through this call. A reference fails with
   * NOT_FOUND once the Runner evicted or expired it.
   */
  readonly captures: {
    image: (capture: CaptureTarget, options?: CaptureImageOptions) => Promise<CaptureImage>
  }
  readonly displays: {
    capture: (selector?: Init<typeof DisplaySelectorSchema>, options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureDisplay.output>>
    captureRegion: (region: Init<typeof ScreenRectSchema>, selector?: Init<typeof DisplaySelectorSchema>, options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureRegion.output>>
    findText: (selector: Init<typeof DisplaySelectorSchema> | undefined, query: string, options?: FindDisplayTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findDisplayText.output>>
    list: (options?: OperationOptions) => Promise<readonly Display[]>
  }
  readonly input: {
    clickScreenPoint: (point: Init<typeof ScreenPointSchema>, clickOptions: Init<typeof ScreenClickOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickScreenPoint.output>>
    createMouse: (request: Init<typeof InputService.method.createMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.createMouse.output>>
    dragMouse: (request: Init<typeof InputService.method.dragMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.dragMouse.output>>
    holdKeys: (request: Init<typeof InputService.method.holdKeys.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.holdKeys.output>>
    keyDown: (request: Init<typeof InputService.method.keyDown.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.keyDown.output>>
    keyUp: (request: Init<typeof InputService.method.keyUp.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.keyUp.output>>
    mouseDown: (request: Init<typeof InputService.method.mouseDown.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.mouseDown.output>>
    mouseUp: (request: Init<typeof InputService.method.mouseUp.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.mouseUp.output>>
    moveMouse: (request: Init<typeof MoveMouseRequestSchema>, options?: OperationOptions) => Promise<AsyncIterable<MoveMouseStreamResponse>>
    pasteText: (text: string, inputOptions?: Init<typeof PasteTextOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.pasteText.output>>
    pressKey: (key: string, options?: PressKeyOptions) => Promise<Shape<typeof InputService.method.pressKey.output>>
    removeMouse: (request: Init<typeof InputService.method.removeMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.removeMouse.output>>
    streamMouseMotion: (options?: OperationOptions) => Promise<TypedDuplexCall<typeof InputService.method.streamMouseMotion.input, typeof InputService.method.streamMouseMotion.output>>
    typeText: (text: string, inputOptions?: Init<typeof TypeTextOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.typeText.output>>
  }
  readonly macos: {
    readonly accessibility: {
      focusText: (request: Init<typeof FocusTextRequestSchema>, options?: OperationOptions) => Promise<Shape<typeof AccessibilityService.method.focusText.output>>
    }
    readonly applications: {
      activateBundleId: (request: Init<typeof ActivateBundleIdRequestSchema>, options?: OperationOptions) => Promise<Shape<typeof ApplicationService.method.activateBundleId.output>>
    }
    readonly media: {
      nextTrack: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.nextTrack.output>>
      nowPlaying: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.getNowPlaying.output>>
      pause: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.pause.output>>
      play: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.play.output>>
      previousTrack: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.previousTrack.output>>
      togglePlayPause: (options?: OperationOptions) => Promise<Shape<typeof MediaControlService.method.togglePlayPause.output>>
    }
    readonly permissions: {
      probe: (options?: OperationOptions) => Promise<Shape<typeof PermissionService.method.probePermissions.output>>
    }
  }
  readonly overlay: {
    remove: (options?: OperationOptions) => Promise<void>
    show: (request: Init<typeof ShowOverlayRequestSchema>, options?: OperationOptions) => Promise<void>
  }
  /** Runs OCR on a Runner-held capture (no pixels travel) or on `{ frame }`, a caller-owned image. */
  recognizeText: (source: RecognitionSource, options?: RecognizeTextOptions) => Promise<Shape<typeof TextRecognitionService.method.recognizeText.output>>
  readonly windows: {
    /**
     * A client for a window on this route, without a call. Takes a client from any route
     * (for example an earlier Run), a listed or resolved `Window`, a
     * `WindowRef`, or a window ID. Window references are Device resources,
     * not Run resources, so this is how a window outlives a Run.
     */
    from: (target: WindowTarget) => WindowClient
    /**
     * The window `target` names, from a fresh listing and bound to this route.
     * Rejects with an `AuvRpcError` (`rpcCode` 5, NOT_FOUND) once it is gone.
     */
    get: (target: WindowTarget, options?: OperationOptions) => Promise<WindowClient>
    /** Lists windows as ready-to-use clients carrying their metadata. */
    list: (options?: OperationOptions) => Promise<readonly WindowClient[]>
    resolve: (selector: Init<typeof WindowSelectorSchema>, options?: OperationOptions) => Promise<WindowClient>
  }
}

export interface RunnerRouteOptions extends OperationOptions {
  deviceId?: string
  runId?: string
  runnerClass: string
}
/** Begin parameters for `scrollStream`; the window comes from the client. */
export type ScrollStreamBegin = InputFields<typeof StreamScrollBeginSchema, 'window'>

export interface ScrollStreamController {
  cancel: () => Promise<void>
  /** Started, progress (latest value), and completed events. */
  readonly events: AsyncIterable<StreamScrollResponse>
  setVelocity: (velocity: Init<typeof ScrollVelocitySchema>) => Promise<void>
  stop: () => Promise<void>
}

export interface ScrollUntilCallOptions extends OperationOptions {
  /** Receives every observation in order, including the last one. */
  onObservation?: (observation: ScrollUntilObservation) => Promise<void> | void
  /**
   * Client-side stop predicate. Returning `true` stops the loop with reason
   * `predicateSatisfied`. The Runner waits for each answer, and does not ask
   * about observations it already ends itself (`observation.stop`).
   */
  until?: (observation: ScrollUntilObservation) => boolean | Promise<boolean>
}

/**
 * `scrollUntil` begin fields; the window, point, and decision mode come from
 * the call. Without a `condition`, the loop stops at the end (no visual motion).
 * Observations carry the capture by reference (fetch pixels with
 * `captures.image`) and the recognized text unless `observe.omitText` is set.
 */
export type ScrollUntilOptions = InputFields<typeof ScrollUntilBeginSchema, 'awaitDecisions' | 'point' | 'window'>

/** One generator step for `scrollWith`. Velocities are logical px/s. */
export interface ScrollWithStep {
  holdMs?: number
  velocityX?: number
  velocityY?: number
}

export interface WindowClient {
  capture: (options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureWindow.output>>
  click: (point: Init<typeof WindowPointSchema>, clickOptions?: Init<typeof ClickOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickWindowPoint.output>>
  findText: (query: string, options?: FindWindowTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findWindowText.output>>
  /** Window ID; the same as `window.ref.windowId`. */
  readonly id: string
  /**
   * Wheel-scrolls at a window-local point. Deltas are logical pixels: positive
   * `deltaY` scrolls toward later content (down) and positive `deltaX` scrolls
   * right, like DOM `WheelEvent`. The result is delivery evidence only.
   */
  scroll: (point: Init<typeof WindowPointSchema>, scroll: Init<typeof ScrollSchema>, scrollOptions?: Init<typeof ScrollOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.scrollWindowPoint.output>>
  /**
   * Spreads one scroll over time with a timing function and streams
   * `started`, `progress`, and `completed` events. Aborting `options.signal`
   * stops the remaining samples. Totals use the same logical-pixel,
   * positive-down convention as `scroll`.
   */
  scrollMotion: (point: Init<typeof WindowPointSchema>, motion: Init<typeof ScrollMotionSchema>, scrollOptions?: Init<typeof ScrollOptionsSchema>, options?: OperationOptions) => Promise<AsyncIterable<ScrollWindowPointMotionResponse>>
  /**
   * Opens a live scroll stream. Delivery starts at zero velocity; steer it
   * with `setVelocity` (logical px/s, positive = down/right). Every update
   * renews `begin.lease`; without renewal the stream ramps to zero and
   * completes. Aborting `options.signal` disconnects and stops delivery.
   */
  scrollStream: (begin: ScrollStreamBegin, options?: OperationOptions) => Promise<ScrollStreamController>
  /**
   * Scrolls in steps and observes after each step on the Runner until no
   * visual motion remains, the query text appears (`condition.case ===
   * 'textVisible'`), `options.until` returns `true`, or `maxSteps` runs out.
   * Resolves with the completion. An end stop means no visual progress was
   * observed, not proof that no more content exists.
   */
  scrollUntil: (point: Init<typeof WindowPointSchema>, request: ScrollUntilOptions, options?: ScrollUntilCallOptions) => Promise<ScrollUntilCompleted>
  /**
   * Drives a live scroll stream from a (possibly async) generator. Each yielded
   * step sets the velocity and holds it for `holdMs` (default 100), renewing the
   * lease while it holds. The stream stops (ramping down) when the generator
   * finishes, and resolves with the completion event.
   */
  scrollWith: (steps: AsyncIterable<ScrollWithStep> | Iterable<ScrollWithStep>, begin: ScrollStreamBegin, options?: OperationOptions) => Promise<StreamScrollCompleted>
  /**
   * Metadata from the call that produced this client (`resolve` or `list`),
   * or just the reference for a client bound with `windows.client`. It is a
   * snapshot: the Runner re-resolves the window before every operation.
   */
  readonly window: Window
}
// gRPC status code NOT_FOUND.
const GRPC_NOT_FOUND = 5

/** Anything that names one window: a client, a `Window`, a `WindowRef`, or a window ID. */
export type WindowTarget = string | Window | WindowClient | WindowRef

type Init<T extends DescMessage> = MessageInitShape<T>
type InputFields<T extends DescMessage, K extends keyof Init<T>> = Omit<Init<T>, '$typeName' | K>

type Shape<T extends DescMessage> = MessageShape<T>

/** Binds first-party Driver capabilities to one existing Runner route. */
export function createRunnerClient(connection: AuvConnection, route: RunnerRouteOptions): RunnerClient {
  const callOptions = (options?: OperationOptions): OperationOptions => ({
    signal: combineSignals(route.signal, options?.signal),
  })
  const unary = <I extends DescMessage, O extends DescMessage>(
    method: DescMethodUnary<I, O>,
    request: MessageInitShape<I>,
    options?: OperationOptions,
  ) => invokeUnary(connection, {
    ...route,
    input: method.input,
    method: method.name,
    output: method.output,
    request,
    service: method.parent.typeName,
    ...callOptions(options),
  })
  const serverStream = <I extends DescMessage, O extends DescMessage>(
    method: DescMethodServerStreaming<I, O>,
    request: MessageInitShape<I>,
    options?: OperationOptions,
  ) => invokeServerStream(connection, {
    ...route,
    input: method.input,
    method: method.name,
    output: method.output,
    request,
    service: method.parent.typeName,
    ...callOptions(options),
  })
  const duplex = <I extends DescMessage, O extends DescMessage>(
    method: DescMethodBiDiStreaming<I, O>,
    options?: OperationOptions,
  ) => invokeDuplex(connection, {
    ...route,
    input: method.input,
    method: method.name,
    output: method.output,
    service: method.parent.typeName,
    ...callOptions(options),
  })
  const openScrollStream = async (windowId: string, begin: ScrollStreamBegin, options?: OperationOptions): Promise<ScrollStreamController> => {
    const call = await duplex(InputService.method.streamScroll, options)
    await call.send({ event: { case: 'begin', value: { ...begin, window: { windowId } } } })
    return {
      cancel: () => call.send({ event: { case: 'cancel', value: {} } }),
      events: call.responses,
      setVelocity: velocity => call.send({ event: { case: 'setVelocity', value: { velocity } } }),
      stop: () => call.send({ event: { case: 'stop', value: {} } }),
    }
  }
  const window = (metadata: Window): WindowClient => {
    const id = metadata.ref?.windowId
    if (id === undefined || id.length === 0)
      throw new AuvProtocolError('Window omitted ref.windowId')
    return {
      capture: options => unary(CaptureService.method.captureWindow, { window: { windowId: id } }, options),
      click: (point, clickOptions, options) => unary(InputService.method.clickWindowPoint, {
        options: clickOptions,
        point,
        window: { windowId: id },
      }, options),
      findText: (query, options = {}) => {
        const { signal, ...request } = options
        return unary(TextRecognitionService.method.findWindowText, {
          ...request,
          query,
          window: { windowId: id },
        }, { signal })
      },
      id,
      scroll: (point, scroll, scrollOptions, options) => unary(InputService.method.scrollWindowPoint, {
        options: scrollOptions,
        point,
        scroll,
        window: { windowId: id },
      }, options),
      scrollMotion: (point, motion, scrollOptions, options) => serverStream(InputService.method.scrollWindowPointMotion, {
        motion,
        options: scrollOptions,
        point,
        window: { windowId: id },
      }, options),
      scrollStream: (begin, options) => openScrollStream(id, begin, options),
      scrollUntil: async (point, request, options = {}) => {
        const { onObservation, until, ...operation } = options
        const call = await duplex(InputService.method.scrollUntil, operation)
        const condition = request.condition?.case === undefined ? { case: 'end' as const, value: {} } : request.condition
        await call.send({
          event: {
            case: 'begin',
            value: { ...request, awaitDecisions: until !== undefined, condition, point, window: { windowId: id } },
          },
        })
        for await (const response of call.responses) {
          const event = response.event
          if (event.case === 'completed')
            return event.value
          if (event.case !== 'observation')
            continue
          await onObservation?.(event.value)
          if (event.value.awaitingDecision)
            await call.send({ event: { case: 'decision', value: { stop: await until?.(event.value) ?? true } } })
        }
        throw new Error('ScrollUntil ended without a completion event')
      },
      scrollWith: async (steps, begin, options) => {
        const controller = await openScrollStream(id, begin, options)
        const completion = (async () => {
          for await (const response of controller.events) {
            if (response.event.case === 'completed')
              return response.event.value
          }
          throw new Error('StreamScroll ended without a completion event')
        })()
        const leaseMs = durationMilliseconds(begin.lease)
        for await (const step of steps) {
          const velocity = { deltaXPerSecond: step.velocityX ?? 0, deltaYPerSecond: step.velocityY ?? 0 }
          await controller.setVelocity(velocity)
          let remaining = step.holdMs ?? 100
          // Renew the lease while a step holds longer than half of it.
          while (remaining > 0) {
            const wait = Math.min(remaining, Math.max(leaseMs / 2, 1))
            await delay(wait, options?.signal)
            remaining -= wait
            if (remaining > 0)
              await controller.setVelocity(velocity)
          }
        }
        await controller.stop()
        return await completion
      },
      window: metadata,
    }
  }

  return {
    captures: {
      image: async (capture, options = {}) => {
        const { signal, ...request } = options
        const response = await unary(CaptureService.method.getCaptureImage, { ...request, capture: captureRefOf(capture) }, { signal })
        if (!response.image)
          throw new AuvProtocolError('GetCaptureImageResponse omitted image')
        return response.image
      },
    },
    displays: {
      capture: (selector, options) => unary(CaptureService.method.captureDisplay, { selector }, options),
      captureRegion: (region, selector, options) => unary(CaptureService.method.captureRegion, { region, selector }, options),
      findText: (selector, query, options = {}) => {
        const { signal, ...request } = options
        return unary(TextRecognitionService.method.findDisplayText, { ...request, query, selector }, { signal })
      },
      list: async options => (await unary(DisplayService.method.listDisplays, {}, options)).displays,
    },
    input: {
      clickScreenPoint: (point, clickOptions, options) => unary(InputService.method.clickScreenPoint, { options: clickOptions, point }, options),
      createMouse: (request, options) => unary(InputService.method.createMouse, request, options),
      dragMouse: (request, options) => unary(InputService.method.dragMouse, request, options),
      holdKeys: (request, options) => unary(InputService.method.holdKeys, request, options),
      keyDown: (request, options) => unary(InputService.method.keyDown, request, options),
      keyUp: (request, options) => unary(InputService.method.keyUp, request, options),
      mouseDown: (request, options) => unary(InputService.method.mouseDown, request, options),
      mouseUp: (request, options) => unary(InputService.method.mouseUp, request, options),
      moveMouse: (request, options) => serverStream(InputService.method.moveMouse, request, options),
      pasteText: (text, options, operation) => unary(InputService.method.pasteText, { options, text }, operation),
      pressKey: (key, options = {}) => unary(InputService.method.pressKey, { key, settle: options.settle }, options),
      removeMouse: (request, options) => unary(InputService.method.removeMouse, request, options),
      streamMouseMotion: options => duplex(InputService.method.streamMouseMotion, options),
      typeText: (text, options, operation) => unary(InputService.method.typeText, { options, text }, operation),
    },
    macos: {
      accessibility: {
        focusText: (request, options) => unary(AccessibilityService.method.focusText, request, options),
      },
      applications: {
        activateBundleId: (request, options) => unary(ApplicationService.method.activateBundleId, request, options),
      },
      media: {
        nextTrack: options => unary(MediaControlService.method.nextTrack, {}, options),
        nowPlaying: options => unary(MediaControlService.method.getNowPlaying, {}, options),
        pause: options => unary(MediaControlService.method.pause, {}, options),
        play: options => unary(MediaControlService.method.play, {}, options),
        previousTrack: options => unary(MediaControlService.method.previousTrack, {}, options),
        togglePlayPause: options => unary(MediaControlService.method.togglePlayPause, {}, options),
      },
      permissions: {
        probe: options => unary(PermissionService.method.probePermissions, {}, options),
      },
    },
    overlay: {
      async remove(options) {
        await unary(OverlayService.method.removeOverlay, {}, options)
      },
      async show(request, options) {
        await unary(OverlayService.method.showOverlay, request, options)
      },
    },
    recognizeText: (source, options = {}) => {
      const { signal, ...request } = options
      const value = typeof source === 'object' && 'frame' in source
        ? { case: 'image' as const, value: source.frame }
        : { case: 'captureRef' as const, value: captureRefOf(source) }
      return unary(TextRecognitionService.method.recognizeText, { ...request, source: value }, { signal })
    },
    windows: {
      from: target => window(windowOf(target)),
      get: async (target, options) => {
        const id = windowOf(target).ref?.windowId
        const windows = (await unary(WindowService.method.listWindows, {}, options)).windows
        const found = windows.find(candidate => candidate.ref?.windowId === id)
        // Same shape as a Runner NOT_FOUND status, so callers handle both alike.
        if (!found)
          throw new AuvRpcError(GRPC_NOT_FOUND, `window:${id} was not found`)
        return window(found)
      },
      list: async options => (await unary(WindowService.method.listWindows, {}, options)).windows.map(window),
      resolve: async (selector, options) => {
        const response = await unary(WindowService.method.resolveWindow, { selector }, options)
        if (!response.window)
          throw new AuvProtocolError('ResolveWindowResponse omitted window')
        return window(response.window)
      },
    },
  }
}

function captureRefOf(target: CaptureTarget): Init<typeof CaptureRefSchema> {
  if (typeof target === 'string')
    return { captureId: target }
  if ('captureId' in target)
    return { captureId: target.captureId }
  if (!target.ref)
    throw new TypeError('CapturedFrame has no capture reference; pass a frame returned by the Runner')
  return { captureId: target.ref.captureId }
}

function combineSignals(first?: AbortSignal, second?: AbortSignal): AbortSignal | undefined {
  if (first === undefined)
    return second
  if (second === undefined)
    return first
  return AbortSignal.any([first, second])
}

async function delay(ms: number, signal?: AbortSignal): Promise<void> {
  signal?.throwIfAborted()
  await new Promise<void>((resolve, reject) => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const abort = () => {
      clearTimeout(timer)
      reject(signal?.reason)
    }
    timer = setTimeout(() => {
      signal?.removeEventListener('abort', abort)
      resolve()
    }, ms)
    signal?.addEventListener('abort', abort, { once: true })
  })
}

function durationMilliseconds(duration: ScrollStreamBegin['lease']): number {
  if (!duration)
    return 0
  return Number(duration.seconds ?? 0n) * 1000 + (duration.nanos ?? 0) / 1_000_000
}

function isWindowClient(target: Exclude<WindowTarget, string>): target is WindowClient {
  return 'window' in target && typeof target.capture === 'function'
}

/** `Window` metadata for any window target; IDs and refs carry only the reference. */
function windowOf(target: WindowTarget): Window {
  if (typeof target === 'string')
    return create(WindowSchema, { ref: { windowId: target } })
  if (isWindowClient(target))
    return target.window
  if ('windowId' in target)
    return create(WindowSchema, { ref: { windowId: target.windowId } })
  return target
}
