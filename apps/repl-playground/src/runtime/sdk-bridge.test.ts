import type { DuplexCall, Transport, UnaryCall } from '@auv-js/sdk'

import type { BridgedCallEnd, BridgedCallStart } from './sdk-bridge'

import { AuvRpcError, connect, createAuv } from '@auv-js/sdk'
import { createContext as createHostContext } from '@moeru/eventa/adapters/webworkers'
import { createContext as createWorkerContext } from '@moeru/eventa/adapters/webworkers/worker'
import { afterEach, describe, expect, it } from 'vitest'

import { createBridgeTransport, serveBridge } from './sdk-bridge'

const channels: MessageChannel[] = []

afterEach(() => {
  for (const channel of channels.splice(0)) {
    channel.port1.close()
    channel.port2.close()
  }
})

/** A worker-side SDK transport bridged over a MessageChannel to a host forwarding to `target`. */
function bridge(target: string | Transport, recorded: Array<{ end?: BridgedCallEnd, start: BridgedCallStart }> = []) {
  const channel = new MessageChannel()
  channels.push(channel)
  const { context: host } = createHostContext(channel.port1 as unknown as Worker)
  serveBridge(host, {
    record: (start) => {
      const entry: { end?: BridgedCallEnd, start: BridgedCallStart } = { start }
      recorded.push(entry)
      return (end) => {
        entry.end = end
      }
    },
    target: () => typeof target === 'string' ? target : { headers: { authorization: 'Bearer paired-token' }, transport: target },
  })
  const { context: worker } = createWorkerContext({ messagePort: channel.port2 as unknown as Worker })
  return createBridgeTransport(worker, () => 7)
}

function fakeDaemon(overrides: Partial<Transport> = {}): { calls: UnaryCall[], transport: Transport } {
  const calls: UnaryCall[] = []
  return {
    calls,
    transport: {
      close() {},
      async connect() {},
      async duplex() { throw new Error('unexpected duplex call') },
      async unary(call) {
        calls.push(call)
        return new Uint8Array()
      },
      ...overrides,
    },
  }
}

describe('sdk bridge', () => {
  // ROOT CAUSE:
  //
  // Playground scripts could only reach Runner RPCs that had a hand-written
  // binding. The bridge lets a script use `@auv-js/sdk` directly: encoded calls
  // cross to the host, which adds the credential and records them.
  it('forwards an SDK call to the host transport with the credential, and records it', async () => {
    const daemon = fakeDaemon()
    const recorded: Array<{ end?: BridgedCallEnd, start: BridgedCallStart }> = []
    const connection = await connect({ transport: bridge(daemon.transport, recorded) })
    const runner = createAuv(connection).runner({ runId: 'run-1', runnerClass: 'auv.core.local' })

    await runner.displays.list()

    const [call] = daemon.calls
    expect(call?.method).toBe('/auv.api.driver.v1.DisplayService/ListDisplays')
    expect(call?.headers.get('authorization')).toBe('Bearer paired-token')
    expect(call?.headers.get('auv-run-id')).toBe('run-1')
    expect(call?.http).toBeDefined()
    expect(recorded).toEqual([{
      end: { exchange: [{ request: expect.any(Uint8Array) }, { response: new Uint8Array() }], json: undefined },
      start: { body: expect.any(Uint8Array), method: '/auv.api.driver.v1.DisplayService/ListDisplays', stepId: 7 },
    }])
    await connection.close()
  })

  it('rebuilds remote errors as the same SDK error class', async () => {
    const daemon = fakeDaemon({
      unary: async () => {
        throw new AuvRpcError(5, 'window:9 was not found')
      },
    })
    const connection = await connect({ transport: bridge(daemon.transport) })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })

    await expect(runner.windows.from('9').capture()).rejects.toMatchObject({ message: 'window:9 was not found', name: 'AuvRpcError', rpcCode: 5 })
    await connection.close()
  })

  it('explains when the active backend cannot serve SDK calls', async () => {
    const connection = await connect({ transport: bridge('SDK calls need a connected device') })
    const runner = createAuv(connection).runner({ runnerClass: 'auv.core.local' })

    await expect(runner.displays.list()).rejects.toThrow('SDK calls need a connected device')
    await connection.close()
  })

  it('streams requests and responses both ways on one call', async () => {
    const sent: Uint8Array[] = []
    let push!: (body: null | Uint8Array) => void
    const responses = (async function* () {
      const queue: Array<null | Uint8Array> = []
      let wake: (() => void) | undefined
      push = (body) => {
        queue.push(body)
        wake?.()
      }
      while (true) {
        if (queue.length === 0)
          await new Promise<void>((resolve) => { wake = resolve })
        const next = queue.shift()!
        if (next === null)
          return
        yield next
      }
    })()
    const daemon = fakeDaemon({
      async duplex(call): Promise<DuplexCall> {
        expect(call.headers.get('authorization')).toBe('Bearer paired-token')
        return {
          close() {},
          async halfClose() {
            push(null)
          },
          responses,
          async send(body) {
            sent.push(body)
            push(new Uint8Array([body[0]! + 1]))
          },
        }
      },
    })
    const recorded: Array<{ end?: BridgedCallEnd, start: BridgedCallStart }> = []
    const transport = bridge(daemon.transport, recorded)

    const call = await transport.duplex({ headers: new Headers(), method: '/auv.api.driver.v1.InputService/ScrollUntil' })
    const received: number[] = []
    const reading = (async () => {
      for await (const body of call.responses)
        received.push(body[0]!)
    })()
    // Like a scroll-until decision, the second request answers the first response.
    await call.send(new Uint8Array([1]))
    await expect.poll(() => received.length).toBe(1)
    await call.send(new Uint8Array([10]))
    await call.halfClose()
    await reading

    expect(sent.map(body => body[0])).toEqual([1, 10])
    expect(received).toEqual([2, 11])
    // Replay needs every message of the call, in the order they crossed.
    await expect.poll(() => recorded[0]?.end).toBeDefined()
    expect(recorded[0]?.end?.exchange.map(frame => 'request' in frame ? `request ${frame.request[0]}` : `response ${frame.response[0]}`))
      .toEqual(['request 1', 'response 2', 'request 10', 'response 11'])
  })
})
