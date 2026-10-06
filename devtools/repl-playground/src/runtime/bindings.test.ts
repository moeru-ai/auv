import type { Backend, NormalizedRect } from '../backend/types'
import type { TextHandle } from '../script-api/api'

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
