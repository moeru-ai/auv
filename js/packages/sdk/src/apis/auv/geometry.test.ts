import { describe, expect, it } from 'vitest'

import { above, at, below, center, contains, inset, intersect, leftOf, offset, Position, region, rightOf } from './geometry'

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

  it('builds strips, insets and points in the same space as the rectangle', () => {
    const rect = { height: 50, width: 200, x: 100, y: 100 }
    expect(above(rect, 20, 4)).toEqual({ height: 20, width: 200, x: 100, y: 76 })
    expect(below(rect, 20)).toEqual({ height: 20, width: 200, x: 100, y: 150 })
    expect(leftOf(rect, 30, 2)).toEqual({ height: 50, width: 30, x: 68, y: 100 })
    expect(rightOf(rect, 30)).toEqual({ height: 50, width: 30, x: 300, y: 100 })
    expect(inset(rect, 10)).toEqual({ height: 30, width: 180, x: 110, y: 110 })
    expect(inset(rect, { left: 20, top: 5 })).toEqual({ height: 45, width: 180, x: 120, y: 105 })
    expect(offset(rect, -10, 5)).toEqual({ height: 50, width: 200, x: 90, y: 105 })
    expect(at(rect, 0, 0)).toEqual({ x: 100, y: 100 })
    expect(at(rect, 0.5, 0.5)).toEqual(center(rect))
  })

  it('places a region by two of start, end and size per axis, with percentages', () => {
    expect(region(frame, { height: 80, top: '10%' })).toEqual({ height: 80, width: 800, x: 100, y: 110 })
    // A missing start is 0; a missing size fills the rest.
    expect(region(frame, { right: 100, width: '25%' })).toEqual({ height: 600, width: 200, x: 600, y: 50 })
    expect(region(frame, { bottom: 0, left: 40, top: '50%' })).toEqual({ height: 300, width: 760, x: 140, y: 350 })
    expect(() => region(frame, { left: 0, right: 0, width: 10 })).toThrow(RangeError)
    expect(() => region(frame, { top: '10px' as `${number}%` })).toThrow(TypeError)
  })

  it('builds positions in each coordinate space from what callers hold', () => {
    expect(Position.screen(10, 20)).toEqual({ coordinateSpace: { case: 'screen', value: true }, x: 10, y: 20 })
    expect(Position.display({ displayId: 'd-2' }, 1, 2)).toEqual({ coordinateSpace: { case: 'displayId', value: 'd-2' }, x: 1, y: 2 })
    for (const target of ['w-1', { windowId: 'w-1' }, { ref: { windowId: 'w-1' } }] as const)
      expect(Position.window(target as Parameters<typeof Position.window>[0], 5, 6).coordinateSpace).toEqual({ case: 'windowId', value: 'w-1' })
    expect(() => Position.window({ windowId: '' } as Parameters<typeof Position.window>[0], 0, 0)).toThrow(TypeError)
  })
})
