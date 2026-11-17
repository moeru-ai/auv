// @vitest-environment jsdom

import type { AnyMessage } from '@agentclientprotocol/sdk'
import type { CodemodeResult } from '@exterioragentprotocol/client-sdk/codemode'

import { AcpThreadController, SdkAcpClientAdapter } from '@hafbit/react-acp/core'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js'
import { beforeEach, describe, expect, it, onTestFinished, vi } from 'vitest'

import { codeModeConnection, CodeModeHost } from './codemode-bridge'

const execution = vi.hoisted(() => ({ runAgent: vi.fn() }))
const success: CodemodeResult = { calls: [], ok: true, output: [], storeWrites: { delete: [], set: {} }, value: undefined }
const canceled: CodemodeResult = { calls: [], error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: [] }
vi.mock('../../runtime/session', () => ({ session: execution }))

/** Exercise the same MCP interface used by agents, with an in-memory transport. */
async function connectTools(host: CodeModeHost): Promise<Client> {
  const [agent, page] = InMemoryTransport.createLinkedPair()
  const client = new Client({ name: 'agent', version: '1' })
  onTestFinished(() => {
    host.close()
    return client.close()
  })
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
    execution.runAgent.mockReset()
  })

  it('serves standard MCP before ACP session creation and binds execution to the active session', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    const { tools } = await client.listTools()
    expect(tools.map(tool => tool.name)).toEqual(['execute'])
    expect((await client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })).isError).toBe(true)
    host.setSession('one')
    execution.runAgent.mockResolvedValueOnce({ ...success, output: [{ text: 'result', type: 'text' }] })
    expect(await client.callTool({ arguments: { code: 'text(1)' }, name: 'execute' })).toMatchObject({ content: [{ text: 'result' }] })
    expect(execution.runAgent.mock.lastCall?.[0].timeoutMs).toBe(30000)
  })

  it('advertises native tools in execute discovery before a session starts', async () => {
    const client = await connectTools(new CodeModeHost())
    const { tools } = await client.listTools()
    expect(tools[0]?.description).toContain('JavaScript')
    expect(tools[0]?.description).toContain('find_window_text')
    expect(tools[0]?.description).toContain('ALL_TOOLS')
    expect(execution.runAgent).not.toHaveBeenCalled()
  })

  it('cancels the MCP invocation without affecting calls from another transport', async () => {
    const host = new CodeModeHost()
    host.setSession('one')
    const [client, other] = await Promise.all([connectTools(host), connectTools(host)])
    let finish!: (value: unknown) => void
    execution.runAgent.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const controller = new AbortController()
    const call = client.callTool({ arguments: { code: 'await operation()' }, name: 'execute' }, undefined, { signal: controller.signal })
    const rejection = expect(call).rejects.toThrow()
    await vi.waitFor(() => expect(execution.runAgent.mock.lastCall?.[0].code).toBe('await operation()'))
    const signal = execution.runAgent.mock.lastCall?.[0].signal as AbortSignal
    await other.close()
    expect(signal.aborted).toBe(false)
    controller.abort()
    await rejection
    await vi.waitFor(() => expect(signal.aborted).toBe(true))
    finish(canceled)
  })

  it('reports cancellation if a finishing execution returns success', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    let finish!: (value: CodemodeResult) => void
    execution.runAgent.mockImplementationOnce(() => new Promise<CodemodeResult>(resolve => void (finish = resolve)))
    const pending = client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })
    await vi.waitFor(() => expect(execution.runAgent).toHaveBeenCalledOnce())
    host.cancel()
    finish(success)
    expect(await pending).toMatchObject({
      content: [{ text: 'Script canceled.' }],
      isError: true,
    })
  })

  it('cancels on session replacement and rejects simultaneous calls', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    host.setSession('one')
    let finish!: (value: unknown) => void
    execution.runAgent.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const first = client.callTool({ arguments: { code: 'await operation()', timeoutMs: 1000 }, name: 'execute' })
    await vi.waitFor(() => expect(execution.runAgent.mock.lastCall?.[0].code).toBe('await operation()'))
    const signal = execution.runAgent.mock.lastCall?.[0].signal as AbortSignal
    expect(await client.callTool({ arguments: { code: 'text(1)' }, name: 'execute' })).toMatchObject({
      content: [{ text: 'Another playground execution is running.' }],
      isError: true,
    })
    expect(execution.runAgent).toHaveBeenCalledOnce()
    expect(signal.aborted).toBe(false)
    host.setSession('two')
    expect(signal.aborted).toBe(true)
    finish(canceled)
    expect((await first).isError).toBe(true)
    execution.runAgent.mockResolvedValueOnce(success)
    expect((await client.callTool({ arguments: { code: 'return 1' }, name: 'execute' })).isError).toBeFalsy()
  })

  it('passes the Relay HTTP endpoint through ACP and closes it with the connection', async () => {
    const host = new CodeModeHost()
    const client = await connectTools(host)
    const { controller, incoming, messages, tools } = createAgentFixture(host)
    await controller.connect()
    expect(controller.getState().capabilities?.mcpCapabilities?.http).toBe(true)
    await controller.createSession()
    expect(messages.find(message => 'method' in message && message.method === 'session/new')).toMatchObject({ params: { mcpServers: [{ headers: tools.headers, name: 'auv-playground', type: 'http', url: tools.url }] } })
    execution.runAgent.mockResolvedValueOnce(success)
    expect((await client.callTool({ arguments: { code: 'return 1', timeoutMs: 1000 }, name: 'execute' })).isError).toBeFalsy()
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
    execution.runAgent.mockImplementationOnce(() => new Promise((resolve) => {
      finish = resolve
    }))
    const pending = client.callTool({ arguments: { code: 'await operation()', timeoutMs: 1000 }, name: 'execute' })
    await vi.waitFor(() => expect(execution.runAgent.mock.lastCall?.[0].code).toBe('await operation()'))
    const signal = execution.runAgent.mock.lastCall?.[0].signal as AbortSignal
    await controller.cancel(sessionId)
    expect(signal.aborted).toBe(true)
    expect(messages).toContainEqual(expect.objectContaining({ method: 'session/cancel', params: { sessionId } }))
    finish(canceled)
    expect((await pending).isError).toBe(true)
    controller.dispose()
    expect(tools.close).toHaveBeenCalledOnce()
  })
})
