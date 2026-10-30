import { describe, expect, it } from 'vitest'

import { presentTool } from './tool-presentation'

describe('tool presentation', () => {
  it('uses structured error metadata when an execute wrapper completes', () => {
    const tool = {
      content: [{ content: { text: 'ReferenceError: Unknown identifier \'text\'.', type: 'text' as const }, type: 'content' as const }],
      rawInput: { code: 'text(api.help());' },
      rawOutput: { metadata: { error: true } },
      status: 'completed' as const,
      title: 'execute',
      toolCallId: 'code-failure',
    }
    expect(presentTool(tool)).toMatchObject({ error: true, errorText: 'ReferenceError: Unknown identifier \'text\'.', script: 'text(api.help());', status: 'error', title: 'Run script' })
    expect(tool.status).toBe('completed')
  })

  it('does not interpret ordinary output text as an execution failure', () => {
    expect(presentTool({
      content: [{ content: { text: '0 errors found.', type: 'text' }, type: 'content' }],
      status: 'completed',
      toolCallId: 'notes',
    })).toMatchObject({ error: false, errorText: undefined, status: 'finished' })
  })
})
