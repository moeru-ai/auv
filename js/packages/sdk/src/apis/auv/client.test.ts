import type { Transport } from '../../web/index'

import { create, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { ListDevicesResponseSchema } from '../../gen/auv/api/daemon/v1/device_pb'
import { connect, createAuv } from '../../web/index'

describe('aUV client lifecycle', () => {
  it('combines the client and call signals without dispatching aborted work', async () => {
    function createMockedTransport(calls: { value: number }): Transport {
      return {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected dispatch') },
        async unary() {
          calls.value += 1
          throw new Error('unexpected dispatch')
        },
      }
    }

    const page = new AbortController()
    const request = new AbortController()
    const calls = { value: 0 }

    const connection = await connect({ transport: createMockedTransport(calls) })
    const auv = createAuv(connection, { signal: page.signal })

    page.abort()
    await expect(auv.devices.list({ signal: request.signal })).rejects.toMatchObject({ name: 'AuvAbortError' })
    expect(calls.value).toBe(0)
  })

  it('does not retain the signal used only to establish a connection', async () => {
    function createMockedTransport(): Transport {
      return {
        close() {},
        async connect() {},
        async duplex() { throw new Error('unexpected dispatch') },
        async unary(_call) {
          return toBinary(ListDevicesResponseSchema, create(ListDevicesResponseSchema, { devices: [] }))
        },
      }
    }
    const establishment = new AbortController()

    const connection = await connect({
      signal: establishment.signal,
      transport: createMockedTransport(),
    })

    establishment.abort()
    await expect(createAuv(connection).devices.list()).resolves.toEqual([])
  })
})
