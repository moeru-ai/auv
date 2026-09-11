import type { TestProject } from 'vitest/node'

import { isLinux, isMacOS, isWindows } from 'std-env'

import { neteaseMusicRunner, setupPairedAuvDaemon } from './auv-daemon'

export type BrowserAuvDaemonContext
  = | { readonly available: false }
    | {
      readonly available: true
      readonly credential: string
      readonly endpoint: string
      readonly isLinux: boolean
      readonly isMacOS: boolean
      readonly isWindows: boolean
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

  const daemon = await setupPairedAuvDaemon('auv-js-browser-integration', {
    runners: [neteaseMusicRunner],
  })

  project.provide('auvBrowserDaemon', {
    available: true,
    credential: daemon.credential,
    endpoint: daemon.remoteEndpoint,
    isLinux,
    isMacOS,
    isWindows,
  })

  return daemon.stop
}
