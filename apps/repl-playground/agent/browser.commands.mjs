import assert from 'node:assert/strict'
import process from 'node:process'

import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js'
import { WebSocketServer } from 'ws'

import { createMcpRelay } from './relay.fixture.mjs'

const fixtures = new Map()

/** Node-only wire peers. Test cases and workbench interactions run in Browser Mode. */
export const browserCommands = {
  /**
   * @param {import('vitest/node').BrowserCommandContext} context
   * @param {{ supported?: boolean, startupError?: boolean, holdTicket?: boolean, releaseTicket?: boolean, discoverTools?: boolean }} [options]
   * @returns {Promise<{connections: number, errors: string[], messages: Array<{method?: string, params: Record<string, unknown>}>, registrations: string[], ticketRequests: number, tools: Array<{name: string}>}>} Current peer state and requested tool discovery results.
   */
  async agentFixtureState(context, options = {}) {
    return fixtures.get(context.testPath).state(options)
  },

  /** Preserve the existing opt-in evidence paths without exposing Node APIs to tests. */
  async agentScreenshot(context, name) {
    const path = {
      agent: process.env.AUV_AGENT_SCREENSHOT,
      codemode: process.env.AUV_CODEMODE_SCREENSHOT,
      connected: process.env.AUV_AGENT_CONNECTED_SCREENSHOT,
    }[name]
    if (path)
      await context.iframe.locator('body').screenshot({ animations: 'disabled', path })
  },

  async startAgentFixtures(context, imageResult) {
    const page = context.page
    const relay = await createMcpRelay()
    const errors = []
    let releaseTicket
    const server = new WebSocketServer({ host: '127.0.0.1', port: 0 })
    await new Promise(resolve => server.once('listening', resolve))
    const messages = []
    let supported = true
    let connections = 0
    let ticketRequests = 0
    let startupError = false
    let pendingTicket
    const tickets = new Set()
    const clients = []
    server.on('connection', (socket, request) => {
      connections++
      const ticket = new URL(request.url, 'http://localhost').searchParams.get('ticket')
      if (!tickets.delete(ticket)) {
        socket.close(1008, 'Ticket expired or already consumed')
        return
      }
      socket.on('message', async (data) => {
        try {
          const message = JSON.parse(data.toString())
          messages.push(message)
          let result
          if (message.method === 'initialize') {
            if (startupError) {
              socket.send(JSON.stringify({ error: { code: -32603, message: 'Agent startup failed' }, id: message.id, jsonrpc: '2.0' }))
              return
            }
            result = { agentCapabilities: supported ? { mcpCapabilities: { http: true } } : {}, protocolVersion: 1 }
          }
          else if (message.method === 'session/new') {
            const declaration = message.params.mcpServers.find(server => server.type === 'http')
            assert.ok(declaration, 'An ordinary agent must receive the HTTP MCP declaration')
            const client = new Client({ name: 'agent', version: '1' })
            await client.connect(new StreamableHTTPClientTransport(new URL(declaration.url), { requestInit: { headers: Object.fromEntries(declaration.headers.map(header => [header.name, header.value])) } }))
            const { tools } = await client.listTools()
            assert.deepEqual(tools.map(tool => tool.name), ['execute'], 'Tool discovery must work before ACP session creation finishes')
            assert.ok(tools[0].description.includes('text(api.help())'), 'Tool discovery must include execution and SDK discovery instructions')
            clients.push(client)
            result = { sessionId: `ui-${message.id}` }
          }
          else if (message.method === 'session/prompt') {
            const notify = update => socket.send(JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: message.params.sessionId, update } }))
            // ROOT CAUSE: the chat rendered only the wrapper's completed status,
            // hiding its input and the failed nested tool result delivered by ACP.
            // OpenCode 2.0.26 sends nested statuses in rawOutput.metadata.toolCalls.
            notify({ content: { text: 'I will inspect the desktop and check the result.', type: 'text' }, sessionUpdate: 'agent_message_chunk' })
            notify({ content: [{ content: { text: 'ReferenceError: Unknown identifier \'text\'.', type: 'text' }, type: 'content' }], kind: 'other', rawInput: { code: 'text(api.help());' }, rawOutput: { metadata: { error: true } }, sessionUpdate: 'tool_call', status: 'completed', title: 'execute', toolCallId: 'code-failure' })
            notify({ kind: 'other', rawInput: { code: 'return await tools["auv-playground"].execute({ code: "await auv.overlay.show({});" });' }, sessionUpdate: 'tool_call', status: 'in_progress', title: 'execute', toolCallId: 'nested-failure' })
            notify({ content: [{ content: { text: 'Script failed: OverlayService/ShowOverlay is UNIMPLEMENTED', type: 'text' }, type: 'content' }], rawOutput: { metadata: { toolCalls: [{ status: 'error', tool: 'auv-playground.execute' }] } }, sessionUpdate: 'tool_call_update', status: 'completed', toolCallId: 'nested-failure' })
            notify({ content: [{ content: imageResult, type: 'content' }], kind: 'read', rawInput: { code: 'await image(capture);' }, sessionUpdate: 'tool_call', status: 'completed', title: 'Capture result', toolCallId: 'image-result' })
            notify({ content: [{ content: { text: '0 errors found.', type: 'text' }, type: 'content' }], kind: 'read', rawInput: { path: 'notes.md' }, sessionUpdate: 'tool_call', status: 'completed', title: 'Read notes', toolCallId: 'neutral-result' })
            socket.send(JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: {
              sessionId: message.params.sessionId,
              update: { content: { text: 'Connected through the test ACP peer.\n\n| Check | Operation | Expected | Actual | Evidence | Result |\n| --- | --- | --- | --- | --- | --- |\n| Counter | Click Increment twice and capture the window. | Count: 2 | Count: 2 | Screenshot | Passed |\n\n- [x] Verified\n\n~~Outdated~~\n\n> Checked with a screenshot.\n\n[Documentation](https://example.com/docs)\n\n```ts\ntext("verified");\n```', type: 'text' }, sessionUpdate: 'agent_message_chunk' },
            } }))
            result = { stopReason: 'end_turn' }
          }
          if (result) {
            socket.send(JSON.stringify({ id: message.id, jsonrpc: '2.0', result }))
          }
        }
        catch (error) {
          errors.push(String(error))
          socket.close(1011, 'Synthetic agent failed')
        }
      })
    })

    const cwd = 'C:/Users/test/agent-workspace'
    await page.route(`${relay.url}/eap/v1/devices`, route => route.fulfill({ json: { devices: [
      { agents: [{ agentType: 'custom', name: 'auv' }], hostname: 'agent-device', id: 'device', platform: 'windows', workingDirectory: cwd },
    ] } }))
    await page.route(`${relay.url}/eap/v1/connections`, async (route) => {
      if (route.request().method() === 'OPTIONS')
        return route.fulfill({ status: 204 })
      assert.deepEqual(route.request().postDataJSON(), { agentName: 'auv', deviceId: 'device' })
      assert.equal(route.request().headers().authorization, 'Bearer connection-token')
      const ticket = String(++ticketRequests)
      tickets.add(ticket)
      if (pendingTicket) {
        const pending = pendingTicket
        pendingTicket = undefined
        await pending
      }
      return route.fulfill({ json: { agentName: 'auv', agentType: 'custom', url: `ws://127.0.0.1:${server.address().port}?ticket=${ticket}` } })
    })

    fixtures.set(context.testPath, {
      async close() {
        releaseTicket?.()
        for (const client of clients) await client.close()
        for (const socket of server.clients) socket.terminate()
        await new Promise(resolve => server.close(resolve))
        await page.unroute(`${relay.url}/eap/v1/devices`)
        await page.unroute(`${relay.url}/eap/v1/connections`)
        await relay.close()
      },
      async state(options) {
        if (options.supported !== undefined)
          supported = options.supported
        if (options.startupError !== undefined)
          startupError = options.startupError
        if (options.holdTicket) {
          pendingTicket = new Promise((resolve) => {
            releaseTicket = resolve
          })
        }
        if (options.releaseTicket)
          releaseTicket?.()
        const tools = options.discoverTools ? (await clients[0].listTools()).tools : []
        return { connections, errors, messages, registrations: [...relay.registrations], ticketRequests, tools }
      },
    })
    return { benchmark: Boolean(process.env.AUV_CODEMODE_BENCHMARK), cwd, relayUrl: relay.url }
  },

  async stopAgentFixtures(context) {
    const fixture = fixtures.get(context.testPath)
    fixtures.delete(context.testPath)
    await fixture?.close()
  },
}
