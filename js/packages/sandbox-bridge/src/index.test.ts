import type { DuplexCall } from '@auv-js/sdk'

import { base64Encode } from '@bufbuild/protobuf/wire'
import { expect, it, onTestFinished, vi } from 'vitest'

import { createSandboxRunnerHost } from './index'

const method = '/auv.api.driver.v1.WindowService/ScrollWindowUntil'
const begin = Uint8Array.of(10, 1, 2)
const update = Uint8Array.of(18, 1, 3)
const stop = Uint8Array.of(26, 1, 1)

function fixture(open: (signal: AbortSignal | undefined) => DuplexCall) {
  const execution = new AbortController()
  const duplex = vi.fn(async ({ signal }: { signal?: AbortSignal }) => open(signal))
  const unary = vi.fn(async () => update)
  const finish = vi.fn()
  const record = vi.fn(async () => finish)
  const host = createSandboxRunnerHost({
    route: { deviceId: 'device', runId: 'run', runnerClass: 'auv.core.local' },
    target: { transport: { close() {}, async connect() {}, duplex, unary } },
  }, execution.signal, record)
  onTestFinished(host.close)
  const [binding] = host.globals
  const context = { signal: execution.signal }
  const execute = (request: unknown) => binding!.execute(request, context)
  return { duplex, execute, execution, finish, record, unary }
}

it('records a unary call and cancels a timer through the same binding', async () => {
  const { execute, execution, finish, unary } = fixture(() => {
    throw new Error('Unexpected stream.')
  })
  const unaryMethod = '/auv.api.driver.v1.WindowService/GetWindow'
  const result = await execute(['unary', unaryMethod, base64Encode(begin), '{}'])
  expect(result).toEqual({ body: base64Encode(update) })
  expect(unary).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ body: begin, method: unaryMethod }))
  expect(finish).toHaveBeenCalledExactlyOnceWith({
    exchange: [{ request: begin }, { response: update }],
  })

  const canceled = expect(execute(['wait', 60_000])).rejects.toMatchObject({ name: 'AbortError' })
  execution.abort()
  await canceled
})

it('delivers and records interleaved Runner frames as one call', async () => {
  const started = Promise.withResolvers<void>()
  const decided = Promise.withResolvers<void>()
  const close = vi.fn()
  const { duplex, execute, finish, record } = fixture(() => ({
    close,
    halfClose: async () => {},
    responses: (async function* () {
      await started.promise
      yield update
      await decided.promise
    })(),
    async send(body) {
      if (body[0] === begin[0]) {
        expect(body).toEqual(begin)
        started.resolve()
      }
      else {
        expect(body).toEqual(stop)
        decided.resolve()
      }
    },
  }))
  const stream = await execute(['open', method])
  const reading = execute(['next', stream])
  await execute(['send', stream, base64Encode(begin)])
  await expect(reading).resolves.toBe(base64Encode(update))
  await execute(['send', stream, base64Encode(stop)])
  await expect(execute(['next', stream])).resolves.toBeNull()
  await execute(['close', stream])

  expect(duplex).toHaveBeenCalledWith(expect.objectContaining({ method }))
  expect(record).toHaveBeenCalledExactlyOnceWith({ body: begin, method })
  expect(finish).toHaveBeenCalledExactlyOnceWith({
    exchange: [{ request: begin }, { response: update }, { request: stop }],
  })
  expect(close).toHaveBeenCalledTimes(1)
})

it('cancels a waiting Runner read and finishes its recording', async () => {
  const waiting = Promise.withResolvers<void>()
  const close = vi.fn()
  const { execute, execution, finish } = fixture(signal => ({
    close,
    halfClose: async () => {},
    responses: (async function* () {
      const aborted = Promise.withResolvers<never>()
      signal?.addEventListener('abort', () => aborted.reject(signal.reason), { once: true })
      waiting.resolve()
      yield await aborted.promise
    })(),
    send: async () => {},
  }))
  const stream = await execute(['open', method])
  await execute(['send', stream, base64Encode(begin)])
  const reading = execute(['next', stream])
  await waiting.promise
  const canceled = expect(reading).rejects.toMatchObject({ name: 'AbortError' })
  execution.abort()
  await canceled

  expect(finish).toHaveBeenCalledExactlyOnceWith({
    error: expect.objectContaining({ name: 'AbortError' }),
    exchange: [{ request: begin }],
  })
  expect(close).toHaveBeenCalledTimes(1)
})
