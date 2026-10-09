import type { UnaryCall } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { ClickPointRequestSchema, ClickPointResponseSchema, PressKeyRequestSchema, PressKeyResponseSchema, ScrollUntilBeginSchema, ScrollUntilRequestSchema, ScrollUntilResponseSchema, ScrollUntilStopReason } from '../../gen/auv/api/driver/v1/input_pb'
import { ResolveWindowResponseSchema } from '../../gen/auv/api/driver/v1/window_pb'
import { connect } from '../../node/index'
import { createAuv } from './client'
import { durationsFromMillis } from './duration'

describe('millisecond durations', () => {
  it('converts numbers in Duration fields, including nested and oneof fields', () => {
    const begin = durationsFromMillis(ScrollUntilBeginSchema, {
      options: { settle: 250 },
      settle: 1500,
      step: { case: 'motion', value: { timing: { case: 'fixedDuration', value: { duration: 2.5 } } } },
    })
    expect(begin.settle).toEqual({ nanos: 500_000_000, seconds: 1n })
    expect(begin.options?.settle).toEqual({ nanos: 250_000_000, seconds: 0n })
    expect(begin.step?.case === 'motion' && begin.step.value.timing?.case === 'fixedDuration' && begin.step.value.timing.value.duration).toEqual({ nanos: 2_500_000, seconds: 0n })
  })

  it('keeps Duration objects as they are', () => {
    const settle = { nanos: 7, seconds: 3n }
    expect(durationsFromMillis(ScrollUntilBeginSchema, { settle }).settle).toBe(settle)
  })

  it('rejects milliseconds that are not finite', () => {
    expect(() => durationsFromMillis(ScrollUntilBeginSchema, { settle: Number.NaN })).toThrow(TypeError)
    expect(() => durationsFromMillis(ScrollUntilBeginSchema, { settle: Number.POSITIVE_INFINITY })).toThrow(TypeError)
  })

  it('sends millisecond options from Runner calls as protobuf Durations', async () => {
    const calls: UnaryCall[] = []
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
                event: { case: 'completed', value: { reason: ScrollUntilStopReason.END_BY_NO_VISUAL_PROGRESS, steps: 1 } },
              }))
            })(),
            async send(body: Uint8Array) { sent.push(body) },
          }
        },
        async unary(call) {
          calls.push(call)
          switch (call.method) {
            case '/auv.api.driver.v1.InputService/ClickPoint':
              return toBinary(ClickPointResponseSchema, create(ClickPointResponseSchema))
            case '/auv.api.driver.v1.InputService/PressKey':
              return toBinary(PressKeyResponseSchema, create(PressKeyResponseSchema))
            case '/auv.api.driver.v1.WindowService/ResolveWindow':
              return toBinary(ResolveWindowResponseSchema, create(ResolveWindowResponseSchema, {
                window: { ref: { windowId: 'w-1' } },
              }))
            default:
              throw new Error(`unexpected unary call: ${call.method}`)
          }
        },
      },
    })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
    const window = await runner.windows.resolve({ bundleId: 'com.example.App' })

    await window.click({ x: 1, y: 2 }, { click: { count: 2, interval: 80 } })
    await runner.input.pressKey('return', { settle: 120 })
    await window.scrollUntil({ x: 1, y: 2 }, { settle: 300, step: { case: 'instant', value: { deltaY: 100 } } })

    const click = fromBinary(ClickPointRequestSchema, calls.find(call => call.method.endsWith('/ClickPoint'))!.body)
    expect(click.options?.click?.interval).toMatchObject({ nanos: 80_000_000, seconds: 0n })
    const press = fromBinary(PressKeyRequestSchema, calls.find(call => call.method.endsWith('/PressKey'))!.body)
    expect(press.settle).toMatchObject({ nanos: 120_000_000, seconds: 0n })
    const begin = fromBinary(ScrollUntilRequestSchema, sent[0]!).event
    expect(begin.case === 'begin' && begin.value.settle).toMatchObject({ nanos: 300_000_000, seconds: 0n })
  })
})
