import type { AgentTools, FunctionCall, FunctionTool, ToolOutput } from './agent'
import type { McpServerProcess, McpTool, McpToolResult } from './mcp'

import { readFile } from 'node:fs/promises'

import { errorMessageFrom } from '@moeru/std'

import { McpStdioClient } from './mcp'

/** The AUV MCP tools an episode exposes to the model. */
export interface AuvTools extends AgentTools {
  close: () => void
}

/** Text of one tool output shown to the model; larger results are truncated. */
const maxOutputText = 20_000

/**
 * MCP tools the model may call. The Device session tools lock and unlock OS
 * logins and `op_run` replays compiled app operations; neither belongs in an
 * OSWorld episode.
 */
const exposedTools = new Set(['invoke'])

/**
 * `invoke` fields the eval owns. The episode pins the Device through the
 * server's root `--device-id`, keeps every Run in the episode store, and
 * never dry-runs, so the model does not see these fields.
 */
const evalOwnedFields = ['device_id', 'device_name', 'dry_run', 'store_root']

/**
 * Starts `auv mcp serve` for one paired guest Device and exposes its tools.
 *
 * Call stack:
 *
 * {@link runEpisode}
 *   -> {@link startAuvTools}
 *     -> `auv --device-id <guest> mcp serve` (MCP `tools/list`)
 *   -> {@link runAgent}
 *     -> `AuvTools.call` -> MCP `tools/call invoke`
 *       -> image artifacts read from the episode's AUV store
 *
 * NOTICE: AUV results carry artifact paths, never pixels. Attaching the
 * captured image to the tool output is this eval's choice for a multimodal
 * model; AUV itself stays text-only so non-vision clients pay nothing.
 */
export async function startAuvTools(
  server: Omit<McpServerProcess, 'args'> & { deviceId: string, storeRoot: string },
  signal: AbortSignal,
): Promise<AuvTools> {
  const client = await McpStdioClient.start({ ...server, args: ['--device-id', server.deviceId, 'mcp', 'serve'] }, signal)
  try {
    const definitions = (await client.listTools(signal)).filter(tool => exposedTools.has(tool.name)).map(functionTool)
    return {
      call: async (call, callSignal) => await callTool(client, call, server.storeRoot, callSignal),
      close: () => client.close(),
      definitions,
    }
  }
  catch (error) {
    client.close()
    throw error
  }
}

async function callTool(client: McpStdioClient, call: FunctionCall, storeRoot: string, signal: AbortSignal): Promise<ToolOutput> {
  let args: Record<string, unknown>
  try {
    args = JSON.parse(call.arguments) as Record<string, unknown>
  }
  catch (error) {
    return { images: [], text: `Invalid JSON arguments: ${errorMessageFrom(error) ?? String(error)}` }
  }
  for (const field of evalOwnedFields) {
    delete args[field]
  }
  let result: McpToolResult
  try {
    result = await client.callTool(call.name, { ...args, store_root: storeRoot }, signal)
  }
  catch (error) {
    // Protocol errors (unknown command, invalid params) go back to the model
    // as text, like a failed command, so it can correct its call.
    if (signal.aborted) {
      throw error
    }
    return { images: [], text: errorMessageFrom(error) ?? String(error) }
  }
  return await toolOutput(result)
}

/** Maps one MCP tool definition onto a model function, minus eval-owned fields. */
function functionTool(tool: McpTool): FunctionTool {
  const parameters = structuredClone(tool.inputSchema)
  const properties = parameters.properties as Record<string, unknown> | undefined
  for (const field of evalOwnedFields) {
    delete properties?.[field]
  }
  return { description: tool.description, name: tool.name, parameters }
}

/** Presents an MCP result as text, attaching recorded image artifacts. */
async function toolOutput(result: McpToolResult): Promise<ToolOutput> {
  const structured = result.structuredContent as undefined | {
    artifacts?: Array<{ content_type?: string, file_path?: string }>
    run_id?: string
  }
  const images = await Promise.all((structured?.artifacts ?? [])
    .filter(artifact => artifact.content_type?.startsWith('image/') === true && artifact.file_path != null)
    .map(async artifact => `data:${artifact.content_type};base64,${(await readFile(artifact.file_path!)).toString('base64')}`))
  // Store file paths are host-local and mean nothing to the model.
  const shown = structured == null
    ? (result.content ?? []).map(part => part.text ?? '').join('\n')
    : JSON.stringify({ ...structured, artifacts: structured.artifacts?.map(({ file_path: _path, ...artifact }) => artifact) })
  return {
    images,
    runId: structured?.run_id,
    text: shown.length > maxOutputText ? `${shown.slice(0, maxOutputText)}…[truncated]` : shown,
  }
}
