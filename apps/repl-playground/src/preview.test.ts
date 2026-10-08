import { describe, expect, it } from 'vitest'

import { REVEAL_MS, revealProgress } from './preview'

const bitmap = {} as ImageBitmap

describe('revealProgress', () => {
  it('shows only the preview until the pixels load', () => {
    expect(revealProgress({ preview: bitmap }, 1_000)).toBe(0)
  })

  it('shows pixels that loaded before any preview at once', () => {
    expect(revealProgress({ bitmap }, 1_000)).toBe(1)
  })

  it('fades pixels in over the preview, eased, then holds them', () => {
    const image = { bitmap, preview: bitmap, revealedAt: 1_000 }
    expect(revealProgress(image, 1_000)).toBe(0)
    expect(revealProgress(image, 1_000 + REVEAL_MS / 2)).toBeCloseTo(0.5)
    expect(revealProgress(image, 1_000 + REVEAL_MS / 4)).toBeLessThan(0.25)
    expect(revealProgress(image, 1_000 + REVEAL_MS)).toBe(1)
    expect(revealProgress(image, 1_000 + REVEAL_MS * 3)).toBe(1)
  })
})
