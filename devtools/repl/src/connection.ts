import { createAuvBackend } from './backend/auv'
import { MockBackend } from './backend/mock'
import { activateBackend } from './runtime/session'
import { usePlayground } from './store'

const SETTINGS_KEY = 'auv-playground:connection'

export interface SavedConnection {
  credential?: string
  endpoint: string
  /** `local` connects to a loopback daemon without a credential. */
  mode?: 'local' | 'paired'
}

export async function connectMockDesktop(): Promise<void> {
  await activateBackend(new MockBackend())
}

export async function connectSaved(): Promise<boolean> {
  const saved = loadSavedConnection()
  if (!saved.credential && saved.mode !== 'local')
    return false
  usePlayground.setState({ connection: 'connecting' })
  try {
    const { backend } = await createAuvBackend({ credential: saved.credential, endpoint: saved.endpoint })
    await activateBackend(backend)
    return true
  }
  catch (error) {
    usePlayground.setState({ connection: 'error', connectionError: (error as Error).message })
    return false
  }
}

// NOTICE(credential-storage): the paired Device credential is kept in
// localStorage for this local development tool. Any script on this origin can
// read it; do not host the playground on a shared origin with real
// credentials. See README "Sandboxing".
export function loadSavedConnection(): SavedConnection {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY)
    if (raw)
      return JSON.parse(raw) as SavedConnection
  }
  catch {}
  return { endpoint: 'http://127.0.0.1:9847' }
}

export function saveConnection(connection: SavedConnection): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(connection))
  }
  catch {}
}
