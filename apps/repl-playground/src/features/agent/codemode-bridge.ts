import type { ServedMcp } from '@exterioragentprotocol/sdk/client'
import type { AcpClientConnection } from '@hafbit/react-acp/core'
import type { Transport } from '@modelcontextprotocol/sdk/shared/transport.js'
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js'

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { CallToolRequestSchema, ErrorCode, ListToolsRequestSchema, McpError } from '@modelcontextprotocol/sdk/types.js'

import { version } from '../../../package.json'
import { codeModeLimits } from '../../runtime/codemode-limits'
import { session } from '../../runtime/session'

const tool = {
  description: [
    'Execute TypeScript in AUV Playground against its selected Device or mock desktop. The agent\'s local files and shell cannot reach this environment.',
    'Start with text(api.help()). Find exact SDK methods with api.search({query:"window text"}) and api.describe({path:"WindowClient.findText"}). Output their results with text().',
    'Use auv for SDK calls, sdk for constants and geometry, text(value) or return for output, and await image(capture) for bounded screenshots.',
    'Calls share one mutable desktop. Call sequentially, including help queries, and await every device call.',
    'Each call has fresh variables. Use store(key, value) and load(key) for small JSON values committed only after success. Save IDs, not SDK handles or pixels.',
    'SDK arguments must be data. Callbacks, async iterators, and editor-only helpers are unavailable. Read help for exact types and examples.',
    'Failed or canceled scripts do not undo delivered input. Verify the requested application state after execution.',
  ].join('\n'),
  inputSchema: {
    additionalProperties: false,
    properties: {
      code: { description: 'Raw TypeScript, with top-level await and return.', maxLength: 100000, minLength: 1, type: 'string' },
      timeoutMs: { default: codeModeLimits.defaultTimeoutMs, maximum: codeModeLimits.maxTimeoutMs, minimum: 1, type: 'integer' },
    },
    required: ['code'],
    type: 'object',
  },
  name: 'execute',
}

/** Owns one ACP session's JSON state and the currently executing tool call. */
export class CodeModeHost {
  #active?: AbortController
  #sessionId?: string
  #store: Record<string, unknown> = {}

  cancel(): void {
    this.#active?.abort()
  }

  /** Each Relay MCP session gets its own protocol connection to this execution host. */
  async connect(transport: Transport): Promise<void> {
    const server = new Server({ name: 'auv-playground', version }, { capabilities: { tools: {} } })
    server.setRequestHandler(ListToolsRequestSchema, () => ({ tools: [tool] }))
    server.setRequestHandler(CallToolRequestSchema, async (request, { signal }) => {
      if (request.params.name !== tool.name)
        throw new McpError(ErrorCode.InvalidParams, `Unknown tool: ${request.params.name}`)
      // TODO(mcp-progress): operation-scoped notifications belong to an
      // approved streaming/progress tool slice. Return one final result.
      return this.#execute(request.params.arguments, signal)
    })
    await server.connect(transport)
  }

  setSession(sessionId: string | undefined): void {
    if (sessionId === this.#sessionId)
      return
    this.cancel()
    this.#sessionId = sessionId
    this.#store = {}
  }

  async #execute(params: Record<string, unknown> | undefined, requestSignal: AbortSignal): Promise<CallToolResult> {
    const error = (text: string): CallToolResult => ({ content: [{ text, type: 'text' }], isError: true })
    if (!this.#sessionId)
      return error('This ACP session is not active in the playground.')
    // NOTICE(codemode-serialization): calls share one mutable desktop. Parallel
    // execution needs an owner-approved ordering contract for delivered input.
    if (this.#active || session.running)
      return error('Another playground execution is running.')
    const code = params?.code
    const timeoutMs = params?.timeoutMs ?? codeModeLimits.defaultTimeoutMs
    if (typeof code !== 'string' || !code.trim() || code.length > 100000
      || typeof timeoutMs !== 'number' || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > codeModeLimits.maxTimeoutMs) {
      return error(`execute requires code and a timeoutMs from 1 to ${codeModeLimits.maxTimeoutMs}.`)
    }
    const active = new AbortController()
    this.#active = active
    // Each MCP request carries its own signal, including across transports.
    const signal = AbortSignal.any([active.signal, requestSignal])
    try {
      signal.throwIfAborted()
      const outcome = await session.run({
        codemode: { store: this.#store, timeoutMs },
        mode: 'run',
        signal,
        source: code,
        startLine: 1,
      })
      const content = [...(outcome.codemode?.content ?? [])]
      const succeeded = outcome.status === 'ok' && !signal.aborted
      if (succeeded && outcome.codemode)
        this.#store = outcome.codemode.store
      // TODO(codemode-result-metadata): typed MCP outcomes require an approved
      // result contract; this interface exposes content and isError.
      let text = 'Script canceled. Delivered inputs are not undone.'
      if (succeeded)
        text = 'Script completed. This confirms execution only; verify the requested external state separately.'
      else if (outcome.status === 'error')
        text = `Script failed${outcome.line === undefined ? '' : ` at line ${outcome.line}`}: ${outcome.message}`
      else if (outcome.status === 'stopped' && outcome.reason === 'timeout')
        text = `Script timed out after ${timeoutMs} ms. Delivered inputs are not undone.`
      content.push({ text, type: 'text' })
      return { content, isError: !succeeded }
    }
    catch (cause) { return error(cause instanceof Error ? cause.message : String(cause)) }
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
    host.cancel()
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
