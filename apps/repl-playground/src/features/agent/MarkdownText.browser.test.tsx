import { describe, expect, it } from 'vitest'
import { page } from 'vitest/browser'

import { mountAgentThread } from '../../../agent/thread.fixture'

describe('agent Markdown', () => {
  it('renders formatted Agent messages with GFM and code blocks', async () => {
    const container = await mountAgentThread([{
      content: { text: '| Check | Result |\n| --- | --- |\n| Counter | Passed |\n\n- [x] Verified\n\n~~Outdated~~\n\n> Checked with a screenshot.\n\n[Documentation](https://example.com/docs)\n\n```js\ntext("verified");\n```', type: 'text' },
      sessionUpdate: 'agent_message_chunk',
    }])
    const screen = page.elementLocator(container)
    await expect.element(screen.getByRole('cell', { exact: true, name: 'Passed' })).toBeVisible()
    expect([...container.querySelectorAll('th')].map(header => header.textContent)).toEqual(['Check', 'Result'])
    await expect.element(screen.getByRole('checkbox')).toBeChecked()
    expect(container.querySelector('del')?.textContent).toBe('Outdated')
    expect(container.querySelector('blockquote')?.textContent?.trim()).toBe('Checked with a screenshot.')
    await expect.element(screen.getByRole('link', { name: 'Documentation' })).toHaveAttribute('href', 'https://example.com/docs')
    expect(container.querySelector('pre code')?.textContent?.trim()).toBe('text("verified");')
  })
})
