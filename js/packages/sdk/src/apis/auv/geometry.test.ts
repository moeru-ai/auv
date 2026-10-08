import { describe, expect, it } from 'vitest'

import { center, contains, intersect, Position } from './geometry'

const frame = { height: 600, width: 800, x: 100, y: 50 }

describe('geometry helpers', () => {
  it('finds the center of any rectangle, such as a text match', () => {
    expect(center({ height: 20, width: 60, x: 300, y: 200 })).toEqual({ x: 330, y: 210 })
  })

  it('treats edges as inside, for points and whole rectangles', () => {
    expect(contains(frame, { x: 100, y: 50 })).toBe(true)
    expect(contains(frame, { x: 900, y: 650 })).toBe(true)
    expect(contains(frame, { x: 99, y: 60 })).toBe(false)
    expect(contains(frame, { height: 600, width: 800, x: 100, y: 50 })).toBe(true)
    expect(contains(frame, { height: 10, width: 10, x: 895, y: 60 })).toBe(false)
  })

  it('clips a rectangle to a frame, and reports no overlap as undefined', () => {
    expect(intersect({ height: 100, width: 100, x: 850, y: 0 }, frame)).toEqual({ height: 50, width: 50, x: 850, y: 50 })
    // Touching edges share no area.
    expect(intersect({ height: 10, width: 10, x: 900, y: 50 }, frame)).toBeUndefined()
  })

  it('builds positions in each coordinate space from what callers hold', () => {
    expect(Position.screen(10, 20)).toEqual({ coordinateSpace: { case: 'screen', value: true }, x: 10, y: 20 })
    expect(Position.display({ displayId: 'd-2' }, 1, 2)).toEqual({ coordinateSpace: { case: 'displayId', value: 'd-2' }, x: 1, y: 2 })
    for (const target of ['w-1', { windowId: 'w-1' }, { ref: { windowId: 'w-1' } }] as const)
      expect(Position.window(target as Parameters<typeof Position.window>[0], 5, 6).coordinateSpace).toEqual({ case: 'windowId', value: 'w-1' })
    expect(() => Position.window({ windowId: '' } as Parameters<typeof Position.window>[0], 0, 0)).toThrow(TypeError)
  })
})
