import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js'

import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js'
import { expect, it, vi } from 'vitest'
import { commands, page } from 'vitest/browser'

import { CodeModeHost } from '../src/features/agent/codemode-bridge'
import { serveMcp } from '../src/features/agent/exterior'
import { restoreDevices } from '../src/features/device/devices'
import { session } from '../src/runtime/session'
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
    expect((await session.run({ mode: 'run', source: 'const editorValue = 99;', startLine: 1 })).status).toBe('ok')
    endpoint = await serveMcp(fixture.relayUrl, 'browser-check', transport => void host.connect(transport))
    host.setSession('browser-check')
    await client.connect(new StreamableHTTPClientTransport(new URL(endpoint.url), { requestInit: { headers: Object.fromEntries(endpoint.headers.map(header => [header.name, header.value])) } }))
    const backend = usePlayground.getState().backend
    usePlayground.setState({ backend: null })
    try {
      const pure = await execute('text(api.help().limits.memoryMiB); return 2;')
      expect(pure.isError).toBe(false)
      expect(textItems(pure).slice(0, 2)).toEqual(['64', '2'])
      const missing = await execute('await auv.windows.list();')
      expect(missing.isError).toBe(true)
      expect(textItems(missing).at(-1)).toMatch(/needs a device/)
    }
    finally { usePlayground.setState({ backend }) }

    expect((await client.listTools()).tools[0].description).toMatch(/api\.help/)
    const discovery = await execute('text(api.help()); text(api.describe({ path: api.search({ query: "WindowClient.findText" }).items[0].path }));')
    expect(discovery.isError).toBe(false)
    const help = JSON.parse(textItems(discovery)[1])
    expect(help.signature).toMatch(/query: string/)
    expect(help.types.FindWindowTextResponse.definition).toMatch(/matches/)
    expect(usePlayground.getState().calls).toHaveLength(0)
    const display = await execute('const selector = { selector: { case: "display", value: { displayId: "2" } } }; text((await auv.displays.capture(selector)).display.displayId); text((await auv.displays.findText(selector, "Counter")).matches.length);')
    expect(display.isError).toBe(false)
    expect(textItems(display)).toEqual(expect.arrayContaining(['2', '1']))
    // SDK examples do not call VM globals; the test adds the output that an agent would write.
    const example = await execute(`${help.examples[0]}\ntext(matches)`)
    expect(example.isError).toBe(false)
    expect(textItems(example).join('\n')).toContain('Increment')
    const result = await execute(`
      const counter = await auv.windows.resolve({ appName: 'Counter' });
      const found = await counter.findText('Increment');
      if (found.matches.length !== 1) throw new Error('Expected one button');
      for (let i = 0; i < 3; i++) await counter.click(found.matches[0]);
      const after = await counter.findText('3');
      text(after.matches.map(item => item.text));
      store('counterId', counter.id);
      const shot = await counter.capture();
      await image(shot.capture);
    `)
    expect(result.isError).toBe(false)
    expect(result.content).toEqual(expect.arrayContaining([expect.objectContaining({ type: 'image' })]))
    expect(textItems(result)).toContain('["Count: 3"]')
    const calls = usePlayground.getState().calls
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
    expect(saved.isError).toBe(false)
    expect(textItems(saved).filter(text => text === 'undefined')).toHaveLength(2)
    const continued = history()
    expect(continued.calls).toEqual(previous.calls)
    expect(continued.frames).toEqual(previous.frames)
    expect(continued.seq).toBeGreaterThan(previous.seq)
    await page.getByRole('tab', { exact: true, name: 'Variables' }).click()
    await expect.element(page.getByRole('tabpanel', { exact: true, name: 'Variables' })).toHaveTextContent(/Declared names show up here/)
    await page.getByRole('tab', { name: /^Calls / }).click()
    const failed = await execute('store("counterId", "bad"); text("before"); throw new Error("intentional");')
    expect(failed.isError).toBe(true)
    expect(textItems(failed)).toContain('before')
    expect(textItems(await execute('return load("counterId");'))).toContain('w-counter')
    const jpeg = await execute(`
      const counter = await auv.windows.get(load('counterId'));
      await image((await counter.capture()).capture, { encoding: sdk.ImageEncoding.JPEG, maxSize: { height: 256, width: 256 } });
    `)
    expect(jpeg.isError).toBe(false)
    expect(jpeg.content).toEqual(expect.arrayContaining([expect.objectContaining({ mimeType: 'image/jpeg', type: 'image' })]))
    const appended = usePlayground.getState().calls
    expect(appended.slice(0, previous.calls.length).map(call => call.id)).toEqual(previous.calls)
    expect(appended[previous.calls.length].seq).toBeGreaterThan(continued.seq)
    expect(appended.at(-1)!.script.id).not.toBe(appended[previous.calls.length - 1].script.id)
    expect(appended[previous.calls.length - 1].script.source).toMatch(/for \(let i = 0; i < 3;/)
    await page.elementLocator(document.querySelector(`[data-call-id="${previous.calls.at(-1)}"] td`)!).click()
    const source = [...document.querySelectorAll('summary')].find(element => element.textContent?.includes('source'))!
    await page.elementLocator(source).click()
    await expect.poll(() => document.querySelector('details pre')?.textContent).toMatch(/for \(let i = 0; i < 3;/)

    const canceled = execute('text("waiting"); await new Promise(() => {});', 10000)
    await vi.waitUntil(() => usePlayground.getState().logs.some(log => log.values.includes('waiting')))
    host.cancel()
    const canceledResult = await canceled
    expect(canceledResult.isError).toBe(true)
    expect(textItems(canceledResult).at(-1)).toMatch(/canceled/)
    expect(textItems(canceledResult)).toContain('waiting')
    const reset = execute('text("reset waiting"); await new Promise(() => {});', 10000)
    await vi.waitUntil(() => usePlayground.getState().logs.some(log => log.values.includes('reset waiting')))
    session.reset()
    expect((await reset).isError).toBe(true)
    const transportAbort = new AbortController()
    const transportCanceled = client.callTool({ arguments: { code: 'text("transport waiting"); await new Promise(() => {});', timeoutMs: 10000 }, name: 'execute' }, undefined, { signal: transportAbort.signal })
    const rejected = expect(transportCanceled).rejects.toBeDefined()
    await vi.waitUntil(() => usePlayground.getState().logs.some(log => log.values.includes('transport waiting')))
    transportAbort.abort()
    await rejected
    await vi.waitUntil(() => usePlayground.getState().run.status === 'stopped')
    const loop = await execute('while (true) {}', 300)
    expect(loop.isError).toBe(true)
    expect(textItems(loop).at(-1)).toMatch(/timed out|interrupted/)
    const recovered = await execute('return 7;')
    expect(recovered.isError).toBe(false)
    expect(textItems(recovered)).toContain('7')
    const allocation = await execute('const values = new Array(1000000).fill("x"); text(values.length);')
    expect(allocation.isError).toBe(false)
    expect(textItems(allocation)).toContain('1000000')
    // NOTICE(codemode-trace-granularity): the owner retained complete statement
    // history. This optional probe reproduces its CPU-loop cost; it is not a
    // performance guarantee of the normal browser check.
    if (fixture.benchmark) {
      const busy = await execute('let sum=0; for(let i=0;i<1000000;i++){ sum+=i; } text(sum);', 10000)
      expect(busy.isError).toBe(false)
      expect(textItems(busy)).toContain('499999500000')
    }
    const malformed = await execute('await auv.displays.captureRegion({ x: 0, y: 0, width: -10, height: -10 });')
    expect(malformed.isError).toBe(true)
    expect(textItems(malformed).at(-1)).toMatch(/positive/)
    const empty = await execute('const win = await auv.windows.get("w-counter"); text((await win.findText("")).matches.length); text((await win.findText(" ")).matches.length);')
    expect(empty.isError).toBe(false)
    expect(textItems(empty).filter(text => text === '0')).toHaveLength(2)
    const editorRun = await session.run({ mode: 'run', source: 'editorValue;', startLine: 1 })
    expect(editorRun.status).toBe('ok')
    expect(editorRun.status === 'ok' && editorRun.value).toBe(99)
    const editorStep = session.run({ mode: 'step', source: 'editorValue + 1;', startLine: 1 })
    await vi.waitUntil(() => usePlayground.getState().run.status === 'paused')
    session.resume('continue')
    const stepped = await editorStep
    expect(stepped.status).toBe('ok')
    expect(stepped.status === 'ok' && stepped.value).toBe(100)
  }
  finally {
    host.cancel()
    await client.close()
    endpoint?.close()
  }
})
