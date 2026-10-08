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
import type { CaptureRefSchema, CaptureResolution, GetCaptureImageRequestSchema, ImageFrameSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import type { Display, DisplaySelectorSchema } from '../../gen/auv/api/driver/v1/display_pb'
import type { PositionSchema, ScreenRectSchema } from '../../gen/auv/api/driver/v1/geometry_pb'
import type {
  ClickOptionsSchema,
  InputActionResult,
  InputKeyboardRequestSchema,
  KeyboardInputSchema,
  MoveMouseRequestSchema,
  MoveMouseStreamResponse,
  PasteTextOptionsSchema,
  PressKeysOptionsSchema,
  PressKeysRequestSchema,
  ScrollMotionSchema,
  ScrollOptionsSchema,
  ScrollSchema,
  ScrollUntilBeginSchema,
  ScrollUntilCompleted,
  ScrollUntilUpdate,
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
import { InputPolicy, InputService } from '../../gen/auv/api/driver/v1/input_pb'
import { OverlayService } from '../../gen/auv/api/driver/v1/overlay_pb'
import { TextRecognitionService } from '../../gen/auv/api/driver/v1/text_recognition_pb'
import { WindowSchema, WindowService } from '../../gen/auv/api/driver/v1/window_pb'
import { AuvProtocolError, AuvRpcError } from '../../transport/errors'
import { center } from './geometry'
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
 * Capture call options. `resolution` defaults to native (backing pixels, 2x
 * on Retina); `CaptureResolution.LOGICAL` takes one pixel per point for
 * display and motion checks.
 */
export interface CaptureOptions extends OperationOptions {
  resolution?: CaptureResolution
}
/**
 * A capture held by the Runner: its ID, its `CaptureRef`, or a `CapturedFrame`
 * returned by a capture, find-text, or scroll-until call (structurally typed,
 * so plain objects work).
 */
export type CaptureTarget = string | { captureId: string } | { ref?: { captureId: string } }

export interface FindDisplayTextOptions extends InputFields<typeof FindDisplayTextRequestSchema, 'query' | 'selector'>, OperationOptions {}

export interface FindWindowTextOptions extends InputFields<typeof FindWindowTextRequestSchema, 'query' | 'window'>, OperationOptions {}

/**
 * Where a pointer action lands (*provisional* name):
 *
 * - a `Position`, with an explicit `coordinateSpace` (screen, display or window);
 * - an object with screen-space `bounds`, such as a text match: its center;
 * - a plain `{ x, y }`: window-local for a `WindowClient`, screen space for `input`.
 *
 * Window-scoped calls convert screen and display positions with the window's
 * current frame on the Runner.
 */
export type PointTarget = Init<typeof PositionSchema> | { bounds?: Init<typeof ScreenRectSchema> } | { x: number, y: number }

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
    capture: (selector?: Init<typeof DisplaySelectorSchema>, options?: CaptureOptions) => Promise<Shape<typeof CaptureService.method.captureDisplay.output>>
    captureRegion: (region: Init<typeof ScreenRectSchema>, selector?: Init<typeof DisplaySelectorSchema>, options?: CaptureOptions) => Promise<Shape<typeof CaptureService.method.captureRegion.output>>
    findText: (selector: Init<typeof DisplaySelectorSchema> | undefined, query: string, options?: FindDisplayTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findDisplayText.output>>
    list: (options?: OperationOptions) => Promise<readonly Display[]>
  }
  readonly input: {
    /**
     * Clicks at a point. A window position is delivered to that window under
     * `clickOptions.policy` and `windowStrategy`; a screen or display position
     * is a global click, which takes neither (the Runner rejects them).
     */
    click: (target: PointTarget, clickOptions?: Init<typeof ClickOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickPoint.output>>
    createMouse: (request: Init<typeof InputService.method.createMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.createMouse.output>>
    dragMouse: (request: Init<typeof InputService.method.dragMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.dragMouse.output>>
    holdKeys: (request: Init<typeof InputService.method.holdKeys.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.holdKeys.output>>
    /**
     * Runs ordered keyboard actions (`press`, `typeText`, `pasteText`) on one
     * explicit recipient: a window, an application, or the foreground. The
     * Runner validates every action before delivering any; an error carries
     * how far delivery got. Older Runners reject the call as UNIMPLEMENTED.
     */
    keyboard: (request: Init<typeof InputKeyboardRequestSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.inputKeyboard.output>>
    keyDown: (request: Init<typeof InputService.method.keyDown.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.keyDown.output>>
    keyUp: (request: Init<typeof InputService.method.keyUp.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.keyUp.output>>
    mouseDown: (request: Init<typeof InputService.method.mouseDown.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.mouseDown.output>>
    mouseUp: (request: Init<typeof InputService.method.mouseUp.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.mouseUp.output>>
    moveMouse: (request: Init<typeof MoveMouseRequestSchema>, options?: OperationOptions) => Promise<AsyncIterable<MoveMouseStreamResponse>>
    pasteText: (text: string, inputOptions?: Init<typeof PasteTextOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.pasteText.output>>
    /** Legacy: presses a key or shortcut string in whichever app has keyboard focus. Prefer `pressKeys`. */
    pressKey: (key: string, options?: PressKeyOptions) => Promise<Shape<typeof InputService.method.pressKey.output>>
    /** One key combination, optionally repeated, on an explicit recipient; see `keyboard`. */
    pressKeys: (request: Init<typeof PressKeysRequestSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.pressKeys.output>>
    removeMouse: (request: Init<typeof InputService.method.removeMouse.input>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.removeMouse.output>>
    streamMouseMotion: (options?: OperationOptions) => Promise<TypedDuplexCall<typeof InputService.method.streamMouseMotion.input, typeof InputService.method.streamMouseMotion.output>>
    /**
     * Types into whichever app has keyboard focus when the call arrives. To
     * type into a specific window, use `WindowClient.typeText` or `keyboard`.
     */
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
    /**
     * The window a selector names, such as `{ bundleId: 'com.apple.TextEdit' }`
     * for that app's main visible window. Takes a `WindowQuery` or the full
     * `WindowSelector`.
     */
    resolve: (selector: Init<typeof WindowSelectorSchema> | WindowQuery, options?: OperationOptions) => Promise<WindowClient>
  }
}

export interface RunnerRouteOptions extends OperationOptions {
  deviceId?: string
  runId?: string
  runnerClass: string
}

/** Begin parameters for `scrollStream`; the window comes from the client. */
export type ScrollStreamBegin = InputFields<typeof StreamScrollBeginSchema, 'point' | 'window'> & { point: PointTarget }

export interface ScrollStreamController {
  cancel: () => Promise<void>
  /** Started, progress (latest value), and completed events. */
  readonly events: AsyncIterable<StreamScrollResponse>
  setVelocity: (velocity: Init<typeof ScrollVelocitySchema>) => Promise<void>
  stop: () => Promise<void>
}
export interface ScrollUntilCallOptions extends OperationOptions {
  /** Receives every update in order, including the last one. */
  onUpdate?: (update: ScrollUntilUpdate) => Promise<void> | void
  /**
   * Client-side stop predicate. Returning `true` stops the loop with reason
   * `predicateSatisfied`. The Runner waits for each answer, and does not ask
   * about updates it already ends itself (`update.stop`).
   */
  until?: (update: ScrollUntilUpdate) => boolean | Promise<boolean>
}

/**
 * `scrollUntil` begin fields; the window, point, and decision mode come from
 * the call. Without a `condition`, the loop stops at the end (no visual motion).
 * Updates carry the capture by reference (fetch pixels with
 * `captures.image`) and the recognized text unless `output.omitText` is set.
 */
export type ScrollUntilOptions = InputFields<typeof ScrollUntilBeginSchema, 'awaitDecisions' | 'point' | 'window'>

/** One generator step for `scrollWith`. Velocities are logical px/s. */
export interface ScrollWithStep {
  holdMs?: number
  velocityX?: number
  velocityY?: number
}

export interface WindowClient {
  capture: (options?: CaptureOptions) => Promise<Shape<typeof CaptureService.method.captureWindow.output>>
  /** Clicks in this window; see `PointTarget` for what `target` accepts. */
  click: (target: PointTarget, clickOptions?: Init<typeof ClickOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickPoint.output>>
  findText: (query: string, options?: FindWindowTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findWindowText.output>>
  /** Window ID; the same as `window.ref.windowId`. */
  readonly id: string
  /**
   * Pastes text into this window through the clipboard. Same policy default
   * and focus rules as `typeText`.
   */
  pasteText: (text: string, pasteOptions?: WindowPasteTextOptions, options?: OperationOptions) => Promise<InputActionResult>
  /**
   * Presses one key combination in this window, e.g. `'cmd+a'`, `['cmd', 'a']`
   * or `'return'`, optionally repeated. Strings split as `splitKeyCombination`
   * does. Same policy default and focus rules as `typeText`.
   */
  pressKeys: (keys: readonly string[] | string, pressOptions?: WindowPressKeysOptions, options?: OperationOptions) => Promise<InputActionResult>
  /**
   * Wheel-scrolls at a point in this window. Deltas are logical pixels: positive
   * `deltaY` scrolls toward later content (down) and positive `deltaX` scrolls
   * right, like DOM `WheelEvent`. The result is delivery evidence only.
   */
  scroll: (point: PointTarget, scroll: Init<typeof ScrollSchema>, scrollOptions?: Init<typeof ScrollOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.scrollWindowPoint.output>>
  /**
   * Spreads one scroll over time with a timing function and streams
   * `started`, `progress`, and `completed` events. Aborting `options.signal`
   * stops the remaining samples. Totals use the same logical-pixel,
   * positive-down convention as `scroll`.
   */
  scrollMotion: (point: PointTarget, motion: Init<typeof ScrollMotionSchema>, scrollOptions?: Init<typeof ScrollOptionsSchema>, options?: OperationOptions) => Promise<AsyncIterable<ScrollWindowPointMotionResponse>>
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
  scrollUntil: (point: PointTarget, request: ScrollUntilOptions, options?: ScrollUntilCallOptions) => Promise<ScrollUntilCompleted>
  /**
   * Drives a live scroll stream from a (possibly async) generator. Each yielded
   * step sets the velocity and holds it for `holdMs` (default 100), renewing the
   * lease while it holds. The stream stops (ramping down) when the generator
   * finishes, and resolves with the completion event.
   */
  scrollWith: (steps: AsyncIterable<ScrollWithStep> | Iterable<ScrollWithStep>, begin: ScrollStreamBegin, options?: OperationOptions) => Promise<StreamScrollCompleted>
  /**
   * Types text into this window. `typeOptions.policy` defaults to
   * `InputPolicy.FOREGROUND_PREFERRED`: the window is brought to the front and
   * focused first, so text cannot land in another app. Background policies
   * post to the window's process without activating it, so the control must
   * already have keyboard focus; a background click does not give it focus in
   * every app (it did not in NetEase Cloud Music). The result is delivery
   * evidence, not proof the text arrived.
   */
  typeText: (text: string, typeOptions?: Init<typeof TypeTextOptionsSchema>, options?: OperationOptions) => Promise<InputActionResult>
  /**
   * Metadata from the call that produced this client (`resolve` or `list`),
   * or just the reference for a client bound with `windows.client`. It is a
   * snapshot: the Runner re-resolves the window before every operation.
   */
  readonly window: Window
}

/** Options for `WindowClient.pasteText`; `policy` defaults to `InputPolicy.FOREGROUND_PREFERRED`. */
export type WindowPasteTextOptions = Init<typeof PasteTextOptionsSchema> & { policy?: InputPolicy }

/** Options for `WindowClient.pressKeys`; `policy` defaults to `InputPolicy.FOREGROUND_PREFERRED`. */
export type WindowPressKeysOptions = InputFields<typeof PressKeysOptionsSchema, 'keys'> & { policy?: InputPolicy }
// gRPC status code NOT_FOUND.
const GRPC_NOT_FOUND = 5

/**
 * The short form of a `WindowSelector`: at most one application field and at
 * most one title field. With no application the frontmost app is used; with
 * no title, its main visible window.
 */
export interface WindowQuery {
  appName?: string
  bundleId?: string
  /** The frontmost app; the default when no application field is given. */
  frontmost?: boolean
  pid?: number
  /** The exact window title. */
  title?: string
  titleContains?: string
}

/** Anything that names one window: a client, a `Window`, a `WindowRef`, or a window ID. */
export type WindowTarget = string | Window | WindowClient | WindowRef

type CoordinateSpaceInit = NonNullable<Init<typeof PositionSchema>['coordinateSpace']>

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
    await call.send({ event: { case: 'begin', value: { ...begin, point: positionOf(begin.point, { case: 'windowId', value: windowId }), window: { windowId } } } })
    return {
      cancel: () => call.send({ event: { case: 'cancel', value: {} } }),
      events: call.responses,
      setVelocity: velocity => call.send({ event: { case: 'setVelocity', value: { velocity } } }),
      stop: () => call.send({ event: { case: 'stop', value: {} } }),
    }
  }
  /** A window from a fresh listing; NOT_FOUND, like the Runner, once it is gone. */
  const findWindow = async (id: string | undefined, options?: OperationOptions): Promise<Window> => {
    const windows = (await unary(WindowService.method.listWindows, {}, options)).windows
    const found = windows.find(candidate => candidate.ref?.windowId === id)
    if (!found)
      throw new AuvRpcError(GRPC_NOT_FOUND, `window:${id} was not found`)
    return found
  }
  const window = (metadata: Window): WindowClient => {
    const id = metadata.ref?.windowId
    if (id === undefined || id.length === 0)
      throw new AuvProtocolError('Window omitted ref.windowId')
    // Keyboard recipients must name the owning process, so the Runner can
    // refuse a window ID that now belongs to another app. A client bound from
    // a bare ID or ref looks it up once per call.
    const keyboard = async (input: Init<typeof KeyboardInputSchema>, options?: OperationOptions): Promise<InputActionResult> => {
      const recipient = metadata.processId ? metadata : await findWindow(id, options)
      const response = await unary(InputService.method.inputKeyboard, {
        inputs: [input],
        target: { recipient: { case: 'window', value: recipient } },
      }, options)
      const action = response.actions[0]
      if (!action)
        throw new AuvProtocolError('InputKeyboardResponse omitted the action result')
      return action
    }
    const local = { case: 'windowId', value: id } as const
    return {
      capture: ({ resolution, ...options } = {}) => unary(CaptureService.method.captureWindow, { resolution, window: { windowId: id } }, options),
      click: (target, clickOptions, options) => unary(InputService.method.clickPoint, {
        options: clickOptions,
        position: positionOf(target, local),
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
      pasteText: (text, { policy, ...pasteOptions } = {}, options) => keyboard({
        action: { case: 'pasteText', value: { options: pasteOptions, policy: policy || InputPolicy.FOREGROUND_PREFERRED, text } },
      }, options),
      pressKeys: (keys, { policy, ...pressOptions } = {}, options) => keyboard({
        action: { case: 'press', value: { options: { ...pressOptions, keys: typeof keys === 'string' ? splitKeyCombination(keys) : [...keys] }, policy: policy || InputPolicy.FOREGROUND_PREFERRED } },
      }, options),
      scroll: (point, scroll, scrollOptions, options) => unary(InputService.method.scrollWindowPoint, {
        options: scrollOptions,
        point: positionOf(point, local),
        scroll,
        window: { windowId: id },
      }, options),
      scrollMotion: (point, motion, scrollOptions, options) => serverStream(InputService.method.scrollWindowPointMotion, {
        motion,
        options: scrollOptions,
        point: positionOf(point, local),
        window: { windowId: id },
      }, options),
      scrollStream: (begin, options) => openScrollStream(id, begin, options),
      scrollUntil: async (point, request, options = {}) => {
        const { onUpdate, until, ...operation } = options
        const call = await duplex(InputService.method.scrollUntil, operation)
        const condition = request.condition?.case === undefined ? { case: 'end' as const, value: {} } : request.condition
        await call.send({
          event: {
            case: 'begin',
            value: { ...request, awaitDecisions: until !== undefined, condition, point: positionOf(point, local), window: { windowId: id } },
          },
        })
        for await (const response of call.responses) {
          const event = response.event
          if (event.case === 'completed')
            return event.value
          if (event.case !== 'update')
            continue
          await onUpdate?.(event.value)
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
      typeText: (text, typeOptions = {}, options) => keyboard({
        action: { case: 'typeText', value: { options: { ...typeOptions, policy: typeOptions.policy || InputPolicy.FOREGROUND_PREFERRED }, text } },
      }, options),
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
      capture: (selector, { resolution, ...options } = {}) => unary(CaptureService.method.captureDisplay, { resolution, selector }, options),
      captureRegion: (region, selector, { resolution, ...options } = {}) => unary(CaptureService.method.captureRegion, { region, resolution, selector }, options),
      findText: (selector, query, options = {}) => {
        const { signal, ...request } = options
        return unary(TextRecognitionService.method.findDisplayText, { ...request, query, selector }, { signal })
      },
      list: async options => (await unary(DisplayService.method.listDisplays, {}, options)).displays,
    },
    input: {
      click: (target, clickOptions, options) => unary(InputService.method.clickPoint, {
        options: clickOptions,
        position: positionOf(target, { case: 'screen', value: true }),
      }, options),
      createMouse: (request, options) => unary(InputService.method.createMouse, request, options),
      dragMouse: (request, options) => unary(InputService.method.dragMouse, request, options),
      holdKeys: (request, options) => unary(InputService.method.holdKeys, request, options),
      keyboard: (request, options) => unary(InputService.method.inputKeyboard, request, options),
      keyDown: (request, options) => unary(InputService.method.keyDown, request, options),
      keyUp: (request, options) => unary(InputService.method.keyUp, request, options),
      mouseDown: (request, options) => unary(InputService.method.mouseDown, request, options),
      mouseUp: (request, options) => unary(InputService.method.mouseUp, request, options),
      moveMouse: (request, options) => serverStream(InputService.method.moveMouse, request, options),
      pasteText: (text, options, operation) => unary(InputService.method.pasteText, { options, text }, operation),
      pressKey: (key, options = {}) => unary(InputService.method.pressKey, { key, settle: options.settle }, options),
      pressKeys: (request, options) => unary(InputService.method.pressKeys, request, options),
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
      get: async (target, options) => window(await findWindow(windowOf(target).ref?.windowId, options)),
      list: async options => (await unary(WindowService.method.listWindows, {}, options)).windows.map(window),
      resolve: async (selector, options) => {
        const response = await unary(WindowService.method.resolveWindow, { selector: windowSelectorOf(selector) }, options)
        if (!response.window)
          throw new AuvProtocolError('ResolveWindowResponse omitted window')
        return window(response.window)
      },
    },
  }
}

/**
 * Splits a key combination such as `cmd+a` into its keys. A `+` separates
 * keys only when another character follows, so `cmd++` is Command and the
 * plus key. Matches Rust's `split_key_combination`.
 */
export function splitKeyCombination(combination: string): string[] {
  return combination.split(/\+(?=.)/).map(key => key.trim()).filter(key => key.length > 0)
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

/** Encodes a `PointTarget`; a plain `{ x, y }` takes the caller's default space. */
function positionOf(target: PointTarget, plain: CoordinateSpaceInit): Init<typeof PositionSchema> {
  if ('coordinateSpace' in target && target.coordinateSpace?.case !== undefined)
    return target
  if ('bounds' in target) {
    const bounds = target.bounds
    if (!bounds)
      throw new TypeError('point target has no bounds')
    const { height = 0, width = 0, x = 0, y = 0 } = bounds
    return { coordinateSpace: { case: 'screen', value: true }, ...center({ height, width, x, y }) }
  }
  if (!('x' in target) || !('y' in target) || target.x === undefined || target.y === undefined)
    throw new TypeError('point target needs a coordinateSpace, bounds, or x and y')
  return { coordinateSpace: plain, x: target.x, y: target.y }
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

function windowSelectorOf(selector: Init<typeof WindowSelectorSchema> | WindowQuery): Init<typeof WindowSelectorSchema> {
  if ('application' in selector || 'window' in selector || '$typeName' in selector)
    return selector as Init<typeof WindowSelectorSchema>
  const query = selector as WindowQuery
  const applications = [
    query.bundleId === undefined ? undefined : { case: 'applicationBundleId' as const, value: query.bundleId },
    query.appName === undefined ? undefined : { case: 'applicationName' as const, value: query.appName },
    query.pid === undefined ? undefined : { case: 'processId' as const, value: query.pid },
    query.frontmost ? { case: 'frontmostApplication' as const, value: true } : undefined,
  ].filter(value => value !== undefined)
  const windows = [
    query.title === undefined ? undefined : { case: 'titleExact' as const, value: query.title },
    query.titleContains === undefined ? undefined : { case: 'titleContains' as const, value: query.titleContains },
  ].filter(value => value !== undefined)
  if (applications.length > 1)
    throw new TypeError('WindowQuery takes one of bundleId, appName, pid and frontmost')
  if (windows.length > 1)
    throw new TypeError('WindowQuery takes one of title and titleContains')
  return {
    application: applications[0] ?? { case: 'frontmostApplication', value: true },
    window: windows[0] ?? { case: 'mainVisible', value: true },
  }
}
