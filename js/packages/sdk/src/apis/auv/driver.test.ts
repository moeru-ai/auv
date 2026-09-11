import type { UnaryCall } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { CaptureWindowRequestSchema, CaptureWindowResponseSchema } from '../../gen/auv/api/driver/v1/capture_pb'
import { ListDisplaysResponseSchema } from '../../gen/auv/api/driver/v1/display_pb'
import { ResolveWindowRequestSchema, ResolveWindowResponseSchema } from '../../gen/auv/api/driver/v1/window_pb'
import { connect } from '../../node/index'
import { createAuv } from './client'

describe('runner Driver control surface', () => {
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
    expect(Object.keys(window).sort()).toEqual(['capture', 'click', 'findText', 'id'])

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
