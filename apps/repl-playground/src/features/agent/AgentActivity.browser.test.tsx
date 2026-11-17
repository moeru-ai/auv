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
    const activity = screen.getByRole('button', { name: /2 activities/ })
    await expect.element(activity).toHaveAttribute('aria-expanded', 'false')
    await expect.element(screen.getByText('1 issue', { exact: true })).toBeVisible()
    await expect.element(screen.getByTitle('Script failed: OverlayService/ShowOverlay is UNIMPLEMENTED', { exact: true })).toBeVisible()
    await activity.click()
    await expect.element(activity).toHaveAttribute('aria-expanded', 'true')
    const failed = screen.getByRole('button', { name: /Playground.*Returned an error/ })
    await expect.element(failed).toHaveAttribute('aria-expanded', 'false')
    await expect.element(screen.getByRole('button', { name: /Read notes.*Finished/ })).toBeVisible()
    await failed.click()
    await expect.element(failed).toHaveAttribute('aria-expanded', 'true')
    const panelId = failed.element().getAttribute('aria-controls')!
    const details = page.elementLocator(document.getElementById(panelId)!)
    await details.getByText('Script', { exact: true }).click()
    await expect.element(details.getByText(script, { exact: true })).toBeVisible()
    await details.getByText('Protocol details', { exact: true }).click()
    await expect.element(details.getByText('ACP status: completed', { exact: true })).toBeVisible()
    await expect.element(details.getByText(/"status": "error"/)).toBeVisible()
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
    await expect.element(screen.getByRole('button', { name: /1 activity/ })).toHaveAttribute('aria-expanded', 'false')
    const thumbnail = screen.getByRole('img', { exact: true, name: 'Screenshot 1' })
    await expect.element(thumbnail).toBeVisible()
    const image = thumbnail.element() as HTMLImageElement
    await image.decode()
    expect(image.naturalWidth).toBeGreaterThan(0)
    const preview = screen.getByRole('button', { exact: true, name: 'View screenshot 1' })
    await preview.click()
    await expect.element(page.getByRole('dialog', { exact: true, name: 'Screenshot 1' })).toBeVisible()
    await userEvent.keyboard('{Escape}')
    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
    await expect.element(preview).toHaveFocus()
  })
})
