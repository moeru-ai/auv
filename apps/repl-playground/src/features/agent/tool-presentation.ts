import type { AcpToolCallRecord } from '@hafbit/react-acp'

const kindTitles = {
  delete: 'Delete',
  edit: 'Edit',
  execute: 'Run command',
  fetch: 'Fetch',
  move: 'Move',
  other: 'Use tool',
  read: 'Read',
  search: 'Search',
  switch_mode: 'Switch mode',
  think: 'Think',
}

/** Keep strings readable and retain full JSON values in execution details. */
export function formatValue(value: unknown): string {
  return typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? ''
}

/** Execution errors come from ACP or structured agent metadata, never output text. */
export function presentTool(tool: AcpToolCallRecord['value']) {
  const raw = record(tool.rawOutput)
  const metadata = record(raw.metadata)
  const nested = Array.isArray(metadata.toolCalls) ? metadata.toolCalls.map(record) : []
  // NOTICE(opencode-execute): OpenCode can complete its execute wrapper while
  // returning an error. This matches its desktop current-tool-state.ts at
  // `anomalyco/opencode@32b57015`, using metadata.error and toolCalls[].status.
  const error = tool.status === 'failed' || typeof raw.error === 'string' || metadata.error === true || nested.some(call => call.status === 'error')
  const running = tool.status === 'pending' || tool.status === 'in_progress'
  const names = [...new Set(nested.flatMap(call => typeof call.tool === 'string' ? [call.tool === 'auv-playground.execute' ? 'Playground' : call.tool] : []))]
  const title = names.length
    ? names.join(', ')
    : tool.title === 'execute' ? 'Run script' : tool.title || kindTitles[tool.kind ?? 'other']
  const output = tool.content?.flatMap(part => part.type === 'content' && part.content.type === 'text' ? [part.content.text] : []).join('\n')
  const code = record(tool.rawInput).code
  return {
    error,
    // NOTICE(activity-titles): titles describe reported tools, not inferred
    // task success. Semantic step names need an approved agent/run association.
    errorText: error ? output || (typeof raw.error === 'string' ? raw.error : 'A tool returned an error. Open the activity for details.') : undefined,
    running,
    // Show the original script without the enclosing JSON string's escaped newlines.
    script: typeof code === 'string' ? code : undefined,
    status: running ? 'running' : error ? 'error' : 'finished',
    title,
  }
}

/** Decode only the open ACP fields used for presentation, without changing the record. */
function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}
