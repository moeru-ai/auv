import type { AccountSession, AgentTarget } from './exterior'

import { useEffect, useState } from 'react'

import { Empty } from '../../components/Empty'
import { Field, Hint } from '../../components/Field'
import { connectAgent, disconnectAgent, useAgentSession } from './agent-session'
import { AgentSession } from './AgentSession'
import { listTargets, useAccount } from './exterior'

/**
 * Agent tab: Exterior sign-in, one of the account's online devices, then a
 * fresh ACP session. The session is never persisted: a new ticket spawns a new
 * agent process, and disconnecting kills it.
 * TODO(agent-session-persist): a reload drops the chat while the script
 * survives (CodeEditor keeps it in localStorage). Restoring the chat needs the
 * Exterior daemon to keep the agent process and to load the ACP session;
 * that lives in the Exterior repo, so it stays out of this tab.
 */
export function AgentTab() {
  const session = useAccount()
  const connection = useAgentSession(state => state.connection)
  if (!connection) {
    // A sign-in phase change remounts the form for fresh device state.
    // Token refreshes keep the current selection.
    return <ConnectView key={session.account.phase} onConnected={target => connectAgent(session, target)} session={session} />
  }
  return (
    <AgentSession
      connection={connection}
      onDisconnect={disconnectAgent}
      onRetry={() => connectAgent(session, connection.target)}
    />
  )
}

function ConnectView({ onConnected, session }: {
  onConnected: (target: AgentTarget) => void
  session: AccountSession
}) {
  const [targets, setTargets] = useState<AgentTarget[]>([])
  const [selection, setSelection] = useState(0)
  const signedIn = session.account.phase === 'signed-in'
  const listDevices = session.listDevices
  // Remounted on every sign-in phase change (see the `key` in `AgentTab`), so
  // the initial status matches the current sign-in without syncing in the effect.
  const [status, setStatus] = useState(() => signedIn ? 'Checking the sign-in.' : '')

  useEffect(() => {
    if (!signedIn)
      return
    let cancelled = false
    void listTargets(listDevices)
      .then((listed) => {
        if (cancelled)
          return
        setTargets(listed)
        setStatus(listed.length === 0 ? 'No agent is available. Add an agent in Exterior on the device you want to use.' : '')
      })
      .catch((error: unknown) => {
        if (!cancelled)
          setStatus(error instanceof Error ? error.message : 'Sign in again.')
      })
    return () => {
      cancelled = true
    }
  }, [listDevices, signedIn])

  if (session.account.phase === 'checking')
    return <Empty>Checking the sign-in.</Empty>

  if (session.account.phase === 'signed-out') {
    return (
      <div className="p-3 flex flex-col gap-2">
        <Hint>Connect an agent on one of your devices to work with this desktop.</Hint>
        <button className="btn-primary self-start" disabled={session.busy} onClick={session.signIn} type="button">
          <span className={session.busy ? 'i-ph-circle-notch animate-spin' : 'i-ph-sign-in'} />
          Sign in with Exterior
        </button>
        {session.error && <span className="text-[12px] text-bad">{session.error}</span>}
      </div>
    )
  }

  const connect = () => {
    const target = targets[selection]
    if (!target || !signedIn)
      return
    onConnected(target)
  }

  return (
    <div className="p-3 flex flex-col gap-3">
      <Field label="Agent on your devices">
        <select
          aria-label="Agent on your devices"
          className="input"
          disabled={targets.length === 0}
          onChange={event => setSelection(Number(event.target.value))}
          value={selection}
        >
          {targets.map((target, index) => <option key={JSON.stringify([target.deviceId, target.agentName])} value={index}>{target.label}</option>)}
        </select>
      </Field>
      <div className="flex gap-2 items-center">
        <button className="btn-primary" disabled={!targets[selection]} onClick={connect} type="button">
          <span className="i-ph-plugs-connected" />
          Connect
        </button>
        <button className="btn-ghost" onClick={session.signOut} type="button">Sign out</button>
      </div>
      {status && <span className="text-[12px] text-fg-muted">{status}</span>}
      <Hint>Each connection starts a new agent session on the selected device. The chat closes when you disconnect.</Hint>
    </div>
  )
}
