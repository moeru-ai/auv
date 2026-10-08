import type { DuplexCall, Transport, UnaryCall } from '@auv-js/sdk'

import type { Backend } from './types'

import { AuvRpcError } from '@auv-js/sdk'
import { describe, expect, it } from 'vitest'

import { RecordingBackend, ReplayBackend, ReplayDivergence } from './replay'

const bytes = (...values: number[]) => new Uint8Array(values)

async function collect(responses: AsyncIterable<Uint8Array>): Promise<number[]> {
  const out: number[] = []
  for await (const body of responses)
    out.push(body[0]!)
  return out
}

/** A live backend whose SDK transport answers with `transport`. */
function liveBackend(transport: Partial<Transport>): Backend {
  return {
    kind: 'auv',
    label: 'Device',
    sdk: () => ({
      describe: async () => undefined,
      route: { runnerClass: 'auv.core.local' },
      target: { transport: { close() {}, async connect() {}, ...transport } as Transport },
    }),
  } as unknown as Backend
}

/** Records `run` against a live transport, then returns the replay transport. */
async function recordThenReplay(live: Partial<Transport>, run: (transport: Transport) => Promise<void>): Promise<Transport> {
  const recorder = new RecordingBackend(liveBackend(live))
  await run(recorder.sdk!().target.transport)
  return new ReplayBackend(recorder.recording()).sdk!().target.transport
}

function unaryCall(method: string, body: Uint8Array, decodeJson: UnaryCall['decodeJson'] = () => bytes()): UnaryCall {
  return { body, decodeJson, headers: new Headers(), jsonBody: '{}', method }
}

describe('replay of direct SDK calls', () => {
  // ROOT CAUSE:
  //
  // Playground replay recorded only `auv.*` bindings, so a script that called
  // `@auv-js/sdk` directly could not re-run offline. Replay now answers the
  // encoded RPCs themselves.
  it('answers recorded unary calls by method and request, in any order', async () => {
    const replay = await recordThenReplay({ unary: async call => bytes(call.body[0]! * 10) }, async (transport) => {
      await transport.unary(unaryCall('/svc/A', bytes(1)))
      await transport.unary(unaryCall('/svc/A', bytes(2)))
    })

    expect(await replay.unary(unaryCall('/svc/A', bytes(2)))).toEqual(bytes(20))
    expect(await replay.unary(unaryCall('/svc/A', bytes(1)))).toEqual(bytes(10))
  })

  it('reports a call the recording does not have as a divergence', async () => {
    const replay = await recordThenReplay({ unary: async () => bytes(0) }, async (transport) => {
      await transport.unary(unaryCall('/svc/A', bytes(1)))
    })

    await expect(replay.unary(unaryCall('/svc/A', bytes(9)))).rejects.toBeInstanceOf(ReplayDivergence)
    await expect(replay.unary(unaryCall('/svc/B', bytes(1)))).rejects.toThrow('goes beyond the recording')
  })

  it('replays a recorded error as the same SDK error', async () => {
    const notFound = async (): Promise<Uint8Array> => {
      throw new AuvRpcError(5, 'window:9 was not found')
    }
    const replay = await recordThenReplay({ unary: notFound }, async (transport) => {
      await transport.unary(unaryCall('/svc/A', bytes(1))).catch(() => {})
    })

    await expect(replay.unary(unaryCall('/svc/A', bytes(1)))).rejects.toMatchObject({ message: 'window:9 was not found', rpcCode: 5 })
  })

  it('replays a JSON-encoded HTTP response through the caller\'s decoder', async () => {
    const replay = await recordThenReplay({ unary: async call => call.decodeJson('{"displays":[]}') }, async (transport) => {
      await transport.unary(unaryCall('/svc/A', bytes(1), () => bytes(7)))
    })

    const decoded: string[] = []
    await replay.unary(unaryCall('/svc/A', bytes(1), (text) => {
      decoded.push(text)
      return bytes()
    }))
    expect(decoded).toEqual(['{"displays":[]}'])
  })

  it('replays a stream in lockstep and stops at a different request', async () => {
    // A live stream that answers every request with request × 10.
    const live = {
      async duplex(): Promise<DuplexCall> {
        const queue: Array<null | Uint8Array> = []
        let wake: (() => void) | undefined
        return {
          close() {},
          async halfClose() {
            queue.push(null)
            wake?.()
          },
          responses: (async function* () {
            while (true) {
              while (queue.length === 0)
                await new Promise<void>((resolve) => { wake = resolve })
              const next = queue.shift()!
              if (next === null)
                return
              yield next
            }
          })(),
          async send(body) {
            queue.push(bytes(body[0]! * 10))
            wake?.()
          },
        }
      },
    }
    const exchange = async (transport: Transport, second: number) => {
      const call = await transport.duplex({ headers: new Headers(), method: '/svc/Stream' })
      const responses = call.responses[Symbol.asyncIterator]()
      await call.send(bytes(1))
      const first = await responses.next()
      await call.send(bytes(second))
      const rest = (async () => {
        const out = [first.value![0]!]
        for (let next = await responses.next(); !next.done; next = await responses.next())
          out.push(next.value[0]!)
        return out
      })()
      await call.halfClose()
      return await rest
    }
    const replay = () => recordThenReplay(live, async (transport) => {
      expect(await exchange(transport, 2)).toEqual([10, 20])
    })

    expect(await exchange(await replay(), 2)).toEqual([10, 20])
    // The first request matches, so the divergence is the second one.
    await expect(exchange(await replay(), 3)).rejects.toThrow('the script sent a different stream message')
  })

  it('keeps replaying a stream that a script reads to the end', async () => {
    const live = {
      async duplex(): Promise<DuplexCall> {
        return {
          close() {},
          async halfClose() {},
          responses: (async function* () {
            yield bytes(1)
            yield bytes(2)
          })(),
          async send() {},
        }
      },
    }
    const read = async (transport: Transport) => {
      const call = await transport.duplex({ headers: new Headers(), method: '/svc/Updates' })
      await call.send(bytes(0))
      return await collect(call.responses)
    }
    const replay = await recordThenReplay(live, async (transport) => {
      expect(await read(transport)).toEqual([1, 2])
    })

    expect(await read(replay)).toEqual([1, 2])
  })
})
