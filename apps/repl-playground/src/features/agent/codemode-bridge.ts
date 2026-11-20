import type { ServedMcp } from '@exterioragentprotocol/sdk/client'
import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/sdk/client/codemode'
import type { AcpClientConnection } from '@hafbit/react-acp/core'

// oxlint-disable-next-line import/default -- Vite exposes its worker constructor as the default export.
import CodeModeWorker from '@exterioragentprotocol/sdk/client/codemode/worker?worker'

import { createMcpApp } from '@exterioragentprotocol/sdk/client'
import { createBrowserCodeModeSandbox, registerCodeMode } from '@exterioragentprotocol/sdk/client/codemode'

import { version } from '../../../package.json'
import { useDeviceTarget } from '../device/device-target'
import { runAgent } from './agent-execution'

const description = 'Run JavaScript with the AUV SDK (`auv`) on the selected Device or mock desktop. Read the `auv-playground` skill for SDK usage.'

/** Connects tool calls to the selected Device for the active ACP conversation. */
export class CodeModeHost {
  #active?: AbortController
  #sessionId?: string
  #stopWatchingDevice?: () => void
  #store: Record<string, unknown> = {}

  cancel(): void {
    this.#active?.abort()
  }

  close(): void {
    this.setSession(undefined)
  }

  async createServerFactory() {
    const { skills } = await import('./skills')
    return createMcpApp({
      configure: server => registerCodeMode(server, { description, run: request => this.#execute(request), tools: [] }),
      name: 'auv-playground',
      skills,
      version,
    })
  }

  setSession(sessionId: string | undefined): void {
    if (sessionId === this.#sessionId)
      return
    this.cancel()
    this.#stopWatchingDevice?.()
    this.#sessionId = sessionId
    this.#store = {}
    this.#stopWatchingDevice = sessionId === undefined
      ? undefined
      : useDeviceTarget.subscribe((next, previous) => {
          if (next.backend !== previous.backend) {
            this.cancel()
            this.#store = {}
          }
        })
  }

  async #execute(request: CodeModeRequest): Promise<CodemodeResult> {
    if (this.#sessionId === undefined)
      throw new Error('This ACP session is not active in the playground.')
    if (this.#active)
      throw new Error('Another playground execution is running.')
    const active = new AbortController()
    this.#active = active
    const signal = AbortSignal.any([active.signal, request.signal])
    try {
      const result = await runAgent({ ...request, signal }, async ({ globals }, runSignal) => {
        const { default: sdkSource } = await import('virtual:auv-agent-sdk')
        const sandbox = createBrowserCodeModeSandbox({
          createWorker: () => new CodeModeWorker(),
          globals,
          memoryLimitBytes: 64 * 1024 * 1024,
          tools: [],
        })
        try {
          return await sandbox.run({
            ...request,
            code: `${sdkSource}\nconst { auv } = sdk;\nreturn await (async () => {\n${request.code}\n})();`,
            signal: runSignal,
            store: this.#store,
          })
        }
        finally {
          sandbox.close()
        }
      })
      if (result.ok && signal.aborted)
        return { calls: result.calls, error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: result.output }
      if (result.ok) {
        const next = { ...this.#store, ...result.storeWrites.set }
        for (const key of result.storeWrites.delete)
          delete next[key]
        this.#store = next
      }
      return result
    }
    finally {
      this.#active = undefined
    }
  }
}

/**
 * Adds the Playground MCP endpoint to the typed ACP connection.
 * This adapter owns session setup and endpoint cleanup; the SDK owns messages.
 */
export function codeModeConnection(connection: AcpClientConnection, host: CodeModeHost, endpoint: Pick<ServedMcp, 'close' | 'headers' | 'url'>): AcpClientConnection {
  const close = () => {
    host.close()
    endpoint.close()
  }
  if (connection.signal.aborted)
    close()
  else connection.signal.addEventListener('abort', close, { once: true })
  return {
    ...connection,
    cancel(sessionId) {
      host.cancel()
      return connection.cancel(sessionId)
    },
    newSession(request) {
      host.setSession(undefined)
      const { headers, url } = endpoint
      return connection.newSession({ ...request, mcpServers: [...request.mcpServers, { headers, name: 'auv-playground', type: 'http', url }] })
    },
  }
}
