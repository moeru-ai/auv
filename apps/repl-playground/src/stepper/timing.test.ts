import type { StepSite } from './compile'

import { describe, expect, it } from 'vitest'

import { StepTimer } from './timing'

const sites: StepSite[] = [
  { async: true, endLine: 3, id: 0, line: 1, top: true },
  { async: true, endLine: 2, id: 1, line: 2, top: false },
  { async: true, endLine: 4, id: 2, line: 4, top: true },
]

describe('stepTimer', () => {
  it('attributes time to the line whose hook fired last and aggregates hits', () => {
    const timer = new StepTimer(sites)
    timer.step(0, 0)
    timer.step(1, 5)
    timer.step(1, 15)
    timer.step(2, 30)
    timer.finish(31)

    const snapshot = timer.snapshot()
    expect(snapshot.lines).toEqual([
      { hits: 1, line: 1, selfMs: 5 },
      { hits: 2, line: 2, selfMs: 25 },
      { hits: 1, line: 4, selfMs: 1 },
    ])
    expect(snapshot.statements).toEqual([
      { endLine: 3, line: 1, ms: 30, stepId: 0 },
      { endLine: 4, line: 4, ms: 1, stepId: 2 },
    ])
  })

  it('excludes paused time from line and statement durations', () => {
    const timer = new StepTimer(sites)
    timer.step(0, 0)
    timer.pause(2)
    timer.resume(102)
    timer.step(2, 110)
    timer.finish(110)

    expect(timer.snapshot().statements[0]!.ms).toBe(10)
    expect(timer.snapshot().lines[0]!.selfMs).toBe(10)
  })
})
