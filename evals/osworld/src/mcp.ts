import type { ChildProcessWithoutNullStreams } from 'node:child_process'

import process from 'node:process'

import { spawn } from 'node:child_process'
import { createWriteStream } from 'node:fs'
import { createInterface } from 'node:readline'

import { writeExecutionLog } from './executionLog'

export interface McpServerProcess {
  /** Executable plus arguments, for example `auv --device-id <id> mcp serve`. */
  args: string[]
  /** Episode identity for the execution log. */
  context: { caseId: string, podName: string }
  env?: NodeJS.ProcessEnv
  executable: string
  /** File that receives the server's stderr. */
  stderrFile: string
}

/** One tool from MCP `tools/list`. */
export interface McpTool {
  description?: string
  inputSchema: Record<string, unknown>
  name: string
}

/** MCP `tools/call` result content. */
export interface McpToolResult {
  content?: Array<{ data?: string, mimeType?: string, text?: string, type: string }>
  isError?: boolean
  structuredContent?: unknown
}

/**
 * A minimal MCP client over stdio: newline-delimited JSON-RPC 2.0.
 *
 * NOTICE: The eval needs only `initialize`, `tools/list`, and `tools/call`.
 * The official SDK requires `zod` as a peer, and adding it re-resolved other
 * workspace packages' zod peers to an unreleased-age version. Switch to the
 * SDK when the workspace already carries a compatible zod.
 */
export class McpStdioClient {
  private readonly child: ChildProcessWithoutNullStreams
  private exited: Error | undefined
  private nextId = 1
  private readonly pending = new Map<number, { reject: (error: Error) => void, resolve: (result: unknown) => void }>()

  private constructor(private readonly server: McpServerProcess) {
    this.child = spawn(server.executable, server.args, { env: server.env ?? process.env, stdio: ['pipe', 'pipe', 'pipe'] })
    this.child.stderr.pipe(createWriteStream(server.stderrFile))
    createInterface({ input: this.child.stdout }).on('line', line => this.receive(line))
    const fail = (error: Error) => {
      this.exited ??= error
      for (const { reject } of this.pending.values()) {
        reject(error)
      }
      this.pending.clear()
    }
    this.child.on('error', fail)
    this.child.on('exit', (code, signal) => fail(new Error(`MCP server exited (code ${String(code)}, signal ${String(signal)})`)))
    writeExecutionLog('osworld:mcp', 'log', 'MCP server started', { ...server.context, args: server.args, executable: server.executable })
  }

  /** Starts the server and completes the MCP initialize handshake. */
  static async start(server: McpServerProcess, signal: AbortSignal): Promise<McpStdioClient> {
    const client = new McpStdioClient(server)
    try {
      await client.request('initialize', {
        capabilities: {},
        clientInfo: { name: 'auv-osworld-eval', version: '0' },
        protocolVersion: '2025-06-18',
      }, signal)
      client.notify('notifications/initialized')
      return client
    }
    catch (error) {
      client.close()
      throw error
    }
  }

  async callTool(name: string, args: Record<string, unknown>, signal: AbortSignal): Promise<McpToolResult> {
    return await this.request('tools/call', { arguments: args, name }, signal) as McpToolResult
  }

  close(): void {
    this.child.kill('SIGTERM')
    writeExecutionLog('osworld:mcp', 'log', 'MCP server stopped', this.server.context)
  }

  async listTools(signal: AbortSignal): Promise<McpTool[]> {
    const result = await this.request('tools/list', {}, signal) as { tools: McpTool[] }
    return result.tools
  }

  private notify(method: string): void {
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method })}\n`)
  }

  private receive(line: string): void {
    if (line.trim() === '') {
      return
    }
    const message = JSON.parse(line) as { error?: { message: string }, id?: number, result?: unknown }
    const waiter = message.id == null ? undefined : this.pending.get(message.id)
    if (waiter == null || message.id == null) {
      return
    }
    this.pending.delete(message.id)
    if (message.error != null) {
      waiter.reject(new Error(`MCP error: ${message.error.message}`))
      return
    }
    waiter.resolve(message.result)
  }

  private async request(method: string, params: unknown, signal: AbortSignal): Promise<unknown> {
    if (this.exited != null) {
      throw this.exited
    }
    signal.throwIfAborted()
    const id = this.nextId++
    return await new Promise((resolve, reject) => {
      const onAbort = () => {
        this.pending.delete(id)
        reject(signal.reason)
      }
      signal.addEventListener('abort', onAbort, { once: true })
      this.pending.set(id, {
        reject: (error) => {
          signal.removeEventListener('abort', onAbort)
          reject(error)
        },
        resolve: (result) => {
          signal.removeEventListener('abort', onAbort)
          resolve(result)
        },
      })
      this.child.stdin.write(`${JSON.stringify({ id, jsonrpc: '2.0', method, params })}\n`)
    })
  }
}
