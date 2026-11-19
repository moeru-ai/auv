import { createMcpEndpoints } from '@exterioragentprotocol/sdk/relay'
import { createAdaptorServer } from '@hono/node-server'
import { WebSocketServer } from 'ws'

export async function createMcpRelay() {
  const endpoints = createMcpEndpoints()
  const registrations = []
  const sockets = new WebSocketServer({ noServer: true })
  let url
  const server = createAdaptorServer({
    async fetch(request) {
      const path = new URL(request.url).pathname
      let response
      if (request.method === 'OPTIONS') {
        response = new Response(null, { status: 204 })
        response.headers.set('Access-Control-Allow-Headers', request.headers.get('access-control-request-headers') ?? '')
        response.headers.set('Access-Control-Allow-Methods', 'POST, OPTIONS')
      }
      else if (path === '/eap/v1/mcp-endpoints' && request.method === 'POST') {
        const { ticket, token } = endpoints.open({ clientId: 'playground', userId: 'fixture' })
        registrations.push(request.headers.get('authorization'))
        response = Response.json({
          mcpUrl: `${url}/eap/v1/mcp`,
          protocolVersion: '2026-07-28',
          token,
          url: `${url.replace('http:', 'ws:')}/provider?ticket=${ticket}`,
        }, { status: 201 })
      }
      else {
        const token = request.headers.get('authorization')?.replace(/^Bearer /, '') ?? ''
        response = await endpoints.handle(token, request)
      }
      response.headers.set('Access-Control-Allow-Origin', '*')
      return response
    },
  })
  server.on('upgrade', (request, socket, head) => {
    const ticket = new URL(request.url, url).searchParams.get('ticket')
    sockets.handleUpgrade(request, socket, head, (page) => {
      const link = endpoints.acceptSocket(ticket, { close: () => page.close(), send: frame => page.send(frame) })
      if (!link) {
        page.close()
        return
      }
      page.on('message', data => link.frame(data.toString()))
      page.on('close', () => link.closed())
    })
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  url = `http://127.0.0.1:${server.address().port}`
  return {
    async close() {
      for (const socket of sockets.clients)
        socket.terminate()
      sockets.close()
      server.closeAllConnections()
      await new Promise(resolve => server.close(resolve))
    },
    registrations,
    url,
  }
}
