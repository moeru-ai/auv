import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js'

import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js'
import { expect, it, vi } from 'vitest'
import { commands, page } from 'vitest/browser'

import { CodeModeHost } from '../src/features/agent/codemode-bridge'
import { serveMcp } from '../src/features/agent/exterior'
import { restoreDevices } from '../src/features/device/devices'
import { activateBackend, session } from '../src/runtime/session'
import { usePlayground } from '../src/store'
import { cursorSeq, latestFramesAt } from '../src/timeline'
import { mountPlayground } from './browser.fixture'

const textItems = (result: CallToolResult) => result.content.filter(item => item.type === 'text').map(item => item.text)

// https://github.com/moeru-ai/auv/pull/414#discussion_r4399104804
it('executes HTTP MCP calls in real workers and preserves history, cancellation and editor state', async () => {
  const fixture = await mountPlayground()
  const client = new Client({ name: 'browser-check', version: '1' })
  const host = new CodeModeHost()
  let endpoint: Awaited<ReturnType<typeof serveMcp>> | undefined
  const execute = (code: string, timeoutMs?: number) => client.callTool({ arguments: { code, timeoutMs }, name: 'execute' }) as Promise<CallToolResult>
  const history = () => {
    const state = usePlayground.getState()
    return { calls: state.calls.map(call => call.id), frames: latestFramesAt(state, cursorSeq(state)).map(frame => frame.handle.$ref), seq: cursorSeq(state) }
  }
  try {
    const agentTab = page.getByRole('tab', { exact: true, name: 'Agent' })
    const scriptTab = page.getByRole('tab', { exact: true, name: 'Script' })
    await expect.element(page.getByRole('button', { name: 'Sign in with Exterior' })).toBeVisible()
    const agentPanel = page.getByRole('tabpanel', { name: 'Agent' }).element()
    const editor = document.querySelector('.cm-editor')!
    await scriptTab.click()
    await expect.element(scriptTab).toHaveAttribute('aria-selected', 'true')
    await expect.element(page.elementLocator(editor)).toBeVisible()
    await expect.element(page.elementLocator(agentPanel)).not.toBeVisible()
    await agentTab.click()
    await expect.element(agentTab).toHaveAttribute('aria-selected', 'true')
    await expect.element(page.elementLocator(agentPanel)).toBeVisible()
    await expect.element(page.elementLocator(editor)).not.toBeVisible()

    await restoreDevices()
    usePlayground.setState({ live: false })
    expect((await session.run({ mode: 'run', source: 'const editorValue = 99;', startLine: 1 })).status).toBe('ok')
    endpoint = await serveMcp(fixture.relayUrl, 'browser-check', transport => void host.connect(transport))
    host.setSession('browser-check')
    await client.connect(new StreamableHTTPClientTransport(new URL(endpoint.url), { requestInit: { headers: Object.fromEntries(endpoint.headers.map(header => [header.name, header.value])) } }))
    const backend = usePlayground.getState().backend
    await activateBackend(null)
    try {
      const pure = await execute('return 1 + 1;')
      expect(pure.isError).toBeFalsy()
      expect(textItems(pure)).toContain('2')
      const missing = await execute('return await tools.list_windows({});')
      expect(missing.isError).toBe(true)
      expect(textItems(missing).at(-1)).toMatch(/need a Device/)
    }
    finally { await activateBackend(backend) }

    const { tools } = await client.listTools()
    expect(tools.map(tool => tool.name)).toEqual(['execute'])
    expect(tools[0]?.description).toContain('find_window_text')
    const discovery = await execute('return ALL_TOOLS.map(tool => tool.name);')
    expect(discovery.isError).toBeFalsy()
    expect(JSON.parse(textItems(discovery)[0]!)).toContain('find_window_text')
    expect(usePlayground.getState().calls).toHaveLength(0)
    const result = await execute(`
      const counter = await tools.resolve_window({ appName: 'Counter' });
      const found = await tools.find_window_text({ windowId: counter.windowId, query: 'Increment' });
      if (found.matches.length !== 1) throw new Error('Expected one button');
      const { x, y, width, height } = found.matches[0].bounds;
      for (let i = 0; i < 3; i++) {
        await tools.click_window({ windowId: counter.windowId, x: x + width / 2, y: y + height / 2, space: "screen" });
      }
      const after = await tools.find_window_text({ windowId: counter.windowId, query: 'Count:' });
      text(after.matches.map(item => item.text));
      store('counterId', counter.windowId);
      const shot = await tools.capture_window({ windowId: counter.windowId });
      image(await tools.get_capture_image({ captureId: shot.captureId }));
    `)
    expect(result.isError).toBeFalsy()
    expect(result.content).toEqual(expect.arrayContaining([expect.objectContaining({ type: 'image' })]))
    expect(textItems(result)).toContain('["Count: 3"]')
    const calls = usePlayground.getState().calls
    const counterSource = calls.at(-1)!.script
    expect(calls.filter(call => call.effect === 'input')).toHaveLength(3)
    expect(calls.every(call => call.status === 'ok')).toBe(true)
    await scriptTab.click()
    await expect.poll(() => document.querySelector('.cm-content')?.textContent).toContain('Count')
    await commands.agentScreenshot('codemode')
    await page.viewport(1024, 768)
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(window.innerWidth)
    await agentTab.click()
    expect(agentPanel.isConnected).toBe(true)
    expect(editor.isConnected).toBe(true)
    await commands.agentScreenshot('agent')

    // ROOT CAUSE: each codemode call started a new timeline. Even a help or
    // store query erased SDK calls and hid the previous desktop capture.
    const previous = history()
    const saved = await execute('text(load("counterId")); text(typeof counter); text(typeof editorValue);')
    expect(saved.isError).toBeFalsy()
    expect(textItems(saved).slice(0, 3)).toEqual(['w-counter', 'undefined', 'undefined'])
    const continued = history()
    expect(continued.calls).toEqual(previous.calls)
    expect(continued.frames).toEqual(previous.frames)
    expect(continued.seq).toBeGreaterThan(previous.seq)
    const previousRunId = usePlayground.getState().run.runId
    const runIds: Array<null | string> = []
    const access = backend!.sdk.bind(backend)
    const observed = vi.spyOn(backend!, 'sdk').mockImplementation(() => {
      const sdk = access()
      return {
        ...sdk,
        target: {
          ...sdk.target,
          transport: {
            ...sdk.target.transport,
            unary: async (call) => {
              if (call.method.endsWith('/FindWindowText'))
                runIds.push(call.headers.get('auv-run-id'))
              return await sdk.target.transport.unary(call)
            },
          },
        },
      }
    })
    let repeated: CallToolResult
    try {
      repeated = await execute('const result = await tools.find_window_text({ windowId: load("counterId"), query: "Count:" }); return result.matches.map(item => item.text);')
    }
    finally { observed.mockRestore() }
    expect(repeated.isError).toBeFalsy()
    expect(textItems(repeated)).toContain('["Count: 3"]')
    const currentRunId = usePlayground.getState().run.runId
    expect(currentRunId).toMatch(/^[a-f\d]{32}$/)
    expect(currentRunId).not.toBe(previousRunId)
    expect(runIds).toEqual([currentRunId])
    const retainedCall = usePlayground.getState().calls.at(-1)!
    expect(retainedCall.script.id).not.toBe(counterSource.id)
    expect(retainedCall.script.source).toContain('find_window_text')
    expect(retainedCall.line).toBeNull()
    await page.getByRole('tab', { exact: true, name: 'Variables' }).click()
    const variables = page.getByRole('tabpanel', { exact: true, name: 'Variables' })
    await expect.element(variables).toHaveTextContent(/Declared names show up here/)
    await expect.element(variables).not.toHaveTextContent(/editorValue/)
    await page.getByRole('tab', { name: /^Calls / }).click()
    const failed = await execute('store("counterId", "bad"); text("before"); throw new Error("intentional");')
    expect(failed.isError).toBe(true)
    expect(textItems(failed)).toContain('before')
    expect(textItems(await execute('return load("counterId");'))).toContain('w-counter')
    const jpeg = await execute(`
      const shot = await tools.capture_window({ windowId: load('counterId') });
      image(await tools.get_capture_image({ captureId: shot.captureId, encoding: 'jpeg', maxSize: { height: 256, width: 256 } }));
    `)
    expect(jpeg.isError).toBeFalsy()
    expect(jpeg.content).toEqual(expect.arrayContaining([expect.objectContaining({ mimeType: 'image/jpeg', type: 'image' })]))
    const appended = usePlayground.getState().calls
    expect(appended.slice(0, previous.calls.length).map(call => call.id)).toEqual(previous.calls)
    expect(retainedCall.seq).toBeGreaterThan(continued.seq)
    expect(appended.at(-1)!.script.id).not.toBe(counterSource.id)
    expect(appended[previous.calls.length - 1].script.source).toMatch(/for \(let i = 0; i < 3;/)
    await page.elementLocator(document.querySelector(`[data-call-id="${previous.calls.at(-1)}"] td`)!).click()
    const source = [...document.querySelectorAll('summary')].find(element => element.textContent?.includes('source'))!
    await page.elementLocator(source).click()
    await expect.poll(() => document.querySelector('details pre')?.textContent).toMatch(/for \(let i = 0; i < 3;/)

    // Delay a real tool call so cancellation reaches the worker and the
    // transport while the desktop is still locked.
    const originalSdk = backend!.sdk.bind(backend)
    const delayed = vi.spyOn(backend!, 'sdk').mockImplementation(() => {
      const sdk = originalSdk()
      return {
        ...sdk,
        target: {
          ...sdk.target,
          transport: {
            ...sdk.target.transport,
            async unary(call) {
              if (call.method.endsWith('/ListWindows')) {
                await new Promise<void>((_resolve, reject) => {
                  const abort = () => reject(call.signal?.reason)
                  if (call.signal?.aborted)
                    abort()
                  else
                    call.signal?.addEventListener('abort', abort, { once: true })
                })
              }
              return await sdk.target.transport.unary(call)
            },
          },
        },
      }
    })
    try {
      const pending = execute('await tools.list_windows({});', 10000)
      await vi.waitUntil(() => usePlayground.getState().calls.some(call => call.status === 'pending'))
      expect(await session.run({ mode: 'run', source: '1 + 1;', startLine: 1 })).toMatchObject({ status: 'error' })
      host.cancel()
      expect((await pending).isError).toBe(true)
      expect(usePlayground.getState().run.status).toBe('stopped')
      const canceledCall = usePlayground.getState().calls.at(-1)
      expect(canceledCall?.status).toBe('error')

      const transportAbort = new AbortController()
      const transportCall = client.callTool({ arguments: { code: 'await tools.list_windows({});', timeoutMs: 10000 }, name: 'execute' }, undefined, { signal: transportAbort.signal })
      const rejected = expect(transportCall).rejects.toBeDefined()
      await vi.waitUntil(() => usePlayground.getState().calls.some(call => call.status === 'pending'))
      transportAbort.abort()
      await rejected
      await vi.waitUntil(() => !session.running)
      expect(usePlayground.getState().run.status).toBe('stopped')
    }
    finally { delayed.mockRestore() }
    const loop = await execute('while (true) {}', 300)
    expect(loop.isError).toBe(true)
    expect(textItems(loop).at(-1)).toMatch(/timed out|deadline/i)
    const recovered = await execute('return 7;')
    expect(recovered.isError).toBeFalsy()
    expect(textItems(recovered)).toContain('7')

    expect(textItems(await execute('return load("counterId");'))).toContain('w-counter')
    host.setSession('replacement')
    expect(textItems(await execute('return load("counterId") ?? "cleared";'))).toContain('cleared')
    await execute('store("device", "mock");')
    await activateBackend(null)
    await activateBackend(backend)
    expect(textItems(await execute('return load("device") ?? "cleared";'))).toContain('cleared')
    const malformed = await execute('await tools.click_window({ windowId: "w-counter", x: "bad", y: 5 });')
    expect(malformed.isError).toBe(true)
    const empty = await execute(`
      for (const query of ['', ' '])
        text((await tools.find_window_text({ windowId: 'w-counter', query })).matches.length);
    `)
    expect(textItems(empty)).toEqual(['0', '0'])
    expect(empty.isError).toBeFalsy()
    const editorRun = await session.run({ mode: 'run', source: 'editorValue;', startLine: 1 })
    expect(editorRun.status).toBe('ok')
    expect(editorRun.status === 'ok' && editorRun.value).toBe(99)
    const editorStep = session.run({ mode: 'step', source: 'editorValue + 1;', startLine: 1 })
    await vi.waitUntil(() => usePlayground.getState().run.status === 'paused')
    session.resume('continue')
    const stepped = await editorStep
    expect(stepped.status).toBe('ok')
    expect(stepped.status === 'ok' && stepped.value).toBe(100)

    const resetting = session.run({ mode: 'step', source: 'editorValue + 2;', startLine: 1 })
    await vi.waitUntil(() => usePlayground.getState().run.status === 'paused')
    session.reset()
    expect(await resetting).toEqual({ status: 'stopped' })
    expect(usePlayground.getState().run.status).toBe('idle')
    expect(await session.run({ mode: 'run', source: 'typeof editorValue;', startLine: 1 })).toMatchObject({ status: 'ok', value: 'undefined' })
  }
  finally {
    host.close()
    await client.close()
    endpoint?.close()
  }
})
