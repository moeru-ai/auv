import type { TestProject } from 'vitest/node'

import { isWindows } from 'std-env'

import { setupPairedAuvDaemon } from './auv-daemon'

export type BrowserAuvDaemonContext
  = | { readonly available: false }
    | {
      readonly available: true
      readonly credential: string
      readonly endpoint: string
    }

declare module 'vitest' {
  export interface ProvidedContext {
    auvBrowserDaemon: BrowserAuvDaemonContext
  }
}

/** Starts and pairs the daemon before browser tests without bundling Node helpers. */
export async function setup(project: TestProject): Promise<() => Promise<void>> {
  if (isWindows) {
    // TODO(windows-browser-fixture): this pairing fixture still hardcodes a
    // Unix owner socket. Port it to the Windows named-pipe transport when
    // browser pairing coverage becomes an approved slice.
    project.provide('auvBrowserDaemon', { available: false })
    return () => Promise.resolve()
  }

  const daemon = await setupPairedAuvDaemon('auv-js-browser-integration')

  project.provide('auvBrowserDaemon', {
    available: true,
    credential: daemon.credential,
    endpoint: daemon.remoteEndpoint,
  })

  return daemon.stop
}
