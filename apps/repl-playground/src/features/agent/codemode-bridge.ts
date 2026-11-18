import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/client-sdk/codemode'
import type { ServedMcp } from '@exterioragentprotocol/sdk/client'
import type { AcpClientConnection } from '@hafbit/react-acp/core'

// oxlint-disable-next-line import/default -- Vite exposes its worker constructor as the default export.
import CodeModeWorker from '@exterioragentprotocol/client-sdk/codemode/worker?worker'

import { connectCodeMode, createBrowserCodeModeSandbox } from '@exterioragentprotocol/client-sdk/codemode'

import { version } from '../../../package.json'
import { useDeviceTarget } from '../device/device-target'
import { runAgent } from './agent-execution'
import { codeModeToolDescriptions, createCodeModeTools } from './codemode-tools'

const description = [
  'Run JavaScript in AUV Playground against its selected Device or mock desktop.',
  'Use tools.list_windows({}) or tools.resolve_window({ appName }) to get a windowId. Then use tools.find_window_text({ windowId, query }) to identify UI before acting.',
  'Use image(await tools.get_capture_image({ captureId })) to see a bounded preview. Use store/load to keep small JSON values between calls on this Device.',
  'Execute scripts sequentially and await every device call. Failed or canceled scripts leave delivered input in effect.',
].join('\n')

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

  /** Each Relay MCP session gets its own protocol connection to this execution host. */
  connect(transport: Parameters<typeof connectCodeMode>[0]): Promise<void> {
    return connectCodeMode(transport, {
      description,
      name: 'auv-playground',
      run: request => this.#execute(request),
      tools: codeModeToolDescriptions,
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
      const result = await runAgent({ ...request, signal }, async (runner, runSignal) => {
        let pending = Promise.resolve()
        const tools = createCodeModeTools(() => {
          if (!runner)
            throw new Error('AUV tools need a Device or the mock desktop.')
          return runner
        }).map(tool => ({
          ...tool,
          execute(args: unknown, context: Parameters<typeof tool.execute>[1]) {
            // One mutable desktop. Even scripts that use Promise.all deliver
            // calls in issue order and each queued call checks cancellation.
            const call = pending.then(() => {
              const signal = AbortSignal.any([runSignal, context.signal])
              signal.throwIfAborted()
              return tool.execute(args, { ...context, signal })
            })
            pending = call.then(() => {}, () => {})
            return call
          },
        }))
        const sandbox = createBrowserCodeModeSandbox({
          createWorker: () => new CodeModeWorker(),
          memoryLimitBytes: 64 * 1024 * 1024,
          tools,
        })
        try {
          return await sandbox.execute(request.code, { signal: runSignal, store: this.#store, timeoutMs: request.timeoutMs })
        }
        finally {
          sandbox.close()
          await pending
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
export function codeModeConnection(connection: AcpClientConnection, host: CodeModeHost, tools: ServedMcp): AcpClientConnection {
  const close = () => {
    host.close()
    tools.close()
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
      return connection.newSession({ ...request, mcpServers: [...request.mcpServers, { headers: tools.headers, name: 'auv-playground', type: 'http', url: tools.url }] })
    },
  }
}
