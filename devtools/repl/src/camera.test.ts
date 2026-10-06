import { describe, expect, it } from 'vitest'

import { flight, flightDuration, viewFor } from './camera'

const W = 800
const H = 600
const start = { k: 0.3, tx: 20, ty: 20 }

function close(a: { k: number, tx: number, ty: number }, b: { k: number, tx: number, ty: number }) {
  expect(a.k).toBeCloseTo(b.k, 6)
  expect(a.tx).toBeCloseTo(b.tx, 3)
  expect(a.ty).toBeCloseTo(b.ty, 3)
}

describe('viewFor', () => {
  it('centers the rect and fits it with a margin', () => {
    const view = viewFor({ height: 100, width: 400, x: 1000, y: 500 }, W, H, start, true)
    expect(view.k).toBeCloseTo(1.6) // 800 * 0.8 / 400
    // The rect's center lands on the canvas center.
    expect(1200 * view.k + view.tx).toBeCloseTo(W / 2)
    expect(550 * view.k + view.ty).toBeCloseTo(H / 2)
  })

  it('keeps context around small targets instead of magnifying them', () => {
    const view = viewFor({ height: 20, width: 80, x: 0, y: 0 }, W, H, start, true)
    expect(W / view.k).toBeGreaterThanOrEqual(360)
    expect(H / view.k).toBeGreaterThanOrEqual(240)
  })

  it('keeps the current scale without zoom', () => {
    expect(viewFor({ height: 10, width: 10, x: 0, y: 0 }, W, H, start, false).k).toBe(start.k)
  })
})

describe('flight', () => {
  const near = viewFor({ height: 60, width: 200, x: 100, y: 100 }, W, H, start, true)
  const far = viewFor({ height: 60, width: 200, x: 3000, y: 1800 }, W, H, start, true)

  it('starts and ends exactly on the two views', () => {
    for (const arc of [true, false]) {
      const path = flight(near, far, W, H, arc)
      close(path.at(0), near)
      close(path.at(1), far)
    }
  })

  // The zoom-out-then-in arc: between two zoomed-in views far apart, the
  // camera rises (smaller scale) mid-way instead of panning at full zoom.
  it('zooms out mid-way on a far jump with arc, but not without', () => {
    expect(flight(near, far, W, H, true).at(0.5).k).toBeLessThan(near.k * 0.5)
    expect(flight(near, far, W, H, false).at(0.5).k).toBeCloseTo(near.k, 6)
  })

  it('takes longer for farther jumps', () => {
    const nearby = viewFor({ height: 60, width: 200, x: 400, y: 100 }, W, H, start, true)
    expect(flight(near, far, W, H, true).duration).toBeGreaterThan(flight(near, nearby, W, H, true).duration)
  })
})

describe('flightDuration', () => {
  it('keeps flights readable and fits them into the time until the next focus', () => {
    expect(flightDuration(50)).toBe(280)
    expect(flightDuration(5000)).toBe(1400)
    expect(flightDuration(1000, 400)).toBe(340)
    expect(flightDuration(1000, 10)).toBe(160)
  })
})
