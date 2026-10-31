import type { AnyMessage, SessionUpdate } from '@agentclientprotocol/sdk'

import { AcpThreadController } from '@hafbit/react-acp/core'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { onTestFinished } from 'vitest'

/** Mount the real thread with ACP updates over an in-memory transport. */
export async function mountAgentThread(updates: readonly SessionUpdate[]) {
  let incoming!: ReadableStreamDefaultController<AnyMessage>
  const stream = {
    readable: new ReadableStream<AnyMessage>({ start: controller => void (incoming = controller) }),
    writable: new WritableStream<AnyMessage>({
      write(message) {
        if (!('method' in message) || !('id' in message))
          return
        let result: unknown = {}
        if (message.method === 'initialize') {
          result = { agentCapabilities: { mcpCapabilities: { http: true } }, protocolVersion: 1 }
        }
        else if (message.method === 'session/new') {
          result = { sessionId: 'component-test' }
        }
        else if (message.method === 'session/prompt') {
          for (const update of updates) {
            incoming.enqueue({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'component-test', update } })
          }
          result = { stopReason: 'end_turn' }
        }
        incoming.enqueue({ id: message.id, jsonrpc: '2.0', result })
      },
    }),
  }
  const controller = new AcpThreadController({
    connection: { createStream: () => stream, type: 'stream' },
    workspace: { cwd: '/' },
  })
  onTestFinished(() => controller.dispose())
  await controller.connect()
  const sessionId = await controller.createSession()

  // As in the workbench fixture, load components before generating utility CSS.
  const { AgentSession } = await import('../src/features/agent/AgentSession')
  await import('virtual:uno.css')
  await import('../src/styles.css')
  const container = document.createElement('div')
  container.style.height = '600px'
  document.body.appendChild(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  const connection = { controller, error: '', target: { agentName: 'auv', cwd: '/', deviceId: 'fixture', label: 'Component test' } }
  root.render(<StrictMode><AgentSession connection={connection} onDisconnect={() => controller.disconnect()} onRetry={() => void controller.reconnect()} /></StrictMode>)
  await controller.prompt(sessionId, [{ text: 'Inspect results', type: 'text' }])
  return container
}
