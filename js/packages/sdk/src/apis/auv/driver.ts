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
import type { CapturedFrameSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import type { Display, DisplaySelectorSchema } from '../../gen/auv/api/driver/v1/display_pb'
import type { ScreenPointSchema, ScreenRectSchema, WindowPointSchema } from '../../gen/auv/api/driver/v1/geometry_pb'
import type {
  ClickOptionsSchema,
  ClickSchema,
  MouseMotionPlanSchema,
  MoveMouseStreamResponse,
  PasteTextOptionsSchema,
  TypeTextOptionsSchema,
} from '../../gen/auv/api/driver/v1/input_pb'
import type { ShowOverlayRequestSchema } from '../../gen/auv/api/driver/v1/overlay_pb'
import type {
  FindDisplayTextRequestSchema,
  FindWindowTextRequestSchema,
  RecognizeTextRequestSchema,
} from '../../gen/auv/api/driver/v1/text_recognition_pb'
import type { Window, WindowSelectorSchema } from '../../gen/auv/api/driver/v1/window_pb'
import type { AuvConnection, TypedDuplexCall } from '../../transport/connection'
import type { OperationOptions } from '../../transport/types'

import { AccessibilityService } from '../../gen/auv/api/driver/macos/v1/accessibility_pb'
import { ApplicationService } from '../../gen/auv/api/driver/macos/v1/application_pb'
import { MediaControlService } from '../../gen/auv/api/driver/macos/v1/media_control_pb'
import { PermissionService } from '../../gen/auv/api/driver/macos/v1/permission_pb'
import { CaptureService } from '../../gen/auv/api/driver/v1/capture_pb'
import { DisplayService } from '../../gen/auv/api/driver/v1/display_pb'
import { InputService } from '../../gen/auv/api/driver/v1/input_pb'
import { OverlayService } from '../../gen/auv/api/driver/v1/overlay_pb'
import { TextRecognitionService } from '../../gen/auv/api/driver/v1/text_recognition_pb'
import { WindowService } from '../../gen/auv/api/driver/v1/window_pb'
import { AuvProtocolError } from '../../transport/errors'
import { invokeDuplex, invokeServerStream, invokeUnary } from './invoke'

export interface FindDisplayTextOptions extends InputFields<typeof FindDisplayTextRequestSchema, 'query' | 'selector'>, OperationOptions {}
export interface FindWindowTextOptions extends InputFields<typeof FindWindowTextRequestSchema, 'query' | 'window'>, OperationOptions {}
export interface PressKeyOptions extends OperationOptions {
  settle?: Init<typeof DurationSchema>
}

export interface RecognizeTextOptions extends InputFields<typeof RecognizeTextRequestSchema, 'capture'>, OperationOptions {}

export interface RunnerClient {
  readonly displays: {
    capture: (selector?: Init<typeof DisplaySelectorSchema>, options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureDisplay.output>>
    captureRegion: (region: Init<typeof ScreenRectSchema>, selector?: Init<typeof DisplaySelectorSchema>, options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureRegion.output>>
    findText: (selector: Init<typeof DisplaySelectorSchema> | undefined, query: string, options?: FindDisplayTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findDisplayText.output>>
    list: (options?: OperationOptions) => Promise<readonly Display[]>
  }
  readonly input: {
    clickScreenPoint: (point: Init<typeof ScreenPointSchema>, click: Init<typeof ClickSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickScreenPoint.output>>
    moveMouse: (plan: Init<typeof MouseMotionPlanSchema>, options?: OperationOptions) => Promise<AsyncIterable<MoveMouseStreamResponse>>
    pasteText: (text: string, inputOptions?: Init<typeof PasteTextOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.pasteText.output>>
    pressKey: (key: string, options?: PressKeyOptions) => Promise<Shape<typeof InputService.method.pressKey.output>>
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
  recognizeText: (capture: Init<typeof CapturedFrameSchema>, options?: RecognizeTextOptions) => Promise<Shape<typeof TextRecognitionService.method.recognizeText.output>>
  readonly windows: {
    list: (options?: OperationOptions) => Promise<readonly Window[]>
    resolve: (selector: Init<typeof WindowSelectorSchema>, options?: OperationOptions) => Promise<WindowClient>
  }
}

export interface RunnerRouteOptions extends OperationOptions {
  deviceId?: string
  runId?: string
  runnerClass: string
}
export interface WindowClient {
  capture: (options?: OperationOptions) => Promise<Shape<typeof CaptureService.method.captureWindow.output>>
  click: (point: Init<typeof WindowPointSchema>, clickOptions?: Init<typeof ClickOptionsSchema>, options?: OperationOptions) => Promise<Shape<typeof InputService.method.clickWindowPoint.output>>
  findText: (query: string, options?: FindWindowTextOptions) => Promise<Shape<typeof TextRecognitionService.method.findWindowText.output>>
  readonly id: string
}
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
  const window = (id: string): WindowClient => ({
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
  })

  return {
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
      clickScreenPoint: (point, click, options) => unary(InputService.method.clickScreenPoint, { options: { click }, point }, options),
      moveMouse: (plan, options) => serverStream(InputService.method.moveMouse, { plan }, options),
      pasteText: (text, options, operation) => unary(InputService.method.pasteText, { options, text }, operation),
      pressKey: (key, options = {}) => unary(InputService.method.pressKey, { key, settle: options.settle }, options),
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
    recognizeText: (capture, options = {}) => {
      const { signal, ...request } = options
      return unary(TextRecognitionService.method.recognizeText, { ...request, capture }, { signal })
    },
    windows: {
      list: async options => (await unary(WindowService.method.listWindows, {}, options)).windows,
      resolve: async (selector, options) => {
        const response = await unary(WindowService.method.resolveWindow, { selector }, options)
        const id = response.window?.ref?.windowId
        if (id === undefined || id.length === 0)
          throw new AuvProtocolError('ResolveWindowResponse omitted window.ref.windowId')
        return window(id)
      },
    },
  }
}

function combineSignals(first?: AbortSignal, second?: AbortSignal): AbortSignal | undefined {
  if (first === undefined)
    return second
  if (second === undefined)
    return first
  return AbortSignal.any([first, second])
}
