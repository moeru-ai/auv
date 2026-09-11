import { describe, expect, it } from 'vitest'

import * as auv from './index'

describe('browser entry', () => {
  it('loads in a real browser and calls AUV through browser HTTP primitives', async () => {
    expect('createGrpcTransport' in auv).toBe(false)
    expect('startAuv' in auv).toBe(false)

    let request: undefined | { init?: RequestInit, input: string }
    const connection = await auv.connect({
      credential: 'browser-token',
      transport: auv.createHttpTransport({
        endpoint: 'http://auv.example:9847',
        fetch: async (input, init) => {
          request = { init, input: input.toString() }
          return new Response('{}')
        },
      }),
    })

    await expect(auv.listDevices(connection)).resolves.toEqual([])
    expect(request?.input).toBe('http://auv.example:9847/apis/auv/daemon/v1/devices')
    expect(new Headers(request?.init?.headers).get('authorization')).toBe('Bearer browser-token')
  })
})
