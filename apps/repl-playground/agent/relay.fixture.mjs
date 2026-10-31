import { randomUUID } from 'node:crypto'
import { createServer } from 'node:http'

import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js'
import { WebSocketServer } from 'ws'

/** Synthetic Exterior MCP wire peer. Tool execution stays in the actual browser. */
export async function createMcpRelay() {
  const endpoints = new Map()
  const registrations = []
  const sockets = new WebSocketServer({ noServer: true })
  let url
  const server = createServer(async (request, response) => {
    response.setHeader('Access-Control-Allow-Origin', '*')
    response.setHeader('Access-Control-Allow-Headers', 'Authorization, Content-Type, MCP-Session-Id, MCP-Protocol-Version, Last-Event-ID')
    response.setHeader('Access-Control-Expose-Headers', 'MCP-Session-Id')
    response.setHeader('Access-Control-Allow-Methods', 'POST, GET, DELETE, OPTIONS')
    if (request.method === 'OPTIONS') {
      response.writeHead(204).end()
      return
    }
    if (request.url === '/eap/v1/mcp-endpoints' && request.method === 'POST') {
      const token = randomUUID()
      endpoints.set(token, { sessions: new Map() })
      registrations.push(request.headers.authorization)
      response.writeHead(201, { 'Content-Type': 'application/json' }).end(JSON.stringify({
        mcpUrl: `${url}/eap/v1/mcp`,
        token,
        url: `${url.replace('http:', 'ws:')}/provider?ticket=${token}`,
      }))
      return
    }
    const token = request.headers.authorization?.replace(/^Bearer /, '')
    const endpoint = endpoints.get(token)
    if (request.url !== '/eap/v1/mcp' || !endpoint?.socket) {
      response.writeHead(404).end()
      return
    }
    const sessionId = request.headers['mcp-session-id']
    let transport = endpoint.sessions.get(sessionId)
    if (!sessionId) {
      transport = new StreamableHTTPServerTransport({
        onsessioninitialized: session => endpoint.sessions.set(session, transport),
        sessionIdGenerator: randomUUID,
      })
      transport.onmessage = message => endpoint.socket.send(JSON.stringify({ message, session: transport.sessionId }))
      transport.onclose = () => {
        endpoint.sessions.delete(transport.sessionId)
        if (endpoint.socket?.readyState === 1)
          endpoint.socket.send(JSON.stringify({ closed: true, session: transport.sessionId }))
      }
    }
    if (!transport) {
      response.writeHead(404).end()
      return
    }
    await transport.handleRequest(request, response)
  })
  server.on('upgrade', (request, socket, head) => {
    const token = new URL(request.url, url).searchParams.get('ticket')
    const endpoint = endpoints.get(token)
    if (!endpoint || endpoint.socket) {
      socket.destroy()
      return
    }
    sockets.handleUpgrade(request, socket, head, (page) => {
      endpoint.socket = page
      page.on('message', (data) => {
        const frame = JSON.parse(data.toString())
        void endpoint.sessions.get(frame.session)?.send(frame.message, { relatedRequestId: frame.relatedRequestId })
      })
      page.on('close', () => {
        endpoints.delete(token)
        for (const transport of endpoint.sessions.values())
          void transport.close()
      })
    })
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  url = `http://127.0.0.1:${server.address().port}`
  return {
    async close() {
      for (const endpoint of endpoints.values()) {
        for (const transport of endpoint.sessions.values())
          await transport.close()
        endpoint.socket?.terminate()
      }
      sockets.close()
      server.closeAllConnections()
      await new Promise(resolve => server.close(resolve))
    },
    registrations,
    url,
  }
}
