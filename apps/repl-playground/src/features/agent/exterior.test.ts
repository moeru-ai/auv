import { afterEach, describe, expect, it, vi } from 'vitest'

import clientMetadata from '../../../public/oauth-client.json'

import { listTargets, useAccount } from './exterior'

const upstream = vi.hoisted(() => ({ listDevices: vi.fn(), useAccount: vi.fn() }))
vi.mock('@exterioragentprotocol/sdk/client', () => ({ browserCallbackPath: '/oauth/callback' }))
vi.mock('@exterioragentprotocol/sdk/client/react', () => ({ useAccount: upstream.useAccount }))

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('exterior connection adapter', () => {
  // ROOT CAUSE:
  //
  // Without an explicit client id, the old SDK tried anonymous registration.
  // Relays now omit that endpoint, so sign-in failed before authorization.
  it('starts sign-in with the configured client identity without anonymous registration', async () => {
    vi.stubGlobal('window', { location: { origin: 'https://playground.example' } })
    vi.stubEnv('BASE_URL', '/playground/')
    vi.stubEnv('VITE_EXTERIOR_URL', 'https://relay.example')
    vi.stubEnv('VITE_EXTERIOR_CLIENT_ID', 'registered-development-client')
    const fetch = vi.fn(async (input: RequestInfo | URL) => {
      const url = new URL(input instanceof Request ? input.url : input.toString())
      const metadata = url.pathname === '/.well-known/oauth-protected-resource'
        ? { authorization_servers: ['https://relay.example/api/auth'], resource: 'https://relay.example' }
        : {
            authorization_endpoint: 'https://relay.example/api/auth/oauth2/authorize',
            issuer: 'https://relay.example/api/auth',
            token_endpoint: 'https://relay.example/api/auth/oauth2/token',
          }
      return new Response(JSON.stringify(metadata), { headers: { 'content-type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetch)
    useAccount()
    const [, config] = upstream.useAccount.mock.lastCall!
    const { beginBrowserSignIn } = await vi.importActual<typeof import('@exterioragentprotocol/sdk/client')>('@exterioragentprotocol/sdk/client')
    const values = new Map<string, string>()
    const store = {
      getItem: (key: string) => values.get(key) ?? null,
      removeItem: (key: string) => { values.delete(key) },
      setItem: (key: string, value: string) => { values.set(key, value) },
    }
    const url = new URL(await beginBrowserSignIn({ ...config, relayUrl: 'https://relay.example', store }))
    expect(url.searchParams.get('client_id')).toBe('registered-development-client')
    expect(url.searchParams.get('redirect_uri')).toBe('https://playground.example/playground/oauth/callback')
    expect(url.searchParams.get('scope')).toBe('eap:client offline_access')
    expect(fetch.mock.calls.every(([input]) => !input.toString().includes('/register'))).toBe(true)
  })

  it('preserves the device working directory when discovering an agent', async () => {
    const cwd = String.raw`C:\Users\owner\repo`
    vi.stubEnv('VITE_EXTERIOR_URL', 'https://relay.example/path')
    upstream.listDevices.mockResolvedValue([
      { agents: [], hostname: 'empty', id: 'empty', platform: 'linux', workingDirectory: '/home/empty' },
      { agents: [{ agentType: 'custom', name: 'auv' }], hostname: 'workstation', id: 'device', platform: 'windows', workingDirectory: cwd },
    ])
    const targets = await listTargets(upstream.listDevices)
    expect(upstream.listDevices).toHaveBeenCalledWith()
    expect(targets).toEqual([{ agentName: 'auv', cwd, deviceId: 'device', label: 'workstation / auv' }])
  })

  it('keeps the OAuth callback under the Playground base path', () => {
    vi.stubGlobal('window', { location: { origin: 'https://playground.example' } })
    vi.stubEnv('BASE_URL', '/playground/')
    vi.stubEnv('VITE_EXTERIOR_URL', 'https://relay.example')
    const account = { account: { phase: 'signed-out' } }
    upstream.useAccount.mockReturnValue(account)
    expect(useAccount()).toBe(account)
    expect(upstream.useAccount).toHaveBeenCalledWith('https://relay.example', {
      clientId: clientMetadata.client_id,
      redirectUri: 'https://playground.example/playground/oauth/callback',
      returnTo: '/playground/',
    })
  })

  it('uses the local relay default without a valid override', () => {
    vi.stubGlobal('window', { location: { origin: 'https://playground.example' } })
    vi.stubEnv('VITE_EXTERIOR_URL', 'invalid origin')
    useAccount()
    expect(upstream.useAccount).toHaveBeenCalledWith('http://127.0.0.1:8787', expect.objectContaining({ clientId: clientMetadata.client_id }))
  })
})
