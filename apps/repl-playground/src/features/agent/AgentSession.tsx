import type { AgentConnection } from './agent-session'

import { AssistantRuntimeProvider } from '@assistant-ui/react'
import { useAcpControllerRuntime, useAcpThreadState } from '@hafbit/react-acp'

import { Empty } from '../../components/Empty'
import { AgentThread } from './AgentThread'

/** Displays the caller-owned connection without changing its lifetime. */
export function AgentSession({ connection, onDisconnect, onRetry }: {
  connection: AgentConnection
  onDisconnect: () => void
  onRetry: () => void
}) {
  // NOTICE(acp-controller-runtime): `patches/@hafbit__react-acp@0.1.9.patch`
  // exposes the controller projection. Remove after an upstream release exports it.
  const runtime = useAcpControllerRuntime(connection.controller)
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex flex-col h-full min-h-0">
        <div className="px-3 border-b border-line flex shrink-0 gap-2 h-8 items-center">
          <span className="text-[12px] text-fg-muted font-medium flex-1 truncate">{connection.target.label}</span>
          <button className="btn-ghost text-[12px] px-2 h-6" onClick={onDisconnect} type="button">
            <span className="i-ph-plugs" />
            Disconnect
          </button>
        </div>
        <div className="flex-1 min-h-0">
          <SessionBody connection={connection} onRetry={onRetry} />
        </div>
      </div>
    </AssistantRuntimeProvider>
  )
}

/** Human-readable form of the thread's connection error, when it carries one. */
function describeConnectionError(value: unknown): string {
  if (!value)
    return ''
  if (value instanceof Error)
    return value.message
  if (typeof value === 'string')
    return value
  try {
    const text = JSON.stringify(value)
    return text === '{}' ? '' : text
  }
  catch {
    return ''
  }
}

function SessionBody({ connection, onRetry }: {
  connection: AgentConnection
  onRetry: () => void
}) {
  const thread = useAcpThreadState()
  const status = thread.connectionStatus

  // TODO(agent-auth): interactive ACP authentication requires an approved
  // agent-authentication flow; this panel reports the requirement.
  if (status === 'auth-required')
    return <Empty>The agent requires authentication that this tab cannot complete.</Empty>
  if (connection.error || status === 'error' || status === 'closed') {
    const hint = describeConnectionError(thread.connectionError)
    const message = connection.error || hint || status
    return (
      <Empty>
        <span className="flex flex-col gap-2 max-w-72 items-center">
          <span className="text-bad">{message}</span>
          {hint && hint !== message && <span className="text-[11.5px] text-fg-muted">{hint}</span>}
          <button className="btn-ghost" onClick={onRetry} type="button">Reconnect</button>
        </span>
      </Empty>
    )
  }
  if (status !== 'ready')
    return <Empty>{status === 'connecting' ? 'Opening the agent.' : status}</Empty>
  if (thread.capabilities?.mcpCapabilities?.http !== true)
    return <Empty>This agent does not support HTTP MCP. Choose an agent with HTTP MCP support.</Empty>
  if (thread.activeSessionId === undefined)
    return <Empty>Starting a new session.</Empty>
  return <AgentThread />
}
