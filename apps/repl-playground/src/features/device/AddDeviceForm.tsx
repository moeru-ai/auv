import { Tabs } from '@base-ui/react/tabs'
import { useState } from 'react'

import { pairBrowser } from '../../backend/auv'
import { CommandSnippet } from '../../components/CommandSnippet'
import { Field, Hint } from '../../components/Field'
import { TabBar } from '../../components/TabBar'
import { addConnection, DEFAULT_ENDPOINT } from './devices'

type Mode = 'local' | 'paired'

/** Connects a new daemon: a local loopback one without pairing, or a paired one with a token or credential. */
export function AddDeviceForm({ onDone }: { onDone: () => void }) {
  const [endpoint, setEndpoint] = useState(DEFAULT_ENDPOINT)
  const [name, setName] = useState('')
  const [mode, setMode] = useState<Mode>('local')
  const [token, setToken] = useState('')
  const [credential, setCredential] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const listen = listenUri(endpoint)

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

      <Tabs.Root onValueChange={value => setMode(value as Mode)} value={mode}>
        <TabBar
          tabs={[
            { icon: 'i-ph-desktop', label: 'Local · no pairing', value: 'local' },
            { icon: 'i-ph-lock-simple', label: 'Paired', value: 'paired' },
          ]}
          variant="segmented"
        />

        <Tabs.Panel className="pt-3 flex flex-col gap-2" value="local">
          <Hint>Start a daemon on this machine. A loopback listener without a pairing store accepts local connections without a credential (development only):</Hint>
          <CommandSnippet command={`auv serve --listen ${listen}`} />
          <button className="btn-primary mt-1 self-start" disabled={busy} onClick={() => void run(async () => undefined)} type="button">
            <span className={busy ? 'i-ph-circle-notch animate-spin' : 'i-ph-link'} />
            Connect
          </button>
        </Tabs.Panel>

        <Tabs.Panel className="pt-3 flex flex-col gap-2" value="paired">
          <Hint>Start the daemon with a pairing store (any writable path); its HTTP listener then requires a paired credential:</Hint>
          <CommandSnippet command={`auv serve --listen ${listen} --pairing-store ~/.auv/pairings.json`} />
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
        </Tabs.Panel>
      </Tabs.Root>

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

/** `auv serve --listen` value for the endpoint typed in the form. */
function listenUri(endpoint: string): string {
  try {
    const url = new URL(endpoint)
    return `${url.protocol}//${url.host}`
  }
  catch {
    return DEFAULT_ENDPOINT
  }
}
