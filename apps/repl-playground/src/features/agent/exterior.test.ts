import { afterEach, describe, expect, it, vi } from 'vitest'

import clientMetadata from '../../../public/oauth-client.json'

import { listTargets, useAccount } from './exterior'

const upstream = vi.hoisted(() => ({ useAccount: vi.fn() }))
vi.mock('@exterioragentprotocol/sdk/client/react', () => ({ useAccount: upstream.useAccount }))

afterEach(() => {
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('exterior connection adapter', () => {
  it('uses the client ID issued by the configured relay', () => {
    vi.stubGlobal('window', { location: { origin: 'https://playground.example' } })
    vi.stubEnv('BASE_URL', '/playground/')
    vi.stubEnv('VITE_EXTERIOR_URL', 'https://relay.example')
    vi.stubEnv('VITE_EXTERIOR_CLIENT_ID', 'registered-client')
    useAccount()
    expect(upstream.useAccount).toHaveBeenCalledWith({
      clientId: 'registered-client',
      redirectUri: 'https://playground.example/playground/',
      relayUrl: 'https://relay.example',
    })
  })

  it('preserves the device working directory when discovering an agent', async () => {
    const cwd = String.raw`C:\Users\owner\repo`
    const listDevices = vi.fn().mockResolvedValue([
      { agents: [], hostname: 'empty', id: 'empty', platform: 'linux', workingDirectory: '/home/empty' },
      { agents: [{ agentType: 'custom', name: 'auv' }], hostname: 'workstation', id: 'device', platform: 'windows', workingDirectory: cwd },
    ])
    const targets = await listTargets(listDevices)
    expect(listDevices).toHaveBeenCalledWith()
    expect(targets).toEqual([{ agentName: 'auv', cwd, deviceId: 'device', label: 'workstation / auv' }])
  })

  it.each([
    'https://auv.moeru.ai',
    'http://127.0.0.1:5180',
    'http://localhost:5180',
  ])('uses the registered Playground entry URL on %s', (origin) => {
    vi.stubGlobal('window', { location: { origin } })
    vi.stubEnv('BASE_URL', '/playground/')
    vi.stubEnv('VITE_EXTERIOR_CLIENT_ID', undefined)
    vi.stubEnv('VITE_EXTERIOR_URL', 'https://relay.example')
    const account = { account: { phase: 'signed-out' } }
    upstream.useAccount.mockReturnValue(account)
    expect(useAccount()).toBe(account)
    const redirectUri = `${origin}/playground/`
    expect(clientMetadata.redirect_uris).toContain(redirectUri)
    expect(upstream.useAccount).toHaveBeenCalledWith({
      clientId: clientMetadata.client_id,
      redirectUri,
      relayUrl: 'https://relay.example',
    })
  })

  it('uses the local relay when no relay URL is configured', () => {
    vi.stubGlobal('window', { location: { origin: 'https://playground.example' } })
    vi.stubEnv('VITE_EXTERIOR_URL', undefined)
    useAccount()
    expect(upstream.useAccount).toHaveBeenCalledWith(expect.objectContaining({
      relayUrl: 'http://127.0.0.1:8787',
    }))
  })
})
