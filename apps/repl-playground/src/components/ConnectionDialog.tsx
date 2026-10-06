import { Dialog } from '@base-ui/react/dialog'
import { Tabs } from '@base-ui/react/tabs'
import { useState } from 'react'

import { createAuvBackend, pairBrowser } from '../backend/auv'
import { connectMockDesktop, loadSavedConnection, saveConnection } from '../connection'
import { activateBackend } from '../runtime/session'
import { usePlayground } from '../store'
import { CommandSnippet } from './ui'

export function ConnectionDialog() {
  const backend = usePlayground(state => state.backend)
  const connection = usePlayground(state => state.connection)
  const [open, setOpen] = useState(false)
  const [endpoint, setEndpoint] = useState(() => loadSavedConnection().endpoint)
  const [mode, setMode] = useState<'local' | 'paired'>(() => (loadSavedConnection().credential ? 'paired' : 'local'))
  const [token, setToken] = useState('')
  const [credential, setCredential] = useState(() => loadSavedConnection().credential ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()

  /** Connects with a paired credential, or without one to a local (unpaired) daemon. */
  const connectWith = async (nextCredential?: string) => {
    usePlayground.setState({ connection: 'connecting' })
    const { backend: next } = await createAuvBackend({ credential: nextCredential, endpoint })
    saveConnection({ credential: nextCredential, endpoint, mode: nextCredential ? 'paired' : 'local' })
    await activateBackend(next)
  }

  const run = async (task: () => Promise<void>) => {
    setBusy(true)
    setError(undefined)
    try {
      await task()
      setOpen(false)
    }
    catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause)
      setError(message)
      usePlayground.setState({ connection: 'error', connectionError: message })
    }
    finally {
      setBusy(false)
    }
  }

  const dot = connection === 'connected' ? 'bg-good' : connection === 'connecting' ? 'bg-warn animate-pulse' : connection === 'error' ? 'bg-bad' : 'bg-fg-subtle'
  const listen = listenUri(endpoint)

  return (
    <Dialog.Root onOpenChange={setOpen} open={open}>
      <Dialog.Trigger
        className="text-fg-muted rounded-lg flex size-9 cursor-pointer items-center justify-center relative hover:(text-fg bg-surface-2)"
        title={backend ? `Device: ${backend.label}` : connection === 'connecting' ? 'Connecting…' : 'Connect a device'}
      >
        <span className="i-lucide-monitor-cog text-[18px]" />
        <span className={`rounded-full size-2 ring-2 ring-surface-1 bottom-1.5 right-1.5 absolute ${dot}`} />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className="bg-black/50 transition-opacity inset-0 fixed backdrop-blur-[2px] data-[ending-style]:opacity-0 data-[starting-style]:opacity-0" />
        <Dialog.Popup className="p-5 border border-line rounded-xl bg-surface-1 w-[460px] shadow-2xl transition-all left-1/2 top-1/2 fixed data-[ending-style]:(opacity-0 scale-96) data-[starting-style]:(opacity-0 scale-96) -translate-x-1/2 -translate-y-1/2">
          <Dialog.Title className="text-[15px] font-semibold m-0">Connect a device</Dialog.Title>
          <Dialog.Description className="text-fg-muted mb-4 mt-1">
            Connect to an AUV daemon on this machine, pair with one elsewhere, or explore with the built-in mock desktop.
          </Dialog.Description>

          <div className="flex flex-col gap-3">
            <Field label="Daemon endpoint">
              <input className="input font-mono" onChange={event => setEndpoint(event.target.value)} value={endpoint} />
            </Field>

            <Tabs.Root onValueChange={value => setMode(value as 'local' | 'paired')} value={mode}>
              <Tabs.List className="p-0.5 rounded-lg bg-surface-0 flex gap-0.5">
                {([['local', 'Local · no pairing'], ['paired', 'Paired']] as const).map(([value, label]) => (
                  <Tabs.Tab
                    className="text-[12.5px] text-fg-muted font-medium rounded-md flex-1 h-7 cursor-pointer data-[active]:(text-fg bg-surface-2)"
                    key={value}
                    value={value}
                  >
                    {label}
                  </Tabs.Tab>
                ))}
              </Tabs.List>

              <Tabs.Panel className="pt-3 flex flex-col gap-2" value="local">
                <Hint>Start a daemon on this machine. A loopback listener without a pairing store accepts local connections without a credential (development only):</Hint>
                <CommandSnippet command={`auv serve --listen ${listen}`} />
                <button className="btn-primary mt-1 self-start" disabled={busy} onClick={() => void run(() => connectWith())} type="button">
                  <span className="i-lucide-plug" />
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
                    onClick={() => void run(async () => {
                      const paired = await pairBrowser(endpoint, token.trim())
                      setCredential(paired)
                      await connectWith(paired)
                    })}
                    type="button"
                  >
                    Pair
                  </button>
                </div>
                <div className="flex gap-2">
                  <input aria-label="Device credential" className="input font-mono" onChange={event => setCredential(event.target.value)} placeholder="…or an existing Device credential" type="password" value={credential} />
                  <button className="btn-ghost border border-line shrink-0" disabled={busy || !credential.trim()} onClick={() => void run(() => connectWith(credential.trim()))} type="button">
                    Connect
                  </button>
                </div>
              </Tabs.Panel>
            </Tabs.Root>

            {error && <div className="text-[12.5px] text-bad px-3 py-2 rounded-md bg-bad/10">{error}</div>}

            <div className="mt-1 pt-4 border-t border-line flex items-center justify-between">
              <button className="btn-ghost" disabled={busy} onClick={() => void run(connectMockDesktop)} type="button">
                <span className="i-lucide-monitor-smartphone" />
                Use mock desktop
              </button>
              <Dialog.Close className="btn-ghost">Close</Dialog.Close>
            </div>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function Field({ children, label }: { children: React.ReactNode, label: string }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-[12px] text-fg-muted font-medium">{label}</span>
      {children}
    </label>
  )
}

function Hint({ children }: { children: React.ReactNode }) {
  return <span className="text-[11.5px] text-fg-subtle">{children}</span>
}

/** `auv serve --listen` value for the endpoint typed in the dialog. */
function listenUri(endpoint: string): string {
  try {
    const url = new URL(endpoint)
    return `${url.protocol}//${url.host}`
  }
  catch {
    return 'http://127.0.0.1:9847'
  }
}
