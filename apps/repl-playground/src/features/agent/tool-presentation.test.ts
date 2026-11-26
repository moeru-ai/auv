import { describe, expect, it } from 'vitest'

import { presentTool } from './tool-presentation'

describe('tool presentation', () => {
  it('shows completed calls with error details in their output as finished', () => {
    expect(presentTool({
      content: [{ content: { text: 'Script failed', type: 'text' }, type: 'content' }],
      rawInput: { code: 'text(api.help());' },
      rawOutput: { error: 'Script failed', metadata: { error: true, toolCalls: [{ status: 'error', tool: 'inner' }] } },
      status: 'completed',
      title: 'execute',
      toolCallId: 'wrapper',
    })).toMatchObject({ error: false, errorText: undefined, running: false, script: 'text(api.help());', title: 'Run script' })
  })

  it.each([undefined, 'pending', 'in_progress'] as const)('shows an active call as running (status %s)', (status) => {
    expect(presentTool({
      status,
      title: 'Read notes',
      toolCallId: 'read',
    })).toMatchObject({ error: false, running: true })
  })
})
