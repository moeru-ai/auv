// @vitest-environment jsdom

import type { AnyMessage } from '@agentclientprotocol/sdk'

import { AcpThreadController, SdkAcpClientAdapter } from '@hafbit/react-acp/core'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js'
import { beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest'

import { codeModeConnection, CodeModeHost } from './codemode-bridge'

const execution = vi.hoisted(() => ({ run: vi.fn(), running: false }))
vi.mock('../../runtime/session', () => ({ session: execution }))

/** Exercise the same MCP interface used by agents, with an in-memory transport. */
async function connectTools(host: CodeModeHost): Promise<Client> {
  const [agent, page] = InMemoryTransport.createLinkedPair()
  const client = new Client({ name: 'agent', version: '1' })
  onTestFinished(() => client.close())
  await host.connect(page)
  await client.connect(agent)
  return client
}

/** Real ACP adapter and controller over an in-memory agent connection. */
function createAgentFixture(host: CodeModeHost) {
  let incoming!: ReadableStreamDefaultController<AnyMessage>
  let sessions = 0
  const messages: AnyMessage[] = []
  const tools = { close: vi.fn(), closed: new Promise<undefined>(() => {}), headers: [{ name: 'Authorization', value: 'Bearer fixture' }], url: 'https://relay.example/eap/v1/mcp' }
  const raw = {
    readable: new ReadableStream<AnyMessage>({ start: controller => void (incoming = controller) }),
    writable: new WritableStream<AnyMessage>({
      write(message) {
        messages.push(message)
        if (!('method' in message) || !('id' in message))
          return
        let result: unknown = {}
        if (message.method === 'initialize')
          result = { agentCapabilities: { mcpCapabilities: { http: true } }, protocolVersion: 1 }
        else if (message.method === 'session/new')
          result = { sessionId: `session-${++sessions}` }
        else if (message.method === 'session/prompt')
          result = { stopReason: 'end_turn' }
        incoming.enqueue({ id: message.id, jsonrpc: '2.0', result })
      },
    }),
  }
  const adapter = new SdkAcpClientAdapter(() => raw)
  const controller = new AcpThreadController({
    connection: {
      adapter: { connect: async options => codeModeConnection(await adapter.connect(options), host, tools) },
      type: 'adapter',
    },
    onThreadIdChange: sessionId => host.setSession(sessionId),
    workspace: { cwd: '/' },
  })
  onTestFinished(() => controller.dispose())
  return { controller, incoming, messages, tools }
}

describe('playground execute MCP tool', () => {
  beforeEach(() => {
    execution.run.mockReset()
    execution.running = false
  })

  it('serves standard MCP before ACP session creation and binds execution to the active session', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    const { tools } = await client.listTools()
    expect(tools.map(tool => tool.name)).toEqual(['execute'])
    expect(tools[0].inputSchema.properties?.timeoutMs).toMatchObject({ default: 30000, maximum: 120000 })
    expect((await client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })).isError).toBe(true)
    host.setSession('one')
    execution.run.mockResolvedValueOnce({ codemode: { content: [{ text: 'result', type: 'text' }], store: {} }, status: 'ok' })
    expect(await client.callTool({ arguments: { code: 'text(1)' }, name: 'execute' })).toMatchObject({ content: [{ text: 'result' }, { text: expect.stringContaining('Script completed') }], isError: false })
    expect(execution.run.mock.lastCall?.[0].codemode.timeoutMs).toBe(30000)
  })

  it.each([
    { arguments: undefined, label: 'missing arguments' },
    { arguments: { code: ' \n ' }, label: 'blank code' },
    { arguments: { code: 1 }, label: 'non-string code' },
    { arguments: { code: 'x'.repeat(100001) }, label: 'oversized code' },
    { arguments: { code: 'return 1', timeoutMs: 0 }, label: 'zero timeout' },
    { arguments: { code: 'return 1', timeoutMs: 120001 }, label: 'oversized timeout' },
    { arguments: { code: 'return 1', timeoutMs: 1.5 }, label: 'fractional timeout' },
    { arguments: { code: 'return 1', timeoutMs: '1000' }, label: 'non-number timeout' },
  ])('rejects $label before execution', async ({ arguments: args }) => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    expect(await client.callTool({ arguments: args, name: 'execute' })).toMatchObject({
      content: [{ text: 'execute requires code and a timeoutMs from 1 to 120000.' }],
      isError: true,
    })
    expect(execution.run).not.toHaveBeenCalled()
  })

  it('cancels the MCP invocation without affecting calls from another transport', async () => {
    const host = new CodeModeHost()
    host.setSession('one')
    const [client, other] = await Promise.all([connectTools(host), connectTools(host)])
    let finish!: (value: unknown) => void
    execution.run.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const controller = new AbortController()
    const call = client.callTool({ arguments: { code: 'await operation()' }, name: 'execute' }, undefined, { signal: controller.signal })
    const rejection = expect(call).rejects.toThrow()
    await vi.waitFor(() => expect(execution.run.mock.lastCall?.[0].source).toBe('await operation()'))
    const signal = execution.run.mock.lastCall?.[0].signal as AbortSignal
    await other.close()
    expect(signal.aborted).toBe(false)
    controller.abort()
    await rejection
    await vi.waitFor(() => expect(signal.aborted).toBe(true))
    finish({ reason: 'canceled', status: 'stopped' })
  })

  it('keeps state in the active session and commits only successful executions', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    execution.run.mockResolvedValueOnce({ codemode: { content: [{ text: 'selected', type: 'text' }], store: { windowId: 'counter' } }, status: 'ok' })
    expect((await client.callTool({ arguments: { code: 'text(1)', timeoutMs: 1000 }, name: 'execute' })).isError).toBe(false)
    execution.run.mockResolvedValueOnce({ codemode: { content: [{ text: 'partial', type: 'text' }], store: { windowId: 'bad' } }, message: 'failed', status: 'error' })
    const failed = await client.callTool({ arguments: { code: 'throw 1', timeoutMs: 1000 }, name: 'execute' })
    expect(failed).toMatchObject({ content: [{ text: 'partial' }, { text: 'Script failed: failed' }], isError: true })
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: { windowId: 'counter' } }, status: 'ok' })
    await client.callTool({ arguments: { code: 'return load("windowId")' }, name: 'execute' })
    expect(execution.run.mock.lastCall?.[0].codemode.store).toEqual({ windowId: 'counter' })
    host.setSession(undefined)
    expect((await client.callTool({ arguments: { code: 'text(1)' }, name: 'execute' })).isError).toBe(true)
    host.setSession('two')
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: {} }, status: 'ok' })
    await client.callTool({ arguments: { code: 'text(1)', timeoutMs: 1000 }, name: 'execute' })
    expect(execution.run.mock.lastCall?.[0].codemode.store).toEqual({})
  })

  it('keeps committed state when a canceled execution returns an ok outcome', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: { windowId: 'counter' } }, status: 'ok' })
    await client.callTool({ arguments: { code: 'store("windowId", "counter")' }, name: 'execute' })
    let finish!: (value: unknown) => void
    execution.run.mockImplementationOnce(() => new Promise(resolve => void (finish = resolve)))
    const canceled = client.callTool({ arguments: { code: 'store("windowId", "other")' }, name: 'execute' })
    await vi.waitFor(() => expect(execution.run).toHaveBeenCalledTimes(2))
    host.cancel()
    finish({ codemode: { content: [], store: { windowId: 'other' } }, status: 'ok' })
    expect((await canceled).isError).toBe(true)
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: {} }, status: 'ok' })
    await client.callTool({ arguments: { code: 'return load("windowId")' }, name: 'execute' })
    expect(execution.run.mock.lastCall?.[0].codemode.store).toEqual({ windowId: 'counter' })
  })

  it('rejects an MCP call while the shared execution session is running', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    execution.running = true
    expect(await client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })).toMatchObject({
      content: [{ text: 'Another playground execution is running.' }],
      isError: true,
    })
    expect(execution.run).not.toHaveBeenCalled()
    execution.running = false
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: {} }, status: 'ok' })
    expect((await client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })).isError).toBe(false)
  })

  it('cancels on session replacement and rejects simultaneous calls', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    let finish!: (value: unknown) => void
    execution.run.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const first = client.callTool({ arguments: { code: 'await operation()', timeoutMs: 1000 }, name: 'execute' })
    await vi.waitFor(() => expect(execution.run.mock.lastCall?.[0].source).toBe('await operation()'))
    const signal = execution.run.mock.lastCall?.[0].signal as AbortSignal
    expect(await client.callTool({ arguments: { code: 'text(1)' }, name: 'execute' })).toMatchObject({
      content: [{ text: 'Another playground execution is running.' }],
      isError: true,
    })
    expect(execution.run).toHaveBeenCalledOnce()
    expect(signal.aborted).toBe(false)
    host.setSession('two')
    expect(signal.aborted).toBe(true)
    finish({ codemode: { content: [], store: { stale: true } }, status: 'ok' })
    expect((await first).isError).toBe(true)
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: {} }, status: 'ok' })
    await client.callTool({ arguments: { code: 'return load("stale")' }, name: 'execute' })
    expect(execution.run.mock.lastCall?.[0].codemode.store).toEqual({})
  })

  it('passes the Relay HTTP endpoint through ACP and closes it with the connection', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    const { controller, incoming, messages, tools } = createAgentFixture(host)
    await controller.connect()
    expect(controller.getState().capabilities?.mcpCapabilities?.http).toBe(true)
    await controller.createSession()
    expect(messages.find(message => 'method' in message && message.method === 'session/new')).toMatchObject({ params: { mcpServers: [{ headers: tools.headers, name: 'auv-playground', type: 'http', url: tools.url }] } })
    execution.run.mockResolvedValueOnce({ codemode: { content: [], store: {} }, status: 'ok' })
    expect((await client.callTool({ arguments: { code: 'return 1', timeoutMs: 1000 }, name: 'execute' })).isError).toBe(false)
    incoming.close()
    await vi.waitFor(() => expect(tools.close).toHaveBeenCalledOnce())
  })

  it('cancels execute with the ACP turn and closes MCP on disconnect', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    const { controller, messages, tools } = createAgentFixture(host)
    let finish!: (value: unknown) => void
    await controller.connect()
    const sessionId = await controller.createSession()
    execution.run.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const pending = client.callTool({ arguments: { code: 'await operation()', timeoutMs: 1000 }, name: 'execute' })
    await vi.waitFor(() => expect(execution.run.mock.lastCall?.[0].source).toBe('await operation()'))
    const signal = execution.run.mock.lastCall?.[0].signal as AbortSignal
    await controller.cancel(sessionId)
    expect(signal.aborted).toBe(true)
    expect(messages).toContainEqual(expect.objectContaining({ method: 'session/cancel', params: { sessionId } }))
    finish({ reason: 'canceled', status: 'stopped' })
    expect((await pending).isError).toBe(true)
    controller.dispose()
    expect(tools.close).toHaveBeenCalledOnce()
  })
})
