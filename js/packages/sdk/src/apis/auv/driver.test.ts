import type { UnaryCall } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { CaptureResolution, CaptureWindowRequestSchema, CaptureWindowResponseSchema, GetCaptureImageRequestSchema, GetCaptureImageResponseSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import { ListDisplaysResponseSchema } from '../../gen/auv/api/driver/v1/display_pb'
import { ClickPointRequestSchema, ClickPointResponseSchema, CreateMouseResponseSchema, DragMouseRequestSchema, DragMouseResponseSchema, HoldKeysRequestSchema, HoldKeysResponseSchema, InputKeyboardRequestSchema, InputKeyboardResponseSchema, InputPolicy, KeyDownRequestSchema, KeyDownResponseSchema, KeyUpRequestSchema, KeyUpResponseSchema, MouseButton, MouseDownRequestSchema, MouseDownResponseSchema, MouseUpRequestSchema, MouseUpResponseSchema, ScrollDeliveryCandidate, ScrollUntilRequestSchema, ScrollUntilResponseSchema, ScrollUntilStopReason, ScrollWindowPointMotionRequestSchema, ScrollWindowPointMotionResponseSchema, ScrollWindowPointRequestSchema, ScrollWindowPointResponseSchema, StandardMotionTimingFunction, StreamScrollRequestSchema, StreamScrollResponseSchema } from '../../gen/auv/api/driver/v1/input_pb'
import { RecognizeTextRequestSchema, RecognizeTextResponseSchema } from '../../gen/auv/api/driver/v1/text_recognition_pb'
import { ListWindowsResponseSchema, ResolveWindowRequestSchema, ResolveWindowResponseSchema } from '../../gen/auv/api/driver/v1/window_pb'
import { ImageEncoding } from '../../gen/auv/api/image/v1/image_pb'
import { connect } from '../../node/index'
import { createAuv } from './client'
import { splitKeyCombination } from './driver'

describe('runner Driver control surface', () => {
  it('recognizes Runner captures by reference and fetches pixels only on request', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.CaptureService/GetCaptureImage':
              return toBinary(GetCaptureImageResponseSchema, create(GetCaptureImageResponseSchema, {
                image: { data: new Uint8Array([1, 2]), encoding: ImageEncoding.JPEG, height: 50, width: 80 },
              }))
            case '/auv.api.driver.v1.TextRecognitionService/RecognizeText':
              return toBinary(RecognizeTextResponseSchema, create(RecognizeTextResponseSchema, { text: 'ok' }))
            default: throw new Error(`unexpected method: ${call.method}`)
          }
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const frame = { ref: { captureId: 'cap-1' } }

    expect((await runner.recognizeText(frame, { region: { height: 0.5, width: 1, x: 0, y: 0 } })).text).toBe('ok')
    const recognize = fromBinary(RecognizeTextRequestSchema, calls[0]!.body)
    expect(recognize.source).toEqual({ case: 'captureRef', value: expect.objectContaining({ captureId: 'cap-1' }) })
    expect(recognize.region?.height).toBe(0.5)

    await runner.recognizeText(frame, { screenRegion: { height: 40, width: 300, x: 20, y: 10 } })
    expect(fromBinary(RecognizeTextRequestSchema, calls[1]!.body).screenRegion).toMatchObject({ height: 40, width: 300, x: 20, y: 10 })

    const image = await runner.captures.image('cap-1', { encoding: ImageEncoding.JPEG, maxSize: { height: 100, width: 100 } })
    expect(image).toMatchObject({ encoding: ImageEncoding.JPEG, height: 50, width: 80 })
    const fetch = fromBinary(GetCaptureImageRequestSchema, calls[2]!.body)
    expect(fetch.capture?.captureId).toBe('cap-1')
    expect(fetch.maxSize).toMatchObject({ height: 100, width: 100 })

    expect(() => runner.recognizeText({ ref: undefined })).toThrow(TypeError)
    await connection.close()
  })

  it('preserves a held chord and its opaque release ID across Runner calls', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/HoldKeys':
              return toBinary(HoldKeysResponseSchema, create(HoldKeysResponseSchema))
            case '/auv.api.driver.v1.InputService/KeyDown':
              return toBinary(KeyDownResponseSchema, create(KeyDownResponseSchema, { holdId: 17n }))
            case '/auv.api.driver.v1.InputService/KeyUp':
              return toBinary(KeyUpResponseSchema, create(KeyUpResponseSchema))
            default: throw new Error(`unexpected method: ${call.method}`)
          }
        },
      },
    })
    const input = createAuv(connection).runner({ runnerClass: 'auv.core.local' }).input
    const target = { recipient: { case: 'foreground' as const, value: true } }
    const { holdId } = await input.keyDown({ keys: ['shift', 'a'], target, timeout: { seconds: 5n } })
    await input.keyUp({ holdId })
    await input.holdKeys({ duration: { seconds: 1n }, keys: ['space'], target })
    expect(fromBinary(KeyDownRequestSchema, calls[0]!.body).keys).toEqual(['shift', 'a'])
    expect(fromBinary(KeyDownRequestSchema, calls[0]!.body).timeout?.seconds).toBe(5n)
    expect(fromBinary(KeyUpRequestSchema, calls[1]!.body).holdId).toBe(17n)
    expect(fromBinary(HoldKeysRequestSchema, calls[2]!.body).duration?.seconds).toBe(1n)
    await connection.close()
  })

  it('preserves click buttons and modifiers in both screen and window protobuf requests', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/ClickPoint':
              return toBinary(ClickPointResponseSchema, create(ClickPointResponseSchema))
            case '/auv.api.driver.v1.WindowService/ResolveWindow':
              return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
                window: { ref: { windowId: 'modifier-target' } },
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const modifiers = { alt: true, control: true, meta: true, shift: true }
    await runner.input.click({ x: 10, y: 20 }, { button: MouseButton.RIGHT, click: { count: 1 }, modifiers })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    await window.click({ x: 10, y: 20 }, { button: MouseButton.MIDDLE, modifiers })
    const screen = fromBinary(ClickPointRequestSchema, calls[0]!.body)
    const targeted = fromBinary(ClickPointRequestSchema, calls[2]!.body)
    // A plain point is screen space for `input`, and window-local for a window.
    expect(screen.position).toMatchObject({ coordinateSpace: { case: 'screen', value: true }, x: 10, y: 20 })
    expect(screen.window).toBeUndefined()
    expect(targeted.position).toMatchObject({ coordinateSpace: { case: 'windowId', value: 'modifier-target' }, x: 10, y: 20 })
    expect(screen.options?.button).toBe(MouseButton.RIGHT)
    expect(targeted.options?.button).toBe(MouseButton.MIDDLE)
    expect(screen.options?.modifiers).toMatchObject(modifiers)
    expect(targeted.options?.modifiers).toMatchObject(modifiers)
    expect(targeted.window?.windowId).toBe('modifier-target')
  })

  it('sends window scroll deltas, policy, and ordered candidates unchanged', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/ScrollWindowPoint':
              return toBinary(ScrollWindowPointResponseSchema, create(ScrollWindowPointResponseSchema, { scroll: { deltaY: 300 } }))
            case '/auv.api.driver.v1.WindowService/ResolveWindow':
              return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
                window: { ref: { windowId: 'scroll-target' } },
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    const response = await window.scroll({ x: 10, y: 20 }, { deltaX: -15, deltaY: 300 }, {
      deliveryCandidates: [ScrollDeliveryCandidate.WINDOW_TARGETED_WHEEL, ScrollDeliveryCandidate.FOREGROUND_HID],
      policy: InputPolicy.BACKGROUND_ONLY,
    })
    const request = fromBinary(ScrollWindowPointRequestSchema, calls[1]!.body)
    expect(request.window?.windowId).toBe('scroll-target')
    expect(request.point).toMatchObject({ x: 10, y: 20 })
    expect(request.scroll).toMatchObject({ deltaX: -15, deltaY: 300 })
    expect(request.options?.policy).toBe(InputPolicy.BACKGROUND_ONLY)
    expect(request.options?.deliveryCandidates).toEqual([ScrollDeliveryCandidate.WINDOW_TARGETED_WHEEL, ScrollDeliveryCandidate.FOREGROUND_HID])
    expect(response.scroll?.deltaY).toBe(300)
  })

  it('streams a timed window scroll with its motion plan and decodes completion', async () => {
    const sent: Uint8Array[] = []
    let streamedMethod = ''
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex(call) {
          streamedMethod = call.method
          return {
            close() {},
            async halfClose() {},
            responses: (async function* () {
              yield toBinary(ScrollWindowPointMotionResponseSchema, create(ScrollWindowPointMotionResponseSchema, {
                event: { case: 'started', value: { plannedSampleCount: 31n } },
              }))
              yield toBinary(ScrollWindowPointMotionResponseSchema, create(ScrollWindowPointMotionResponseSchema, {
                event: { case: 'completed', value: { delivered: { deltaY: 600 } } },
              }))
            })(),
            async send(body) { sent.push(body) },
          }
        },
        async unary() {
          return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
            window: { ref: { windowId: 'motion-target' } },
          }))
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    const events = []
    for await (const event of await window.scrollMotion({ x: 10, y: 20 }, {
      sampleRateHz: 60,
      timing: {
        case: 'fixedDuration',
        value: {
          duration: { nanos: 500_000_000 },
          function: { function: { case: 'standard', value: StandardMotionTimingFunction.EASE_OUT_CUBIC } },
        },
      },
      total: { deltaY: 600 },
    })) {
      events.push(event.event.case)
    }
    expect(streamedMethod).toBe('/auv.api.driver.v1.InputService/ScrollWindowPointMotion')
    expect(events).toEqual(['started', 'completed'])
    const request = fromBinary(ScrollWindowPointMotionRequestSchema, sent[0]!)
    expect(request.window?.windowId).toBe('motion-target')
    expect(request.motion?.total?.deltaY).toBe(600)
    expect(request.motion?.sampleRateHz).toBe(60)
    expect(request.motion?.timing.case).toBe('fixedDuration')
  })

  it('answers scroll-until updates with a client predicate', async () => {
    const sent: Uint8Array[] = []
    let streamedMethod = ''
    const decisions: Array<(stop: boolean) => void> = []
    const nextDecision = () => new Promise<boolean>(resolve => decisions.push(resolve))
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex(call) {
          streamedMethod = call.method
          return {
            close() {},
            async halfClose() {},
            responses: (async function* () {
              for (const [steps, line] of [[0, 'feed item 1'], [1, 'TARGET ROW 137']] as const) {
                const decision = nextDecision()
                yield toBinary(ScrollUntilResponseSchema, create(ScrollUntilResponseSchema, {
                  event: { case: 'update', value: { awaitingDecision: true, steps, text: { text: line } } },
                }))
                if (await decision) {
                  yield toBinary(ScrollUntilResponseSchema, create(ScrollUntilResponseSchema, {
                    event: { case: 'completed', value: { reason: ScrollUntilStopReason.PREDICATE_SATISFIED, steps } },
                  }))
                  return
                }
              }
            })(),
            async send(body) {
              sent.push(body)
              const event = fromBinary(ScrollUntilRequestSchema, body).event
              if (event.case === 'decision')
                decisions.shift()!(event.value.stop)
            },
          }
        },
        async unary() {
          return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
            window: { ref: { windowId: 'until-target' } },
          }))
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    const seen: number[] = []

    const completed = await window.scrollUntil({ x: 10, y: 20 }, {
      maxSteps: 40,
      noMotionConfirmations: 3,
      settle: {},
      step: { case: 'instant', value: { deltaY: 600 } },
    }, {
      onUpdate: update => void seen.push(update.steps),
      until: async update => update.text?.text.includes('TARGET') ?? false,
    })

    expect(streamedMethod).toBe('/auv.api.driver.v1.InputService/ScrollUntil')
    expect(completed.reason).toBe(ScrollUntilStopReason.PREDICATE_SATISFIED)
    expect(seen).toEqual([0, 1])
    const requests = sent.map(body => fromBinary(ScrollUntilRequestSchema, body).event)
    expect(requests.map(event => event.case === 'decision' ? event.value.stop : event.case)).toEqual(['begin', false, true])
    const begin = requests[0]!
    expect(begin.case === 'begin' && begin.value.window?.windowId).toBe('until-target')
    expect(begin.case === 'begin' && begin.value.awaitDecisions).toBe(true)
    expect(begin.case === 'begin' && begin.value.condition.case).toBe('end')
    expect(begin.case === 'begin' && begin.value.step.case === 'instant' && begin.value.step.value.deltaY).toBe(600)
    // Given values are kept, including an explicit zero settle.
    expect(begin.case === 'begin' && [begin.value.maxSteps, begin.value.noMotionConfirmations, begin.value.settle?.nanos]).toEqual([40, 3, 0])
  })

  it('runs scroll-until without decisions when no predicate is given', async () => {
    const sent: Uint8Array[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() {
          return {
            close() {},
            async halfClose() {},
            responses: (async function* () {
              yield toBinary(ScrollUntilResponseSchema, create(ScrollUntilResponseSchema, {
                event: { case: 'update', value: { steps: 0 } },
              }))
              yield toBinary(ScrollUntilResponseSchema, create(ScrollUntilResponseSchema, {
                event: { case: 'update', value: { steps: 1, stop: ScrollUntilStopReason.TEXT_VISIBLE } },
              }))
              yield toBinary(ScrollUntilResponseSchema, create(ScrollUntilResponseSchema, {
                event: { case: 'completed', value: { reason: ScrollUntilStopReason.TEXT_VISIBLE, steps: 1 } },
              }))
            })(),
            async send(body) { sent.push(body) },
          }
        },
        async unary() {
          return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
            window: { ref: { windowId: 'until-target' } },
          }))
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    const stops: ScrollUntilStopReason[] = []

    const completed = await window.scrollUntil({ x: 10, y: 20 }, {
      condition: { case: 'textVisible', value: { query: 'Load more' } },
      output: { omitText: true },
      step: { case: 'instant', value: { deltaY: 600 } },
    }, { onUpdate: update => void stops.push(update.stop) })

    expect(completed.reason).toBe(ScrollUntilStopReason.TEXT_VISIBLE)
    expect(stops).toEqual([ScrollUntilStopReason.UNSPECIFIED, ScrollUntilStopReason.TEXT_VISIBLE])
    const requests = sent.map(body => fromBinary(ScrollUntilRequestSchema, body).event)
    expect(requests.map(event => event.case)).toEqual(['begin'])
    const begin = requests[0]!
    expect(begin.case === 'begin' && begin.value.awaitDecisions).toBe(false)
    expect(begin.case === 'begin' && begin.value.output?.omitText).toBe(true)
    expect(begin.case === 'begin' && begin.value.condition.case === 'textVisible' && begin.value.condition.value.query).toBe('Load more')
    // Omitted budget fields stay unset so the Runner applies its defaults.
    expect(begin.case === 'begin' && [begin.value.maxSteps, begin.value.noMotionConfirmations, begin.value.settle]).toEqual([0, 0, undefined])
  })

  it('drives a live scroll stream from a generator and stops when it finishes', async () => {
    const sent: Uint8Array[] = []
    let streamedMethod = ''
    let stopped: () => void = () => {}
    const stopReceived = new Promise<void>((resolve) => {
      stopped = resolve
    })
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex(call) {
          streamedMethod = call.method
          return {
            close() {},
            async halfClose() {},
            responses: (async function* () {
              yield toBinary(StreamScrollResponseSchema, create(StreamScrollResponseSchema, { event: { case: 'started', value: {} } }))
              await stopReceived
              yield toBinary(StreamScrollResponseSchema, create(StreamScrollResponseSchema, {
                event: { case: 'completed', value: { delivered: { deltaY: 120 }, reason: 1 } },
              }))
            })(),
            async send(body) {
              sent.push(body)
              if (fromBinary(StreamScrollRequestSchema, body).event.case === 'stop')
                stopped()
            },
          }
        },
        async unary() {
          return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
            window: { ref: { windowId: 'stream-target' } },
          }))
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    function* steps() {
      yield { holdMs: 5, velocityY: 600 }
      yield { holdMs: 5, velocityY: -200 }
    }
    const completed = await window.scrollWith(steps(), {
      lease: { nanos: 500_000_000 },
      maxAcceleration: 4000,
      point: { x: 10, y: 20 },
      sampleRateHz: 60,
    })

    expect(streamedMethod).toBe('/auv.api.driver.v1.InputService/StreamScroll')
    expect(completed.delivered?.deltaY).toBe(120)
    const requests = sent.map(body => fromBinary(StreamScrollRequestSchema, body).event)
    expect(requests.map(event => event.case)).toEqual(['begin', 'setVelocity', 'setVelocity', 'stop'])
    expect(requests[0]!.case === 'begin' && requests[0]!.value.window?.windowId).toBe('stream-target')
    expect(requests[0]!.case === 'begin' && requests[0]!.value.maxAcceleration).toBe(4000)
    expect(requests[2]!.case === 'setVelocity' && requests[2]!.value.velocity?.deltaYPerSecond).toBe(-200)
  })

  it('retains a created mouse and exact window owner across primitive and complete gestures', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/CreateMouse':
              return toBinary(CreateMouseResponseSchema, create(CreateMouseResponseSchema, { mouse: 7n }))
            case '/auv.api.driver.v1.InputService/DragMouse':
              return toBinary(DragMouseResponseSchema, create(DragMouseResponseSchema))
            case '/auv.api.driver.v1.InputService/MouseDown':
              return toBinary(MouseDownResponseSchema, create(MouseDownResponseSchema))
            case '/auv.api.driver.v1.InputService/MouseUp':
              return toBinary(MouseUpResponseSchema, create(MouseUpResponseSchema))
            default: throw new Error(`unexpected method: ${call.method}`)
          }
        },
      },
    })
    const input = createAuv(connection).runner({ runnerClass: 'auv.core.local' }).input
    const { mouse } = await input.createMouse({})
    const target = { recipient: { case: 'window' as const, value: { processId: 123, ref: { windowId: 'window-42' } } } }
    await input.mouseDown({ button: MouseButton.RIGHT, mouse, point: { x: 10, y: 20 }, target, timeout: { seconds: 2n } })
    await input.mouseUp({ mouse })
    await input.dragMouse({ button: MouseButton.MIDDLE, movement: { mouse, options: { curveTolerance: 0.01, duration: { seconds: 3600n }, sampleRateHz: 1000 }, start: { source: { case: 'point', value: { x: 20, y: 30 } } }, target } })
    const down = fromBinary(MouseDownRequestSchema, calls[1]!.body)
    const up = fromBinary(MouseUpRequestSchema, calls[2]!.body)
    const drag = fromBinary(DragMouseRequestSchema, calls[3]!.body)
    expect(down.mouse).toBe(7n)
    expect(up.mouse).toBe(7n)
    expect(down.target?.recipient).toMatchObject(target.recipient)
    expect(down.button).toBe(MouseButton.RIGHT)
    expect(down.timeout?.seconds).toBe(2n)
    expect(drag.movement?.options).toMatchObject({ curveTolerance: 0.01, duration: { seconds: 3600n }, sampleRateHz: 1000 })
    expect(drag.movement?.mouse).toBe(7n)
    expect(drag.movement?.target?.recipient).toMatchObject(target.recipient)
    expect(drag.movement?.start?.source).toMatchObject({ case: 'point', value: { x: 20, y: 30 } })
    await connection.close()
  })

  it('binds a resolved window by ID and refreshes updates through each capability call', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.CaptureService/CaptureWindow':
              return toBinary(CaptureWindowResponseSchema, create(CaptureWindowResponseSchema, {
                window: {
                  frame: { height: 720, width: 1280, x: 20, y: 30 },
                  ref: { windowId: 'window-42' },
                  title: 'Current title',
                },
              }))
            case '/auv.api.driver.v1.WindowService/ResolveWindow':
              return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
                window: {
                  frame: { height: 100, width: 100, x: 0, y: 0 },
                  ref: { windowId: 'window-42' },
                  title: 'Initial title',
                },
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })

    const window = await runner.windows.resolve({
      application: { case: 'applicationBundleId', value: 'com.example.App' },
    })

    expect(window.id).toBe('window-42')
    expect(Object.keys(window).sort()).toEqual(['capture', 'click', 'findText', 'id', 'pasteText', 'pressKeys', 'scroll', 'scrollMotion', 'scrollStream', 'scrollUntil', 'scrollWith', 'typeText', 'window'])

    const capture = await window.capture()
    expect(capture.window?.frame?.width).toBe(1280)
    expect(capture.window?.title).toBe('Current title')

    const resolve = fromBinary(ResolveWindowRequestSchema, calls[0]!.body)
    expect(resolve.selector?.application).toEqual({ case: 'applicationBundleId', value: 'com.example.App' })
    const captureRequest = fromBinary(CaptureWindowRequestSchema, calls[1]!.body)
    expect(captureRequest.window?.windowId).toBe('window-42')
    expect(captureRequest.resolution, 'native by default').toBe(CaptureResolution.UNSPECIFIED)

    await window.capture({ resolution: CaptureResolution.LOGICAL })
    expect(fromBinary(CaptureWindowRequestSchema, calls[2]!.body).resolution).toBe(CaptureResolution.LOGICAL)
    expect(calls.every(call => call.headers.get('auv-runner-class') === 'auv.core.local')).toBe(true)
  })

  it('uses generated service descriptors for route and response typing', async () => {
    const methods: string[] = []
    const connection = await connect({
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          methods.push(call.method)
          return toBinary(ListDisplaysResponseSchema, create(ListDisplaysResponseSchema, {
            displays: [{ displayId: 'main', primary: true, scaleFactor: 2 }],
          }))
        },
      },
    })

    const displays = await createAuv(connection).runner({
      deviceId: 'device-1',
      runId: 'run-1',
      runnerClass: 'auv.core.local',
    }).displays.list()

    expect(methods).toEqual(['/auv.api.driver.v1.DisplayService/ListDisplays'])
    expect(displays.map(display => display.displayId)).toEqual(['main'])
  })
})

describe('window clients', () => {
  async function windowConnection(calls: UnaryCall[]) {
    return await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/ClickPoint':
              return toBinary(ClickPointResponseSchema, create(ClickPointResponseSchema))
            case '/auv.api.driver.v1.WindowService/ListWindows':
              return toBinary(ListWindowsResponseSchema, create(ListWindowsResponseSchema, {
                windows: [{ applicationName: 'Music', ref: { windowId: 'w-2' }, title: 'Liked Songs' }],
              }))
            case '/auv.api.driver.v1.WindowService/ResolveWindow':
              return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
                window: { frame: { height: 600, width: 800, x: 10, y: 20 }, ref: { windowId: 'w-1' }, title: 'Inbox' },
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
  }

  it('keeps resolved and listed window metadata, and listed windows act without a resolve', async () => {
    const calls: UnaryCall[] = []
    const connection = await windowConnection(calls)
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const resolved = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    expect(resolved.id).toBe('w-1')
    expect(resolved.window.title).toBe('Inbox')
    expect(resolved.window.frame).toMatchObject({ height: 600, width: 800, x: 10, y: 20 })

    const [listed] = await runner.windows.list()
    expect(listed?.window.applicationName).toBe('Music')
    await listed!.click({ x: 5, y: 5 })
    expect(calls.map(call => call.method.split('/').pop())).toEqual(['ResolveWindow', 'ListWindows', 'ClickPoint'])
    expect(fromBinary(ClickPointRequestSchema, calls[2]!.body).window?.windowId).toBe('w-2')
    await connection.close()
  })

  it('resolves a window from a short query or the full selector', async () => {
    const calls: UnaryCall[] = []
    const connection = await windowConnection(calls)
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    await runner.windows.resolve({ bundleId: 'com.example.App' })
    await runner.windows.resolve({ appName: 'Music', titleContains: 'Liked' })
    await runner.windows.resolve({})
    await runner.windows.resolve({ application: { case: 'processId', value: 42 } })
    const selectors = calls.map(call => fromBinary(ResolveWindowRequestSchema, call.body).selector)
    expect(selectors.map(selector => [selector?.application, selector?.window])).toEqual([
      [{ case: 'applicationBundleId', value: 'com.example.App' }, { case: 'mainVisible', value: true }],
      [{ case: 'applicationName', value: 'Music' }, { case: 'titleContains', value: 'Liked' }],
      [{ case: 'frontmostApplication', value: true }, { case: 'mainVisible', value: true }],
      [{ case: 'processId', value: 42 }, { case: undefined }],
    ])
    // Two application fields are ambiguous, so they fail before any call.
    await expect(runner.windows.resolve({ appName: 'Music', bundleId: 'com.apple.Music' })).rejects.toThrow(TypeError)
    expect(calls).toHaveLength(4)
    await connection.close()
  })

  it('splits key combinations on plus, unless it is the last key', () => {
    expect(splitKeyCombination('cmd+a')).toEqual(['cmd', 'a'])
    expect(splitKeyCombination(' cmd + shift + z ')).toEqual(['cmd', 'shift', 'z'])
    expect(splitKeyCombination('cmd++')).toEqual(['cmd', '+'])
    expect(splitKeyCombination('++')).toEqual(['+'])
    expect(splitKeyCombination('return')).toEqual(['return'])
  })

  it('clicks a window at a screen position, a display position, or the center of screen bounds', async () => {
    const calls: UnaryCall[] = []
    const connection = await windowConnection(calls)
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    await window.click({ coordinateSpace: { case: 'screen', value: true }, x: 30, y: 40 })
    await window.click({ coordinateSpace: { case: 'displayId', value: 'd-2' }, x: 1, y: 2 })
    await window.click({ bounds: { height: 10, width: 20, x: 100, y: 200 }, text: 'Play' } as { bounds: { height: number, width: number, x: number, y: number } })
    const positions = calls.slice(1).map(call => fromBinary(ClickPointRequestSchema, call.body))
    // The Runner converts these with the window's current frame.
    expect(positions.map(request => [request.position?.coordinateSpace, request.position?.x, request.position?.y])).toEqual([
      [{ case: 'screen', value: true }, 30, 40],
      [{ case: 'displayId', value: 'd-2' }, 1, 2],
      [{ case: 'screen', value: true }, 110, 205],
    ])
    expect(positions.every(request => request.window?.windowId === 'w-1')).toBe(true)
    await connection.close()
  })

  // ROOT CAUSE:
  //
  // A window client kept its Run's route, and a stopped Run rejects routing.
  //
  // Before the fix, callers re-resolved the window by process and title per
  // Run. The fix binds any window target to another route with no call.
  it('rebinds a window from one Run to another without a call', async () => {
    const calls: UnaryCall[] = []
    const connection = await windowConnection(calls)
    const auv = createAuv(connection)
    const first = await auv.runner({ runId: 'run-1', runnerClass: 'auv.core.local' }).windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    const second = auv.runner({ runId: 'run-2', runnerClass: 'auv.core.local' })

    for (const target of [first, first.window, first.window.ref!, 'w-1'])
      await second.windows.from(target).click({ x: 1, y: 1 })

    const clicks = calls.slice(1)
    expect(clicks.map(call => call.headers.get('auv-run-id'))).toEqual(['run-2', 'run-2', 'run-2', 'run-2'])
    expect(clicks.map(call => fromBinary(ClickPointRequestSchema, call.body).window?.windowId)).toEqual(['w-1', 'w-1', 'w-1', 'w-1'])
    expect(second.windows.from(first).window.title).toBe('Inbox')
    await connection.close()
  })
})

describe('windows.get', () => {
  it('returns a fresh bound client, or a NOT_FOUND error once the window is gone', async () => {
    const calls: UnaryCall[] = []
    const connection = await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          return toBinary(ListWindowsResponseSchema, create(ListWindowsResponseSchema, {
            windows: [{ frame: { height: 1, width: 1, x: 300, y: 0 }, ref: { windowId: 'w-2' }, title: 'Moved' }],
          }))
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const stale = runner.windows.from('w-2')
    const fresh = await runner.windows.get(stale)
    expect(fresh.window.frame?.x).toBe(300)
    await expect(runner.windows.get('w-9')).rejects.toMatchObject({ name: 'AuvRpcError', rpcCode: 5 })
    await connection.close()
  })
})

describe('window keyboard', () => {
  async function keyboardConnection(calls: UnaryCall[]) {
    return await connect({
      local: true,
      transport: {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected duplex call') },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/InputKeyboard':
              return toBinary(InputKeyboardResponseSchema, create(InputKeyboardResponseSchema, { actions: [{ attempts: [{ succeeded: true }] }] }))
            case '/auv.api.driver.v1.WindowService/ListWindows':
              return toBinary(ListWindowsResponseSchema, create(ListWindowsResponseSchema, {
                windows: [{ processId: 4242, ref: { windowId: 'w-7' }, title: 'Search' }],
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
  }

  // ROOT CAUSE:
  //
  // The SDK only wrapped TypeText/PressKey, which name no recipient, so a REPL
  // script's typing went to its own browser.
  //
  // The fix sends InputKeyboard to the window, foreground-first by default.
  it('types, presses and pastes into the window as the recipient, foreground first by default', async () => {
    const calls: UnaryCall[] = []
    const connection = await keyboardConnection(calls)
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const [music] = await runner.windows.list()

    await music!.typeText('Reply')
    await music!.pressKeys(['cmd', 'a'], { count: 2, interval: { nanos: 50_000_000 } })
    await music!.pasteText('Reply', { policy: InputPolicy.BACKGROUND_ONLY })
    await music!.pressKeys('cmd+shift+z')

    const requests = calls.slice(1).map(call => fromBinary(InputKeyboardRequestSchema, call.body))
    for (const request of requests) {
      expect(request.target?.recipient).toEqual({ case: 'window', value: expect.objectContaining({ processId: 4242, ref: expect.objectContaining({ windowId: 'w-7' }) }) })
      expect(request.inputs).toHaveLength(1)
    }
    const [typed, pressed, pasted, pressedString] = requests.map(request => request.inputs[0]!.action)
    expect(pressedString).toMatchObject({ case: 'press', value: { options: { keys: ['cmd', 'shift', 'z'] } } })
    expect(typed).toMatchObject({ case: 'typeText', value: { options: { policy: InputPolicy.FOREGROUND_PREFERRED }, text: 'Reply' } })
    expect(pressed).toMatchObject({ case: 'press', value: { options: { count: 2, keys: ['cmd', 'a'] }, policy: InputPolicy.FOREGROUND_PREFERRED } })
    expect(pasted).toMatchObject({ case: 'pasteText', value: { policy: InputPolicy.BACKGROUND_ONLY, text: 'Reply' } })
    await connection.close()
  })

  it('looks up the owning process for a window bound from its ID', async () => {
    const calls: UnaryCall[] = []
    const connection = await keyboardConnection(calls)
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })

    await runner.windows.from('w-7').typeText('Reply', { policy: InputPolicy.BACKGROUND_PREFERRED })

    expect(calls.map(call => call.method.split('/').pop())).toEqual(['ListWindows', 'InputKeyboard'])
    const request = fromBinary(InputKeyboardRequestSchema, calls[1]!.body)
    expect(request.target?.recipient).toEqual({ case: 'window', value: expect.objectContaining({ processId: 4242 }) })
    expect(request.inputs[0]!.action).toMatchObject({ case: 'typeText', value: { options: { policy: InputPolicy.BACKGROUND_PREFERRED } } })
    await expect(runner.windows.from('w-9').pressKeys(['return'])).rejects.toMatchObject({ name: 'AuvRpcError', rpcCode: 5 })
    await connection.close()
  })
})
