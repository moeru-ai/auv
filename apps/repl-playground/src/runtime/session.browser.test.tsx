import type { CodemodeResult } from '@exterioragentprotocol/client-sdk/codemode'

import { afterEach, expect, it } from 'vitest'

import { usePlayground } from '../store'
import { session } from './session'

afterEach(() => session.reset())

it('resets the REPL and drops late timeline output from an active Agent run', async () => {
  expect(await session.run({ mode: 'run', source: 'const editorValue = 99; editorValue;', startLine: 1 })).toMatchObject({ status: 'ok', value: 99 })
  const started = Promise.withResolvers<AbortSignal>()
  const finished = Promise.withResolvers<CodemodeResult>()
  const running = session.runAgent({ code: 'text("partial")', signal: new AbortController().signal, timeoutMs: 10000 }, (_runner, signal) => {
    started.resolve(signal)
    return finished.promise
  })
  const signal = await started.promise

  session.reset()
  const resetState = usePlayground.getState()
  const stillRunning = session.running
  finished.resolve({ calls: [], error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: [{ text: 'partial', type: 'text' }] })

  expect(await running).toMatchObject({ error: { kind: 'aborted' }, output: [{ text: 'partial', type: 'text' }] })
  expect(signal.aborted).toBe(true)
  expect(stillRunning).toBe(true)
  expect(resetState).toMatchObject({ logs: [], run: { status: 'idle' } })
  expect(usePlayground.getState()).toMatchObject({ logs: [], run: { status: 'idle' } })
  expect(await session.run({ mode: 'run', source: 'typeof editorValue;', startLine: 1 })).toMatchObject({ status: 'ok', value: 'undefined' })
})

it('keeps the Agent cancellation signal clear after successful cleanup', async () => {
  let cancellation: AbortSignal | undefined
  const result = await session.runAgent({ code: 'return 7', signal: new AbortController().signal, timeoutMs: 10000 }, async (_runner, signal) => {
    cancellation = signal
    return { calls: [], ok: true, output: [], storeWrites: { delete: [], set: {} }, value: 7 }
  })

  expect(result).toMatchObject({ ok: true, value: 7 })
  expect(cancellation?.aborted).toBe(false)
  expect(usePlayground.getState().run.status).toBe('done')
})

it('reports an Agent stop and keeps partial output in the timeline', async () => {
  const output = { text: 'partial', type: 'text' } as const
  const result = await session.runAgent({ code: 'text("partial")', signal: new AbortController().signal, timeoutMs: 10000 }, async (_runner, signal) => {
    session.stop()
    expect(signal.aborted).toBe(true)
    return { calls: [], ok: true, output: [output], storeWrites: { delete: [], set: {} }, value: undefined }
  })

  expect(result).toMatchObject({ error: { kind: 'aborted' }, ok: false, output: [output] })
  expect(usePlayground.getState()).toMatchObject({ logs: [expect.objectContaining({ values: ['partial'] })], run: { status: 'stopped' } })
})
