import { describe, expect, it } from 'vitest'
import { page } from 'vitest/browser'

import { mountAgentThread } from '../../../agent/thread.fixture'

describe('agent Markdown', () => {
  it('renders GFM tables, task lists, links and quoted text', async () => {
    const container = await mountAgentThread([{
      content: { text: '| Check | Result |\n| --- | --- |\n| Counter | Passed |\n\n- [x] Verified\n\n~~Outdated~~\n\n> Checked with a screenshot.\n\n[Documentation](https://example.com/docs)', type: 'text' },
      sessionUpdate: 'agent_message_chunk',
    }])
    const screen = page.elementLocator(container)
    await expect.element(screen.getByRole('cell', { exact: true, name: 'Passed' })).toBeVisible()
    expect([...container.querySelectorAll('th')].map(header => header.textContent)).toEqual(['Check', 'Result'])
    await expect.element(screen.getByRole('checkbox')).toBeChecked()
    expect(container.querySelector('del')?.textContent).toBe('Outdated')
    expect(container.querySelector('blockquote')?.textContent?.trim()).toBe('Checked with a screenshot.')
    await expect.element(screen.getByRole('link', { name: 'Documentation' })).toHaveAttribute('href', 'https://example.com/docs')
  })

  it('copies the original fenced code to the browser clipboard', async () => {
    const container = await mountAgentThread([{
      content: { text: '```ts\ntext("verified");\n```', type: 'text' },
      sessionUpdate: 'agent_message_chunk',
    }])
    const screen = page.elementLocator(container)
    await expect.poll(() => container.querySelector('pre code')?.textContent).toBe('text("verified");\n')
    await screen.getByRole('button', { exact: true, name: 'Copy code' }).click()
    await expect.element(screen.getByRole('button', { exact: true, name: 'Copied' })).toBeVisible()
    // The Windows clipboard normalizes LF to CRLF.
    expect((await navigator.clipboard.readText()).replaceAll('\r\n', '\n')).toBe('text("verified");\n')
  })
})
