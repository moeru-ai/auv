import type { PlaygroundState, Resource } from './store'

import { describe, expect, it } from 'vitest'

import { latestFramesAt } from './timeline'

function frame(ref: string, seq: number, run: number): Resource {
  return {
    capturedAt: seq,
    frame: { bounds: { height: 1, width: 1, x: 0, y: 0 }, height: 1, rgba: new Uint8Array(4), scale: 1, source: 'window:w-counter', width: 1 },
    handle: { $ref: ref as `frame:${string}`, bounds: { height: 1, width: 1, x: 0, y: 0 }, height: 1, kind: 'frame', scale: 1, source: 'window:w-counter', width: 1 },
    kind: 'frame',
    run,
    seq,
  } as Resource
}

describe('latestFramesAt', () => {
  // ROOT CAUSE:
  //
  // If the same script ran more than once, the canvas could show a frame from
  // an earlier run (e.g. "Count: 3" while this run already counted to 15)
  // because event-log seqs restart at 0 for every run while resources from
  // earlier runs are kept for inspection.
  //
  // Before the fix, the newest frame with `seq <= cursor` across all runs won.
  // The fix keeps time-travel queries scoped to the current run.
  it('ignores frames from earlier runs whose seq overlaps the current run', () => {
    const state = {
      resources: {
        'frame:1': frame('frame:1', 25, 1),
        'frame:9': frame('frame:9', 20, 2),
      },
      runIndex: 2,
    } as unknown as PlaygroundState

    expect(latestFramesAt(state, 30).map(resource => resource.handle.$ref)).toEqual(['frame:9'])
  })
})
