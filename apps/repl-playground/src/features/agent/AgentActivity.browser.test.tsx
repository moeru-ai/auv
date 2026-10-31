import { describe, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'

import { mountAgentThread } from '../../../agent/thread.fixture'

describe('agent activity', () => {
  // ROOT CAUSE: OpenCode completed its execute wrapper while a nested tool
  // failed. The view hid the failure and original script behind that status.
  it('shows nested execution failures and preserves the original protocol details', async () => {
    const script = 'return await tools["auv-playground"].execute({ code: "await auv.overlay.show({});" });'
    const container = await mountAgentThread([{
      content: [{ content: { text: 'Script failed: OverlayService/ShowOverlay is UNIMPLEMENTED', type: 'text' }, type: 'content' }],
      rawInput: { code: script },
      rawOutput: { metadata: { toolCalls: [{ status: 'error', tool: 'auv-playground.execute' }] } },
      sessionUpdate: 'tool_call',
      status: 'completed',
      title: 'execute',
      toolCallId: 'nested-failure',
    }, {
      content: [{ content: { text: '0 errors found.', type: 'text' }, type: 'content' }],
      sessionUpdate: 'tool_call',
      status: 'completed',
      title: 'Read notes',
      toolCallId: 'neutral-result',
    }])
    const screen = page.elementLocator(container)
    await expect.element(screen.getByText('2 activities', { exact: true })).toBeVisible()
    await expect.element(screen.getByText('1 issue', { exact: true })).toBeVisible()
    await expect.element(screen.getByTitle('Script failed: OverlayService/ShowOverlay is UNIMPLEMENTED', { exact: true })).toBeVisible()
    const activity = container.querySelector('[data-agent-activity]')!
    await page.elementLocator(activity.querySelector('summary')!).click()
    const tool = container.querySelector('[data-tool-id="nested-failure"]')!
    expect(tool.getAttribute('data-tool-status')).toBe('error')
    expect(container.querySelector('[data-tool-id="neutral-result"]')?.getAttribute('data-tool-status')).toBe('finished')
    const details = page.elementLocator(tool)
    await page.elementLocator(tool.querySelector('summary')!).click()
    await details.getByText('Script', { exact: true }).click()
    await expect.element(page.elementLocator(tool.querySelector('[data-tool-script]')!)).toHaveTextContent(script)
    await details.getByText('Protocol details', { exact: true }).click()
    await expect.element(details.getByText('ACP status: completed', { exact: true })).toBeVisible()
    expect(tool.textContent).toMatch(/"status": "error"/)
  })

  it('decodes a returned image and closes its preview with Escape', async () => {
    const data = document.createElement('canvas').toDataURL('image/png').split(',')[1]
    const container = await mountAgentThread([{
      content: [{ content: { data, mimeType: 'image/png', type: 'image' }, type: 'content' }],
      sessionUpdate: 'tool_call',
      status: 'completed',
      title: 'Capture result',
      toolCallId: 'image-result',
    }])
    const screen = page.elementLocator(container)
    const thumbnail = screen.getByRole('img', { exact: true, name: 'Screenshot 1' })
    await expect.element(thumbnail).toBeVisible()
    const image = thumbnail.element() as HTMLImageElement
    await image.decode()
    expect(image.naturalWidth).toBeGreaterThan(0)
    await screen.getByRole('button', { exact: true, name: 'View screenshot 1' }).click()
    await expect.element(page.getByRole('dialog', { exact: true, name: 'Screenshot 1' })).toBeVisible()
    await userEvent.keyboard('{Escape}')
    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
  })
})
