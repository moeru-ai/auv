import type { RunRequest } from '../runtime/protocol'

import { describe, expect, it } from 'vitest'

import { compileCell } from '../stepper/compile'
import { runCodeMode } from './codemode'

function request(source: string, store: Record<string, unknown> = {}, timeoutMs = 1000): RunRequest {
  return { ...compileCell(source, { allowReturn: true, recordOnly: true }), breakpoints: [], codemode: { store, timeoutMs }, executionId: 1, mode: 'run' }
}

const hooks = { __bind: () => {}, __step: async () => {}, __stepSync: () => {} }

describe('isolated codemode', () => {
  // ROOT CAUSE: binding inspection copied the entire guest value before the
  // worker could summarize it. Large-array dumps triggered a QuickJS teardown
  // assertion. Bound inspection inside the guest before crossing that seam.
  it('inspects large local arrays without exhausting the VM or leaking handles', async () => {
    const bindings: unknown[] = []
    const outcome = await runCodeMode(request('const values = new Array(300000).fill("x"); text(values.length);', {}, 5000), {
      ...hooks,
      __bind: (_id: number, values: unknown) => { bindings.push(values) },
    }, new AbortController().signal)
    expect(outcome).toMatchObject({ codemode: { content: [{ text: '300000', type: 'text' }] }, status: 'ok' })
    expect(JSON.stringify(bindings).length).toBeLessThan(10000)
  })

  // A supplied host function isolates VM marshalling from SDK request encoding.
  it('preserves bigint arguments and results across host calls', async () => {
    const calls: unknown[] = []
    const outcome = await runCodeMode(request('const result = await auv.echo({ id: 9007199254740993n }); text(typeof result.id); text(result); return result;'), {
      ...hooks,
      auv: { echo: async (value: unknown) => {
        calls.push(value)
        return value
      } },
    }, new AbortController().signal)
    expect(outcome.status).toBe('ok')
    expect(calls).toEqual([{ id: 9007199254740993n }])
    expect(outcome.codemode?.content).toEqual([
      { text: 'bigint', type: 'text' },
      { text: '{"id":"9007199254740993"}', type: 'text' },
      { text: '{"id":"9007199254740993"}', type: 'text' },
    ])
  })

  // A generic async bridge turned the SDK's synchronous windows.from()
  // into a Promise, so the next window operation could not be called.
  it('preserves synchronous window binding while its device methods remain asynchronous', async () => {
    const clicked: unknown[] = []
    const outcome = await runCodeMode(request('const counter = auv.windows.from("counter"); await counter.click({ x: 10, y: 20 });'), {
      ...hooks,
      auv: { windows: { from: () => ({ click: async (target: unknown) => clicked.push(target) }) } },
    }, new AbortController().signal)
    expect(outcome).toMatchObject({ status: 'ok' })
    expect(clicked).toEqual([{ x: 10, y: 20 }])
  })

  it('keeps SDK error names and RPC codes available to the script', async () => {
    const error = Object.assign(new Error('missing window'), { name: 'AuvRpcError', rpcCode: 5 })
    const outcome = await runCodeMode(request('try { await auv.call(); } catch (error) { text({ name: error.name, rpcCode: error.rpcCode }); }'), {
      ...hooks,
      auv: { call: async () => { throw error } },
    }, new AbortController().signal)
    expect(outcome.status).toBe('ok')
    expect(outcome.codemode?.content).toEqual([{ text: '{"name":"AuvRpcError","rpcCode":5}', type: 'text' }])
  })

  it('keeps methods callable on objects returned by host functions', async () => {
    const inputs: unknown[] = []
    const outcome = await runCodeMode(request(`
      const win = await auv.windows.resolve({ appName: 'Counter' });
      const found = await win.findText('Increment');
      await win.click(found.matches[0]);
      text({ clicked: found.matches.length });
    `), { ...hooks, auv: { windows: { resolve: async () => ({ click: async (target: unknown) => inputs.push(target), findText: async () => ({ matches: [{ x: 10, y: 20 }] }) }) } } }, new AbortController().signal)
    expect(outcome).toMatchObject({ status: 'ok' })
    expect(inputs).toEqual([{ x: 10, y: 20 }])
    expect(outcome.codemode?.content).toEqual([{ text: '{"clicked":1}', type: 'text' }])
  })

  it('isolates browser globals and does not retain variables across calls', async () => {
    const first = await runCodeMode(request('const secret = 42; text([typeof fetch, typeof indexedDB, typeof postMessage, typeof process]);'), hooks, new AbortController().signal)
    expect(first.codemode?.content).toEqual([{ text: '["undefined","undefined","undefined","undefined"]', type: 'text' }])
    const second = await runCodeMode(request('text(typeof secret)'), hooks, new AbortController().signal)
    expect(second.codemode?.content?.[0]).toEqual({ text: 'undefined', type: 'text' })
  })

  it('returns candidate JSON state on success and leaves the input state unchanged', async () => {
    const initial = { windowId: 'initial' }
    const first = await runCodeMode(request('store("windowId", "counter"); store("temporary", true); store("temporary", undefined); return load("windowId");', initial), hooks, new AbortController().signal)
    expect(initial).toEqual({ windowId: 'initial' })
    expect(first.codemode?.store).toEqual({ windowId: 'counter' })
    expect(first.codemode?.content).toEqual([{ text: 'counter', type: 'text' }])
    const failed = await runCodeMode(request('store("windowId", "other"); text("before failure"); throw new Error("broken");', first.codemode!.store), hooks, new AbortController().signal)
    expect(failed.status).toBe('error')
    expect(failed.codemode?.store).toEqual({ windowId: 'counter' })
    expect(failed.codemode?.content).toEqual([{ text: 'before failure', type: 'text' }])
  })

  it('rejects oversized text and preserves earlier output', async () => {
    const outcome = await runCodeMode(request('text("small"); text("x".repeat(api.help().limits.maxTextCharacters));'), hooks, new AbortController().signal)
    expect(outcome.status).toBe('error')
    expect(outcome.codemode?.content).toEqual([{ text: 'small', type: 'text' }])
  })

  it('accepts the text limit advertised by help', async () => {
    const outcome = await runCodeMode(request('text("x".repeat(api.help().limits.maxTextCharacters));'), hooks, new AbortController().signal)
    expect(outcome.status).toBe('ok')
    expect(outcome.codemode?.content).toEqual([{ text: 'x'.repeat(16000), type: 'text' }])
  })

  it('accepts the advertised store limit and rejects one more character without replacing state', async () => {
    const outcome = await runCodeMode(request(`
      const size = api.help().limits.maxStoreCharacters - JSON.stringify({ value: "" }).length;
      store("value", "x".repeat(size));
      try { store("value", "x".repeat(size + 1)); } catch (error) { text(error.message); }
    `), hooks, new AbortController().signal)
    expect(outcome.status).toBe('ok')
    expect(JSON.stringify(outcome.codemode?.store)).toHaveLength(64000)
    expect(outcome.codemode?.content).toEqual([{ text: 'Codemode store exceeds 64000 characters', type: 'text' }])
  })

  it('interrupts synchronous loops at the deadline', async () => {
    const outcome = await runCodeMode(request('text("loop started"); while (true) {}', {}, 100), hooks, new AbortController().signal)
    expect(outcome).toMatchObject({
      codemode: { content: [{ text: 'loop started', type: 'text' }] },
      message: expect.stringContaining('interrupted'),
      status: 'error',
    })
  })

  it('cancels a pending asynchronous call', async () => {
    const abort = new AbortController()
    let entered!: () => void
    const waiting = new Promise<void>(resolve => void (entered = resolve))
    const pending = runCodeMode(request('text("waiting"); await auv.wait();'), { ...hooks, auv: { wait: () => {
      entered()
      return new Promise(() => {})
    } } }, abort.signal)
    await waiting
    abort.abort()
    const outcome = await pending
    expect(outcome).toMatchObject({
      codemode: { content: [{ text: 'waiting', type: 'text' }] },
      message: expect.stringContaining('canceled'),
      status: 'error',
    })
  })

  it('explicitly rejects unsupported SDK callback arguments', async () => {
    let called = false
    const outcome = await runCodeMode(request('await auv.call({ until: () => true });'), { ...hooks, auv: { call: async () => {
      called = true
    } } }, new AbortController().signal)
    expect(outcome.status).toBe('error')
    expect(called).toBe(false)
    expect(outcome.status === 'error' && outcome.message).toContain('Functions cannot cross the codemode VM boundary')
  })

  // ROOT CAUSE: text() and SDK arguments shared a function validator whose
  // callback-only error incorrectly blamed a local Array.map callback.
  it('allows local map callbacks and explains function-bearing output', async () => {
    const globals = { ...hooks, auv: { windows: { list: async () => [{ capture: async () => ({}), id: 'w-counter', window: { title: 'Counter' } }] } } }
    const mapped = await runCodeMode(request('const wins = await auv.windows.list(); text(wins.map(w => ({ id: w.id, window: w.window })));'), globals, new AbortController().signal)
    expect(mapped).toMatchObject({ codemode: { content: [{ text: '[{"id":"w-counter","window":{"title":"Counter"}}]', type: 'text' }] }, status: 'ok' })

    const raw = await runCodeMode(request('const wins = await auv.windows.list(); text(wins.map(w => ({ id: w.id, raw: w })));'), globals, new AbortController().signal)
    expect(raw).toMatchObject({ message: expect.stringContaining('Functions cannot cross the codemode VM boundary'), status: 'error' })
    expect(raw.status === 'error' && raw.message).toContain('window.id or window.window')
  })

  it('forwards a capture reference to the host and emits its image response', async () => {
    const images: unknown[] = []
    const outcome = await runCodeMode(request('await image({ captureId: "cap-1" });'), { ...hooks, image: async (capture: unknown) => {
      images.push(capture)
      return { data: 'aGVsbG8=', mimeType: 'image/png' }
    } }, new AbortController().signal)
    expect(images).toEqual([{ captureId: 'cap-1' }])
    expect(outcome.codemode?.content).toEqual([{ data: 'aGVsbG8=', mimeType: 'image/png', type: 'image' }])
  })
})
