import { describe, expect, it } from 'vitest'

import { describeError } from './episode'

describe('describeError', () => {
  // ROOT CAUSE:
  //
  // If a guest setup request lost its port-forward, Node's fetch threw
  // `fetch failed` with the socket error only in `cause`. result.json kept
  // just the message (chrome/6766f2b8 in the 2026-10-11 test_nogdrive run),
  // so the transport reason was lost.
  //
  // The fix records the whole cause chain.
  it('keeps the cause chain of a failed fetch', () => {
    const socket = Object.assign(new Error('other side closed'), { code: 'UND_ERR_SOCKET' })
    const error = new TypeError('fetch failed', { cause: socket })
    expect(describeError(error)).toBe('fetch failed: other side closed')
  })

  it('describes a non-Error value', () => {
    expect(describeError('aborted')).toBe('aborted')
  })
})
