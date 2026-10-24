import { useState } from 'react'

import { pairBrowser } from '../../backend/auv'
import { CommandSnippet } from '../../components/CommandSnippet'
import { Field, Hint } from '../../components/Field'
import { addConnection, DEFAULT_ENDPOINT } from './devices'

/** Connects a new daemon by pairing with a one-time token, or with an existing Device credential. */
export function AddDeviceForm({ onDone }: { onDone: () => void }) {
  const [endpoint, setEndpoint] = useState(DEFAULT_ENDPOINT)
  const [name, setName] = useState('')
  const [token, setToken] = useState('')
  const [credential, setCredential] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const bind = remoteBindAddress(endpoint)

  const run = async (resolveCredential: () => Promise<string | undefined>) => {
    setBusy(true)
    setError(undefined)
    try {
      const resolved = await resolveCredential()
      await addConnection({ credential: resolved, endpoint: endpoint.trim(), name: name.trim() || undefined })
      onDone()
    }
    catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
    finally {
      setBusy(false)
    }
  }

  return (
    <div className="p-3 flex flex-col gap-3">
      <div className="flex gap-2">
        <div className="flex-[3]">
          <Field label="Daemon endpoint">
            <input autoFocus className="input font-mono" onChange={event => setEndpoint(event.target.value)} value={endpoint} />
          </Field>
        </div>
        <div className="flex-[2]">
          <Field label="Name (optional)">
            <input className="input" onChange={event => setName(event.target.value)} placeholder={hostOf(endpoint)} value={name} />
          </Field>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <Hint>Start the daemon with a remote listener; it only accepts paired devices:</Hint>
        <CommandSnippet command={`auv serve --remote-bind ${bind}`} />
        <Hint>Create a one-time token on the daemon host:</Hint>
        <CommandSnippet command="auv devices pair create-token" />
        <div className="mt-1 flex gap-2">
          <input aria-label="Pairing token" className="input font-mono" onChange={event => setToken(event.target.value)} placeholder="one-time token" value={token} />
          <button
            className="btn-primary shrink-0"
            disabled={busy || !token.trim()}
            onClick={() => void run(async () => await pairBrowser(endpoint.trim(), token.trim()))}
            type="button"
          >
            {busy && <span className="i-ph-circle-notch animate-spin" />}
            Pair
          </button>
        </div>
        <div className="flex gap-2">
          <input aria-label="Device credential" className="input font-mono" onChange={event => setCredential(event.target.value)} placeholder="…or an existing Device credential" type="password" value={credential} />
          <button className="btn-ghost border border-line shrink-0" disabled={busy || !credential.trim()} onClick={() => void run(async () => credential.trim())} type="button">
            Connect
          </button>
        </div>
      </div>

      {error && (
        <div className="text-[12.5px] text-bad px-3 py-2 rounded-md bg-bad/10 flex gap-2 items-start" role="alert">
          <span className="i-ph-x-circle mt-0.5 shrink-0" />
          <span className="[overflow-wrap:anywhere]">{error}</span>
        </div>
      )}
    </div>
  )
}

function hostOf(endpoint: string): string {
  try {
    return new URL(endpoint).host
  }
  catch {
    return 'my-mac'
  }
}

/** `auv serve --remote-bind` `IP:PORT` for the endpoint typed in the form. */
function remoteBindAddress(endpoint: string): string {
  try {
    return new URL(endpoint).host
  }
  catch {
    return new URL(DEFAULT_ENDPOINT).host
  }
}
