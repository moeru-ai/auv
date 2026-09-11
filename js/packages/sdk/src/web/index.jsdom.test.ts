import type { AuvAbortError } from './index'

import { describe, expect, it } from 'vitest'

import { connect, createHttpTransport, listDevices } from './index'

describe('browser entry in a DOM-like application', () => {
  it('uses the jsdom web platform and propagates AbortSignal cancellation', async () => {
    const controller = new AbortController()
    const reason = new DOMException('view disconnected', 'AbortError')
    const connection = await connect({
      transport: createHttpTransport({
        fetch: async (_input, init) => new Promise((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => reject(init.signal?.reason), { once: true })
        }),
      }),
    })
    const pending = listDevices(connection, { signal: controller.signal })

    controller.abort(reason)

    await expect(pending).rejects.toEqual(expect.objectContaining<AuvAbortError>({
      cause: reason,
      message: 'AUV operation was aborted',
      name: 'AuvAbortError',
    }))
  })
})
