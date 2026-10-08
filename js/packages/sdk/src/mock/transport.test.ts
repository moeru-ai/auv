import { describe, expect, it } from 'vitest'

import { createAuv, discoverRunner } from '../apis'
import { InputService, ScrollUntilStopReason } from '../gen/auv/api/driver/v1/input_pb'
import { WindowService } from '../gen/auv/api/driver/v1/window_pb'
import { connectTransport } from '../transport/connection'
import { AuvRpcError } from '../transport/errors'
import { serveMockDaemon } from './daemon'
import { createMockTransport } from './transport'

const window = { applicationBundleId: 'dev.auv.mock.music', frame: { height: 580, width: 480, x: 920, y: 120 }, ref: { windowId: 'w-music' }, title: 'Music' }

async function mockRunner(define: Parameters<typeof createMockTransport>[0]) {
  const connection = await connectTransport(createMockTransport(define))
  return { auv: createAuv(connection), connection }
}

describe('mock Runner transport', () => {
  it('answers SDK calls from typed service implementations', async () => {
    const { auv, connection } = await mockRunner(({ service }) => {
      service(WindowService, {
        listWindows: () => ({ windows: [window] }),
        resolveWindow: (request) => {
          expect(request.selector?.application).toEqual({ case: 'applicationBundleId', value: 'dev.auv.mock.music' })
          return { window }
        },
      })
    })
    const runner = auv.runner({ runnerClass: 'auv.core.local' })

    const [listed] = await runner.windows.list()
    expect(listed?.id).toBe('w-music')
    const resolved = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'dev.auv.mock.music' } })
    expect(resolved.window.title).toBe('Music')
    await connection.close()
  })

  it('answers unregistered methods with UNIMPLEMENTED, and passes handler errors through', async () => {
    const { auv, connection } = await mockRunner(({ service }) => {
      service(WindowService, {
        resolveWindow: () => {
          throw new AuvRpcError(5, 'no window matches')
        },
      })
    })
    const runner = auv.runner({ runnerClass: 'auv.core.local' })

    await expect(runner.windows.list()).rejects.toMatchObject({ rpcCode: 12 })
    await expect(runner.windows.resolve({})).rejects.toMatchObject({ message: 'no window matches', rpcCode: 5 })
    await connection.close()
  })

  it('streams both ways for a bidirectional method', async () => {
    const { auv, connection } = await mockRunner(({ service }) => {
      service(WindowService, { listWindows: () => ({ windows: [window] }) })
      service(InputService, {
        async* scrollUntil(requests) {
          const iterator = requests[Symbol.asyncIterator]()
          const begin = (await iterator.next()).value
          expect(begin?.event.case).toBe('begin')
          for (let steps = 1; steps <= 3; steps++) {
            yield { event: { case: 'update', value: { awaitingDecision: true, steps } } }
            const decision = (await iterator.next()).value
            if (decision?.event.case === 'decision' && decision.event.value.stop) {
              yield { event: { case: 'completed', value: { reason: ScrollUntilStopReason.PREDICATE_SATISFIED, steps } } }
              return
            }
          }
          yield { event: { case: 'completed', value: { reason: ScrollUntilStopReason.BUDGET_EXHAUSTED, steps: 3 } } }
        },
      })
    })
    const [music] = await auv.runner({ runnerClass: 'auv.core.local' }).windows.list()

    const completed = await music!.scrollUntil({ x: 10, y: 10 }, { maxSteps: 3, step: { case: 'instant', value: { deltaY: 100 } } }, {
      until: update => update.steps === 2,
    })
    expect([completed.reason, completed.steps]).toEqual([ScrollUntilStopReason.PREDICATE_SATISFIED, 2])
    await connection.close()
  })

  it('serves reflection, so discovery reads the registered methods and their presentation', async () => {
    const transport = createMockTransport(({ service }) => {
      service(WindowService, { listWindows: () => ({ windows: [window] }) })
    })
    const connection = await connectTransport(transport)

    const discovered = await discoverRunner(connection, { runnerClass: 'auv.core.local' })
    const list = discovered.apis.find(api => api.method === 'ListWindows')
    expect(list?.presentation?.name).toBe('windows.list')
    expect(discovered.describeMethod('/auv.api.driver.v1.WindowService/ListWindows')?.effect).toBe('read_only')
    await connection.close()
  })

  it('reports one local Device and keeps Run lifecycles', async () => {
    const { auv, connection } = await mockRunner((router) => {
      serveMockDaemon(router, { id: 'mock-device', name: 'Mock desktop' })
    })

    const devices = await auv.devices.list()
    expect(devices.map(device => [device.id, device.name, device.local])).toEqual([['mock-device', 'Mock desktop', true]])
    const run = await auv.runs.create({ deviceIds: ['mock-device'] })
    expect(run.id).toHaveLength(32)
    const stopped = await auv.runs.stop({ outcome: 'succeeded', runId: run.id })
    expect(stopped.phase).toBe('succeeded')
    await connection.close()
  })
})
