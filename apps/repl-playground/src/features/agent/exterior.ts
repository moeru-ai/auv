import type { AccountSession } from '@exterioragentprotocol/sdk/client/react'

import { useAccount as useExteriorAccount } from '@exterioragentprotocol/sdk/client/react'

/** A discoverable agent slot and the device's default ACP working directory. */
export interface AgentTarget {
  agentName: string
  cwd: string
  deviceId: string
  label: string
}

/** Normalize Exterior devices into selectable agent targets. */
export async function listTargets(listDevices: AccountSession['listDevices']): Promise<AgentTarget[]> {
  const devices = await listDevices()
  return devices.flatMap(device => device.agents.map(agent => ({
    agentName: agent.name,
    cwd: device.workingDirectory,
    deviceId: device.id,
    label: `${device.hostname} / ${agent.name}`,
  })))
}

export function relayUrl(): string {
  return import.meta.env.VITE_EXTERIOR_URL || 'http://127.0.0.1:8787'
}

export function useAccount(): AccountSession {
  return useExteriorAccount({
    // NOTICE(exterior-client-identity): the relay must fetch this document over
    // HTTPS. Until publication, use the relay's app setup flow for a client id.
    clientId: import.meta.env.VITE_EXTERIOR_CLIENT_ID || 'https://auv.moeru.ai/playground/oauth-client.json',
    redirectUri: new URL(import.meta.env.BASE_URL, window.location.origin).href,
    relayUrl: relayUrl(),
  })
}
