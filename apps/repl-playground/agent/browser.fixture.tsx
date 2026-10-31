import type { browserCommands } from './browser.commands.mjs'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { onTestFinished, vi } from 'vitest'
import { commands } from 'vitest/browser'

import { disconnectAgent } from '../src/features/agent/agent-session'
import { session } from '../src/runtime/session'

declare module 'vitest/browser' {
  interface BrowserCommands {
    agentFixtureState: (options?: Parameters<typeof browserCommands.agentFixtureState>[1]) => Promise<Awaited<ReturnType<typeof browserCommands.agentFixtureState>>>
    agentScreenshot: (name: 'agent' | 'codemode' | 'connected') => Promise<void>
    startAgentFixtures: (image?: { data: string, mimeType: string, type: 'image' }) => Promise<Awaited<ReturnType<typeof browserCommands.startAgentFixtures>>>
    stopAgentFixtures: () => Promise<void>
  }
}

/** Mount the actual workbench; only its external Relay and ACP peers are synthetic. */
export async function mountPlayground(signedIn = false) {
  const data = document.createElement('canvas').toDataURL('image/png').split(',')[1]
  const fixture = await commands.startAgentFixtures({ data, mimeType: 'image/png', type: 'image' })
  const container = document.createElement('div')
  container.id = 'root'
  document.body.appendChild(container)
  let root = createRoot(container)
  onTestFinished(async () => {
    disconnectAgent()
    session.setLive(false)
    session.reset()
    root.unmount()
    container.remove()
    sessionStorage.clear()
    vi.unstubAllEnvs()
    await commands.stopAgentFixtures()
  })
  // Load application modules before UnoCSS generates their utility classes.
  const { App } = await import('../src/App')
  await import('virtual:uno.css')
  await import('../src/styles.css')
  vi.stubEnv('VITE_EXTERIOR_URL', fixture.relayUrl)
  if (signedIn) {
    sessionStorage.setItem('exterior.accessToken', 'synthetic-token')
    sessionStorage.setItem('exterior.expiresAt', String(Date.now() + 3600000))
  }
  const render = () => root.render(<StrictMode><App /></StrictMode>)
  render()
  return {
    ...fixture,
    remount() {
      // Rebuild only the view. A user-owned connection must survive its subscribers.
      root.unmount()
      root = createRoot(container)
      render()
    },
  }
}
