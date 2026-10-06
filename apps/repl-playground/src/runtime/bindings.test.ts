import type { Backend, NormalizedRect, ScrollUntilRequest } from '../backend/types'
import type { InputHandle, ScrollObservation, ScrollUntilResult, TextHandle } from '../script-api/api'

import { beforeEach, describe, expect, it } from 'vitest'

import { actions, usePlayground } from '../store'
import { invokeBinding } from './bindings'

const frame = { height: 600, width: 1000, x: 100, y: 50 }
let seq = 0
const context = { hit: 1, line: 1, nextSeq: () => seq++ }

/** Records the OCR region the binding asks the backend for. */
function searchBackend(): { backend: Backend, regions: Array<NormalizedRect | undefined> } {
  const regions: Array<NormalizedRect | undefined> = []
  const backend = {
    findWindowText: async (_id: string, _query: string, region?: NormalizedRect) => {
      regions.push(region)
      return { matches: [] }
    },
  } as unknown as Backend
  return { backend, regions }
}

describe('windows.findText within', () => {
  beforeEach(() => {
    seq = 0
    usePlayground.setState({ calls: [], resources: {} })
    const handle = { $ref: 'window:7' as const, frame, id: '7', kind: 'window' as const }
    actions.putResource('window:7', { callId: 0, handle, kind: 'window', run: usePlayground.getState().runIndex, seq: 0 })
  })

  it('sends the area as fractions of the window and records it on the result', async () => {
    const { backend, regions } = searchBackend()
    const within = { height: 60, width: 250, x: 350, y: 110 }
    const result = await invokeBinding(backend, 'windows.findText', [{ $ref: 'window:7' }, 'Remember', { within }], context) as TextHandle
    expect(regions[0]).toEqual({ height: 0.1, width: 0.25, x: 0.25, y: 0.1 })
    expect(result.within).toEqual(within)
  })

  it('clips an area that reaches outside the window', async () => {
    const { backend, regions } = searchBackend()
    await invokeBinding(backend, 'windows.findText', [{ $ref: 'window:7' }, 'x', { within: { height: 100, width: 200, x: 1000, y: 0 } }], context)
    expect(regions[0]).toEqual({ height: 50 / 600, width: 0.1, x: 0.9, y: 0 })
  })

  it('rejects an area that does not overlap the window instead of searching everything', async () => {
    const { backend, regions } = searchBackend()
    await expect(invokeBinding(backend, 'windows.findText', [{ $ref: 'window:7' }, 'x', { within: { height: 10, width: 10, x: 0, y: 0 } }], context)).rejects.toThrow('does not overlap')
    expect(regions).toEqual([])
  })

  it('searches the whole window without options', async () => {
    const { backend, regions } = searchBackend()
    await invokeBinding(backend, 'windows.findText', [{ $ref: 'window:7' }, 'x'], context)
    expect(regions).toEqual([undefined])
  })
})

describe('windows.scroll', () => {
  beforeEach(() => {
    seq = 0
    usePlayground.setState({ calls: [], resources: {} })
    const handle = { $ref: 'window:7' as const, frame, id: '7', kind: 'window' as const }
    actions.putResource('window:7', { callId: 0, handle, kind: 'window', run: usePlayground.getState().runIndex, seq: 0 })
  })

  it('scrolls at an area center converted to window-local coordinates and records the delta', async () => {
    const points: unknown[] = []
    const backend = {
      scrollWindow: async (_id: string, point: unknown) => {
        points.push(point)
        return { path: 'window-targeted-wheel' }
      },
    } as unknown as Backend
    const result = await invokeBinding(backend, 'windows.scroll', [{ $ref: 'window:7' }, { height: 100, width: 200, x: 300, y: 150 }, { dy: 600 }], context) as InputHandle
    // Area center (400, 200) minus the window origin (100, 50).
    expect(points[0]).toEqual({ x: 300, y: 150 })
    expect(result).toMatchObject({ action: 'scroll', delta: { dx: 0, dy: 600 }, path: 'window-targeted-wheel', point: { x: 400, y: 200 } })
  })

  it('rejects a point outside the window instead of scrolling another app', async () => {
    const backend = { scrollWindow: async () => ({}) } as unknown as Backend
    await expect(invokeBinding(backend, 'windows.scroll', [{ $ref: 'window:7' }, { x: 10, y: 10 }, { dy: 100 }], context)).rejects.toThrow('outside the window')
  })
})

describe('windows.scrollUntil', () => {
  beforeEach(() => {
    seq = 0
    usePlayground.setState({ calls: [], resources: {} })
    const handle = { $ref: 'window:7' as const, frame, id: '7', kind: 'window' as const }
    actions.putResource('window:7', { callId: 0, handle, kind: 'window', run: usePlayground.getState().runIndex, seq: 0 })
  })

  it('applies the CLI defaults and asks the script predicate through decide', async () => {
    const requests: ScrollUntilRequest[] = []
    const asked: Array<[number, ScrollObservation]> = []
    const backend = {
      scrollWindowUntil: async (_id: string, _point: unknown, request: ScrollUntilRequest, decide?: (observation: ScrollObservation) => Promise<boolean>) => {
        requests.push(request)
        const stop = await decide?.({ moved: true, steps: 1, text: 'Remember' })
        return { reason: stop ? 'until' : 'end', steps: 1 }
      },
    } as unknown as Backend
    const decide = async (predicateId: number, observation: ScrollObservation) => {
      asked.push([predicateId, observation])
      return observation.text.includes('Remember')
    }
    const result = await invokeBinding(backend, 'windows.scrollUntil', [{ $ref: 'window:7' }, { x: 600, y: 350 }, { dy: 400, until: { $predicate: 3 } }], { ...context, decide }) as ScrollUntilResult
    expect(requests[0]).toEqual({ confirmations: 2, delta: { dx: 0, dy: 400 }, maxSteps: 50, settleMs: 400, text: undefined })
    expect(asked).toEqual([[3, { moved: true, steps: 1, text: 'Remember' }]])
    expect(result).toMatchObject({ reason: 'until', steps: 1 })
  })

  it('rejects scrolling along both axes', async () => {
    const backend = { scrollWindowUntil: async () => ({ reason: 'end', steps: 0 }) } as unknown as Backend
    await expect(invokeBinding(backend, 'windows.scrollUntil', [{ $ref: 'window:7' }, { x: 600, y: 350 }, { dx: 10, dy: 10 }], context)).rejects.toThrow('one axis')
  })
})
