import type { InputHandle, TextHandle } from '../script-api/api'

import { beforeEach, describe, expect, it } from 'vitest'

import { usePlayground } from '../store'
import { CallScope } from './bindings'
import { recordRpcResources } from './rpc-resources'

const V1 = 'auv.api.driver.v1'

function message<T extends Record<string, unknown>>(typeName: string, fields: T): T & { $typeName: string } {
  return { $typeName: `${V1}.${typeName}`, ...fields }
}

const window = message('Window', {
  applicationName: 'Music',
  frame: message('ScreenRect', { height: 600, width: 800, x: 100, y: 50 }),
  ref: message('WindowRef', { windowId: 'w-1' }),
  title: 'Liked Songs',
})

function capture(id: string) {
  return message('CapturedFrame', {
    bounds: message('ScreenRect', { height: 600, width: 800, x: 100, y: 50 }),
    pixelSize: message('PixelSize', { height: 1200, width: 1600 }),
    ref: message('CaptureRef', { captureId: id }),
    scaleFactor: 2,
    thumbhash: new Uint8Array(),
  })
}

// INPUT_DELIVERY_PATH_AX_PRESS
const action = message('InputActionResult', { selectedPath: 2 })

function resourcesOf(scope: CallScope) {
  const { resources } = usePlayground.getState()
  return scope.refs.map(ref => resources[ref]!)
}

describe('recordRpcResources', () => {
  beforeEach(() => {
    usePlayground.setState({ resources: {} })
  })

  it('shows a window text search as the window, its frame and the matches in it', () => {
    const scope = new CallScope(1, 0, null)
    recordRpcResources(scope, `/${V1}.TextRecognitionService/FindWindowText`, message('FindWindowTextRequest', { query: 'Reply', window: message('WindowRef', { windowId: 'w-1' }) }), [
      message('FindWindowTextResponse', {
        capture: capture('cap-1'),
        matches: [message('TextMatch', { bounds: message('ScreenRect', { height: 20, width: 60, x: 300, y: 200 }), confidence: 0.9, text: 'Reply' })],
        window,
      }),
    ])

    const [frame, text, found] = resourcesOf(scope)
    expect(frame).toMatchObject({ frame: { ref: 'cap-1', source: 'window:w-1' }, handle: { $ref: 'frame:cap-1' }, kind: 'frame' })
    expect(text?.kind).toBe('text')
    const handle = text!.handle as TextHandle
    expect(handle.query).toBe('Reply')
    expect(handle.frame?.$ref).toBe(frame!.handle.$ref)
    expect(handle.matches).toEqual([{ bounds: { height: 20, width: 60, x: 300, y: 200 }, confidence: 0.9, text: 'Reply' }])
    expect(found).toMatchObject({ handle: { $ref: 'window:w-1', title: 'Liked Songs' }, kind: 'window' })
  })

  it('shows a click as a receipt at its delivered screen point', () => {
    const scope = new CallScope(2, 0, null)
    recordRpcResources(scope, `/${V1}.InputService/ClickPoint`, undefined, [
      message('ClickPointResponse', { action, screenPoint: message('ScreenPoint', { x: 640, y: 400 }), window }),
    ])

    const input = resourcesOf(scope).find(resource => resource.kind === 'input')!.handle as InputHandle
    expect(input).toMatchObject({ action: 'click', path: 'ax-press', point: { x: 640, y: 400 } })
  })

  it('places a window scroll at the window point plus the window origin', () => {
    const scope = new CallScope(3, 0, null)
    recordRpcResources(scope, `/${V1}.InputService/ScrollWindowPoint`, undefined, [
      message('ScrollWindowPointResponse', { action, point: message('WindowPoint', { x: 10, y: 20 }), scroll: message('Scroll', { deltaX: 0, deltaY: 300 }), window }),
    ])

    const input = resourcesOf(scope).find(resource => resource.kind === 'input')!.handle as InputHandle
    expect(input).toMatchObject({ action: 'scroll', delta: { dx: 0, dy: 300 }, point: { x: 110, y: 70 } })
  })

  it('shows every scroll-until step, inside its stream event', () => {
    const scope = new CallScope(4, 0, null)
    const update = (id: string) => message('ScrollUntilResponse', { event: { case: 'update', value: message('ScrollUntilUpdate', { capture: capture(id), steps: 1 }) } })
    recordRpcResources(scope, `/${V1}.InputService/ScrollUntil`, message('ScrollUntilRequest', { event: { case: 'begin', value: message('ScrollUntilBegin', { window: message('WindowRef', { windowId: 'w-1' }) }) } }), [update('cap-1'), update('cap-2')])

    const frames = resourcesOf(scope).filter(resource => resource.kind === 'frame')
    expect(frames.map(frame => frame.kind === 'frame' && [frame.frame.ref, frame.frame.source])).toEqual([['cap-1', 'window:w-1'], ['cap-2', 'window:w-1']])
  })

  it('keeps a capture seen again in the same run as one resource with its first producer', () => {
    const first = new CallScope(1, 0, null)
    recordRpcResources(first, `/${V1}.CaptureService/CaptureWindow`, message('CaptureWindowRequest', { window: message('WindowRef', { windowId: 'w-1' }) }), [
      message('CaptureWindowResponse', { capture: capture('cap-7'), window }),
    ])
    const again = new CallScope(2, 5, null)
    recordRpcResources(again, `/${V1}.TextRecognitionService/FindWindowText`, message('FindWindowTextRequest', { query: 'x', window: message('WindowRef', { windowId: 'w-1' }) }), [
      message('FindWindowTextResponse', { capture: capture('cap-7'), matches: [], window }),
    ])

    expect(again.refs).toContain('frame:cap-7')
    expect(usePlayground.getState().resources['frame:cap-7']).toMatchObject({ callId: 1, seq: 0 })
  })
})
