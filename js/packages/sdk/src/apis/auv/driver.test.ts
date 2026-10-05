import type { UnaryCall } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { CaptureWindowRequestSchema, CaptureWindowResponseSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import { ListDisplaysResponseSchema } from '../../gen/auv/api/driver/v1/display_pb'
import { ClickScreenPointRequestSchema, ClickScreenPointResponseSchema, ClickWindowPointRequestSchema, ClickWindowPointResponseSchema, CreateMouseResponseSchema, DragMouseRequestSchema, DragMouseResponseSchema, HoldKeysRequestSchema, HoldKeysResponseSchema, InputPolicy, KeyDownRequestSchema, KeyDownResponseSchema, KeyUpRequestSchema, KeyUpResponseSchema, MouseButton, MouseDownRequestSchema, MouseDownResponseSchema, MouseUpRequestSchema, MouseUpResponseSchema, ScrollDeliveryCandidate, ScrollWindowPointMotionRequestSchema, ScrollWindowPointMotionResponseSchema, ScrollWindowPointRequestSchema, ScrollWindowPointResponseSchema, StandardMotionTimingFunction } from '../../gen/auv/api/driver/v1/input_pb'
import { ResolveWindowRequestSchema, ResolveWindowResponseSchema } from '../../gen/auv/api/driver/v1/window_pb'
import { connect } from '../../node/index'
import { createAuv } from './client'

describe('runner Driver control surface', () => {
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
            case '/auv.api.driver.v1.InputService/ClickScreenPoint':
              return toBinary(ClickScreenPointResponseSchema, create(ClickScreenPointResponseSchema))
            case '/auv.api.driver.v1.InputService/ClickWindowPoint':
              return toBinary(ClickWindowPointResponseSchema, create(ClickWindowPointResponseSchema))
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
    await runner.input.clickScreenPoint({ x: 10, y: 20 }, { button: MouseButton.RIGHT, click: { count: 1 }, modifiers })
    const window = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'com.example.App' } })
    await window.click({ x: 10, y: 20 }, { button: MouseButton.MIDDLE, modifiers })
    const screen = fromBinary(ClickScreenPointRequestSchema, calls[0]!.body)
    const targeted = fromBinary(ClickWindowPointRequestSchema, calls[2]!.body)
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

  it('binds a resolved window by ID and refreshes observations through each capability call', async () => {
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
    expect(Object.keys(window).sort()).toEqual(['capture', 'click', 'findText', 'id', 'scroll', 'scrollMotion'])

    const capture = await window.capture()
    expect(capture.window?.frame?.width).toBe(1280)
    expect(capture.window?.title).toBe('Current title')

    const resolve = fromBinary(ResolveWindowRequestSchema, calls[0]!.body)
    expect(resolve.selector?.application).toEqual({ case: 'applicationBundleId', value: 'com.example.App' })
    const captureRequest = fromBinary(CaptureWindowRequestSchema, calls[1]!.body)
    expect(captureRequest.window?.windowId).toBe('window-42')
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
