import type { CallRecord, PlaygroundState, Resource } from './store'

import { describe, expect, it } from 'vitest'

import { bindingsOf, consumersOf, latestFramesAt } from './timeline'

function frame(ref: string, seq: number, run: number): Resource {
  return {
    capturedAt: seq,
    frame: { bounds: { height: 1, width: 1, x: 0, y: 0 }, height: 1, ref: 'cap-1', scale: 1, source: 'window:w-counter', width: 1 },
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

describe('lineage by Runner IDs', () => {
  const V1 = 'auv.api.driver.v1'
  const window = { $typeName: `${V1}.Window`, frame: { height: 600, width: 800, x: 0, y: 0 }, ref: { $typeName: `${V1}.WindowRef`, windowId: 'w-1' } }

  it('finds the SDK calls whose requests name a window or a capture', () => {
    // Recorded SDK call arguments are ProtoJSON requests.
    const calls = [
      { args: [{ window: { windowId: 'w-1' } }], id: 1 },
      { args: [{ source: { captureRef: { captureId: 'cap-1' } } }], id: 2 },
      { args: [{ position: { windowId: 'w-1', x: 5, y: 5 } }], id: 3 },
      { args: [{ window: { windowId: 'w-2' } }], id: 4 },
    ] as unknown as CallRecord[]

    expect(consumersOf(calls, 'window:w-1').map(call => call.id)).toEqual([1, 3])
    expect(consumersOf(calls, 'frame:cap-1').map(call => call.id)).toEqual([2])
  })

  it('finds names bound to SDK values that hold the resource', () => {
    const binds = [
      // A `WindowClient` as it crosses from the worker: methods become `$fn`.
      { hit: 1, line: 2, seq: 3, values: { music: { click: { $fn: 'click' }, id: 'w-1', window } } },
      { hit: 1, line: 3, seq: 6, values: { shot: { capture: { ref: { $typeName: `${V1}.CaptureRef`, captureId: 'cap-1' } }, window } } },
      // A protobuf-es oneof names the window too.
      { hit: 1, line: 4, seq: 9, values: { at: { coordinateSpace: { case: 'windowId', value: 'w-1' }, x: 1, y: 2 } } },
    ] as unknown as PlaygroundState['binds']

    expect(bindingsOf(binds, 'window:w-1').map(bind => bind.name)).toEqual(['music', 'shot', 'at'])
    expect(bindingsOf(binds, 'frame:cap-1').map(bind => bind.name)).toEqual(['shot'])
  })
})
