import { describe, expect, it } from 'vitest'

import { areaOf } from './area'

const window = { $ref: 'window:7', frame: { height: 600, width: 1000, x: 100, y: 50 }, kind: 'window' } as const

function box(area: { height: number, width: number, x: number, y: number }) {
  return { height: area.height, width: area.width, x: area.x, y: area.y }
}

describe('areaOf', () => {
  it('starts from a window frame and remembers the handle', () => {
    const area = areaOf(window as never, 'music')
    expect(box(area)).toEqual(window.frame)
    expect(area.from).toBe('window:7')
    expect(area.label).toBe('music')
  })

  it('starts from an OCR match through its bounds', () => {
    const area = areaOf({ bounds: { height: 20, width: 80, x: 10, y: 30 }, confidence: 1, text: 'Done' })
    expect(box(area)).toEqual({ height: 20, width: 80, x: 10, y: 30 })
    expect(area.from).toBeUndefined()
  })

  it('rejects values without bounds', () => {
    expect(() => areaOf({ x: 1, y: 2 } as never)).toThrow(TypeError)
  })
})

describe('area.region', () => {
  const music = areaOf(window as never)

  it('pins a fixed-size box by its top-left distances', () => {
    expect(box(music.region({ height: 36, left: 400, top: 32, width: 256 }))).toEqual({ height: 36, width: 256, x: 500, y: 82 })
  })

  it('pins a fixed-size box to the right and bottom edges', () => {
    expect(box(music.region({ bottom: 10, height: 40, right: 20, width: 100 }))).toEqual({ height: 40, width: 100, x: 980, y: 600 })
  })

  it('stretches between two edges and resolves percentages against the parent', () => {
    expect(box(music.region({ bottom: 80, left: '25%', right: 0, top: 120 }))).toEqual({ height: 400, width: 750, x: 350, y: 170 })
  })

  it('rejects an over-constrained axis instead of silently picking two edges', () => {
    expect(() => music.region({ left: 10, right: 10, width: 100 })).toThrow(RangeError)
  })

  it('keeps the source handle but not the parent label on derived areas', () => {
    const derived = areaOf(window as never, 'music').region({ height: 10, width: 10 })
    expect(derived.from).toBe('window:7')
    expect(derived.label).toBeUndefined()
  })
})

describe('area relations', () => {
  const search = areaOf({ height: 36, width: 256, x: 500, y: 82 })

  it('builds adjacent strips with a gap', () => {
    expect(box(search.below(40, 8))).toEqual({ height: 40, width: 256, x: 500, y: 126 })
    expect(box(search.rightOf(30, 4))).toEqual({ height: 36, width: 30, x: 760, y: 82 })
  })

  it('maps fractional positions and the center', () => {
    expect(search.center).toEqual({ x: 628, y: 100 })
    expect(search.at(0, 1)).toEqual({ x: 500, y: 118 })
  })

  it('checks containment for points and rectangles', () => {
    expect(search.contains({ x: 510, y: 90 })).toBe(true)
    expect(search.contains({ height: 10, width: 300, x: 510, y: 90 })).toBe(false)
  })

  it('crosses to the host as plain data without methods', () => {
    const wire = Object.fromEntries(Object.entries(search.named('search')))
    expect(wire).toEqual({ from: undefined, height: 36, kind: 'area', label: 'search', width: 256, x: 500, y: 82 })
  })
})
