import type { Resource } from './store'

import { beforeEach, describe, expect, it } from 'vitest'

import { actions, usePlayground } from './store'

function frame(ref: string, capturedAt: number, bytes: number): Resource {
  const bounds = { height: 1, width: 1, x: 0, y: 0 }
  return {
    capturedAt,
    frame: { bounds, height: 1, native: { image: 'raw' }, rgba: new Uint8Array(bytes), scale: 1, source: 'window:1', width: 1 },
    handle: { $ref: ref as `frame:${string}`, bounds, height: 1, kind: 'frame', scale: 1, source: 'window:1', width: 1 },
    kind: 'frame',
    run: 1,
    seq: capturedAt,
  } as Resource
}

describe('releaseRawFrames', () => {
  beforeEach(() => {
    usePlayground.setState({ resources: {} })
    for (const [ref, at] of [['frame:1', 1], ['frame:2', 2], ['frame:3', 3]] as const)
      actions.putResource(ref, frame(ref, at, 100))
  })

  // ROOT CAUSE:
  //
  // If a script captured a Retina window repeatedly across runs, the browser
  // slowed down and grew by ~50 MB per capture because every frame kept its
  // raw RGBA forever so earlier handles stayed inspectable.
  //
  // The fix keeps raw pixels only for the newest captures within a budget;
  // older frames keep their handle and display bitmap.
  it('drops raw pixels of the oldest frames beyond the budget', () => {
    actions.releaseRawFrames(250)
    const { resources } = usePlayground.getState()
    const raw = (ref: string) => {
      const resource = resources[ref]
      return resource?.kind === 'frame' ? { bytes: resource.frame.rgba.byteLength, native: resource.frame.native !== undefined, released: resource.released === true } : undefined
    }
    expect(raw('frame:3')).toEqual({ bytes: 100, native: true, released: false })
    expect(raw('frame:2')).toEqual({ bytes: 100, native: true, released: false })
    expect(raw('frame:1')).toEqual({ bytes: 0, native: false, released: true })
    expect(resources['frame:1']?.kind === 'frame' && resources['frame:1'].handle.$ref).toBe('frame:1')
  })

  it('keeps everything within the budget', () => {
    actions.releaseRawFrames(1000)
    expect(Object.values(usePlayground.getState().resources).some(resource => resource.kind === 'frame' && resource.released)).toBe(false)
  })
})
