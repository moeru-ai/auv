import type { AccountSession, AgentTarget } from './exterior'

import { createWebSocketStream } from '@agentclientprotocol/sdk/experimental/ws-client'
import { AcpThreadController, SdkAcpClientAdapter } from '@hafbit/react-acp/core'
import { create } from 'zustand'

import { version } from '../../../package.json'
import { codeModeConnection, CodeModeHost } from './codemode-bridge'
import { relayUrl, serveMcp } from './exterior'

export interface AgentConnection {
  controller: AcpThreadController
  error: string
  target: AgentTarget
}

/** The page owns the connection; React views only subscribe to it. */
export const useAgentSession = create<{ connection: AgentConnection | null }>(() => ({ connection: null }))

/** A Connect action opens one agent, independently of the view's lifetime. */
export function connectAgent(account: AccountSession, target: AgentTarget): void {
  disconnectAgent()
  const codemode = new CodeModeHost()
  const controller = new AcpThreadController({
    clientInfo: { name: 'auv-playground', version },
    connection: {
      adapter: {
        async connect(options) {
          const { signal } = options
          const tools = await serveMcp(relayUrl(), await account.accessToken(), transport => void codemode.connect(transport), signal)
          try {
            signal.throwIfAborted()
            const { url } = await account.openConnection({ agentName: target.agentName, deviceId: target.deviceId }, signal)
            signal.throwIfAborted()
            void tools.closed.then((reason) => {
              if (!signal.aborted) {
                codemode.cancel()
                update({ error: reason === 'revoked' ? 'Access was revoked. Sign in again.' : 'The tool connection closed. Reconnect the agent.' })
              }
            })
            const acp = await new SdkAcpClientAdapter(() => createWebSocketStream(url)).connect(options)
            return codeModeConnection(acp, codemode, tools)
          }
          catch (cause) {
            tools.close()
            throw cause
          }
        },
      },
      type: 'adapter',
    },
    onThreadIdChange: sessionId => codemode.setSession(sessionId),
    // TODO(agent-directory): an explicit directory picker is deferred until
    // an approved workspace-selection slice; use the device's directory.
    workspace: { cwd: target.cwd },
  })

  function update(change: Partial<AgentConnection>) {
    useAgentSession.setState((state) => {
      if (state.connection?.controller !== controller)
        return state
      return { connection: { ...state.connection, ...change } }
    })
  }

  useAgentSession.setState({ connection: { controller, error: '', target } })
  void controller.connect().then(async () => {
    const connection = useAgentSession.getState().connection
    const { capabilities, connectionStatus } = controller.getState()
    if (connection?.controller !== controller || capabilities?.mcpCapabilities?.http !== true || connectionStatus !== 'ready')
      return
    await controller.createSession()
  }).catch((cause: unknown) => {
    controller.dispose()
    update({ error: cause instanceof Error ? cause.message : String(cause) })
  })
}

export function disconnectAgent(): void {
  useAgentSession.getState().connection?.controller.dispose()
  useAgentSession.setState({ connection: null })
}

if (import.meta.hot)
  import.meta.hot.dispose(disconnectAgent)
