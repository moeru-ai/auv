import type { Resource } from './store'

import { describe, expect, it } from 'vitest'

import { boundsOf, refOf } from './handles'

const V1 = 'auv.api.driver.v1'
const frame = { height: 600, width: 800, x: 100, y: 50 }
const window = { $typeName: `${V1}.Window`, frame, ref: { $typeName: `${V1}.WindowRef`, windowId: 'w-1' }, title: 'Inbox' }

describe('refOf', () => {
  it('names the resource an SDK value stands for', () => {
    expect(refOf(window)).toBe('window:w-1')
    expect(refOf({ capture: { $fn: 'capture' }, id: 'w-1', window })).toBe('window:w-1')
    expect(refOf({ $typeName: `${V1}.CapturedFrame`, ref: { captureId: 'cap-1' } })).toBe('frame:cap-1')
    expect(refOf({ $typeName: `${V1}.Display`, displayId: 'd-1' })).toBe('display:d-1')
    // ProtoJSON refs in recorded call arguments.
    expect(refOf({ windowId: 'w-1' })).toBe('window:w-1')
    expect(refOf({ $ref: 'text:3', kind: 'text' })).toBe('text:3')
  })

  it('leaves values that only mention a resource as data', () => {
    // A ProtoJSON window position: the point matters, not just the window.
    expect(refOf({ windowId: 'w-1', x: 5, y: 5 })).toBeUndefined()
    expect(refOf({ capture: { ref: { captureId: 'cap-1' } }, window })).toBeUndefined()
    expect(refOf({ $typeName: `${V1}.ScreenRect`, ...frame })).toBeUndefined()
  })
})

describe('boundsOf', () => {
  it('focuses an SDK window client on its recorded window', () => {
    const resources = { 'window:w-1': { handle: { frame: { height: 10, width: 10, x: 1, y: 2 } }, kind: 'window' } } as unknown as Record<string, Resource>
    expect(boundsOf({ id: 'w-1', window }, resources)).toEqual({ height: 10, width: 10, x: 1, y: 2 })
  })
})
