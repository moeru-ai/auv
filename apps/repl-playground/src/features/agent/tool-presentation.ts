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

export function presentTool(tool: AcpToolCallRecord['value']) {
  const status = tool.status ?? 'pending'
  const error = status === 'failed'
  const output = tool.content?.flatMap(part => part.type === 'content' && part.content.type === 'text' ? [part.content.text] : []).join('\n')
  const code = record(tool.rawInput).code
  return {
    error,
    errorText: error ? output || 'A tool returned an error. Open the activity for details.' : undefined,
    running: status === 'pending' || status === 'in_progress',
    // Show the original script without the enclosing JSON string's escaped newlines.
    script: typeof code === 'string' ? code : undefined,
    title: tool.title === 'execute' ? 'Run script' : tool.title || kindTitles[tool.kind ?? 'other'],
  }
}

/** Decode only the open ACP fields used for presentation, without changing the record. */
function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}
