import assert from 'node:assert/strict'

import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client'
import { SkillsClient } from '@modelcontextprotocol/client/ext/skills'
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

  async startAgentFixtures(context) {
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
            const declaration = message.params.mcpServers.find(server => server.name === 'auv-playground' && server.type === 'http')
            assert.ok(declaration, 'An ordinary agent must receive the HTTP MCP declaration')
            const client = new Client({ name: 'agent', version: '1' }, { versionNegotiation: { mode: 'auto' } })
            await client.connect(new StreamableHTTPClientTransport(new URL(declaration.url), { requestInit: { headers: Object.fromEntries(declaration.headers.map(header => [header.name, header.value])) } }))
            const { tools } = await client.listTools()
            assert.deepEqual(tools.map(tool => tool.name), ['execute'], 'Tool discovery must work before ACP session creation finishes')
            clients.push(client)
            assert.equal(client.getProtocolEra(), 'modern')
            const skills = new SkillsClient(client)
            assert.ok(skills.supported)
            const { skills: [entry] } = await skills.list()
            assert.equal(entry.uri, 'skill://auv-playground/SKILL.md')
            const instructions = await skills.read(entry)
            assert.ok(instructions.verified && instructions.body.includes('auv.windows.resolve'))
            const reference = await skills.read(entry, 'references/driver.ts')
            assert.ok(reference.verified && reference.text.includes('interface WindowClient'))
            result = { sessionId: `ui-${message.id}` }
          }
          else if (message.method === 'session/prompt') {
            const text = `Connected through the test ACP peer.

| Check | Operation | Expected | Actual | Evidence | Result |
| --- | --- | --- | --- | --- | --- |
| Counter | Click Increment twice and capture the window. | Count: 2 | Count: 2 | Screenshot | Passed |`
            socket.send(JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: {
              sessionId: message.params.sessionId,
              update: { content: { text, type: 'text' }, sessionUpdate: 'agent_message_chunk' },
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
    return { cwd, relayUrl: relay.url }
  },

  async stopAgentFixtures(context) {
    const fixture = fixtures.get(context.testPath)
    fixtures.delete(context.testPath)
    await fixture?.close()
  },
}
