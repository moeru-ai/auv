import type { ServedMcp } from '@exterioragentprotocol/sdk/client'
import type { AccountSession } from '@exterioragentprotocol/sdk/client/react'
import type { AcpClientConnection, AcpThreadState } from '@hafbit/react-acp/core'

import type { AgentTarget } from './exterior'

import { createWebSocketStream } from '@agentclientprotocol/sdk/experimental/ws-client'
import { serveMcp } from '@exterioragentprotocol/sdk/client'
import { AcpThreadController, SdkAcpClientAdapter } from '@hafbit/react-acp/core'
import { create } from 'zustand'

import { version } from '../../../package.json'
import { CodeModeHost } from './codemode-bridge'
import { relayUrl } from './exterior'

export interface AgentConnection {
  controller: AcpThreadController
  error: string
  target: AgentTarget
}

export function agentSessionStatus({ activeSessionId, capabilities, connectionStatus }: AcpThreadState) {
  if (connectionStatus !== 'ready')
    return connectionStatus
  if (capabilities?.mcpCapabilities?.http !== true)
    return 'http-mcp-required'
  return activeSessionId === undefined ? 'starting-session' : 'ready'
}

/** The page owns the connection; React views only subscribe to it. */
export const useAgentSession = create<{ connection: AgentConnection | null }>(() => ({ connection: null }))

/**
 * Adds the Playground MCP endpoint to the typed ACP connection.
 * This adapter owns session setup and endpoint cleanup; the SDK owns messages.
 */
export function codeModeConnection(connection: AcpClientConnection, host: CodeModeHost, endpoint: Pick<ServedMcp, 'close' | 'headers' | 'url'>): AcpClientConnection {
  const close = () => {
    host.close()
    endpoint.close()
  }
  if (connection.signal.aborted)
    close()
  else connection.signal.addEventListener('abort', close, { once: true })
  return {
    ...connection,
    cancel(sessionId) {
      host.cancel()
      return connection.cancel(sessionId)
    },
    newSession(request) {
      host.setSession(undefined)
      const { headers, url } = endpoint
      return connection.newSession({ ...request, mcpServers: [...request.mcpServers, { headers, name: 'auv-playground', type: 'http', url }] })
    },
  }
}

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
          const token = await account.accessToken()
          const endpoint = await serveMcp(relayUrl(), token, await codemode.createServerFactory(), signal)
          try {
            signal.throwIfAborted()
            const { url } = await account.openConnection({ agentName: target.agentName, deviceId: target.deviceId }, signal)
            signal.throwIfAborted()
            void endpoint.closed.then((reason) => {
              if (signal.aborted)
                return
              codemode.close()
              setError(reason === 'revoked' ? 'Access was revoked. Sign in again.' : 'The tool connection closed. Reconnect the agent.')
            })
            const acp = await new SdkAcpClientAdapter(() => createWebSocketStream(url)).connect(options)
            return codeModeConnection(acp, codemode, endpoint)
          }
          catch (cause) {
            endpoint.close()
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

  function setError(error: string) {
    useAgentSession.setState((state) => {
      if (state.connection?.controller !== controller)
        return state
      return { connection: { ...state.connection, error } }
    })
  }

  useAgentSession.setState({ connection: { controller, error: '', target } })
  void controller.connect().then(async () => {
    const connection = useAgentSession.getState().connection
    if (connection?.controller !== controller || agentSessionStatus(controller.getState()) !== 'starting-session')
      return
    await controller.createSession()
  }).catch((cause: unknown) => {
    controller.dispose()
    setError(cause instanceof Error ? cause.message : String(cause))
  })
}

export function disconnectAgent(): void {
  useAgentSession.getState().connection?.controller.dispose()
  useAgentSession.setState({ connection: null })
}

if (import.meta.hot)
  import.meta.hot.dispose(disconnectAgent)
