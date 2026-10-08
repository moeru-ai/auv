import type { AxNode } from './types'

import { connect, createAuv, discoverRunner, InputPolicy, Position } from '@auv-js/sdk'
import { describe, expect, it } from 'vitest'

import { MockDesktop } from './mock-desktop'
import { MOCK_DEVICE, mockRunner } from './mock-runner'

// NOTICE(mock-runner-node-tests): Node has no OffscreenCanvas, so these tests
// cover the Runner rules and scene state, not captures or recognized text;
// those render on a canvas and are exercised in the browser.

function labels(node: AxNode): string[] {
  return [node.label ?? '', ...node.children.flatMap(labels)].filter(label => label.length > 0)
}

async function mock() {
  const desktop = new MockDesktop()
  const connection = await connect({ transport: mockRunner(desktop) })
  const auv = createAuv(connection)
  return { connection, desktop, runner: auv.runner({ deviceId: MOCK_DEVICE.id, runnerClass: 'auv.core.local' }) }
}

describe('mock Runner', () => {
  it('clicks a window at a window-local point, and the scene reacts', async () => {
    const { connection, desktop, runner } = await mock()
    const counter = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'dev.auv.mock.counter' } })

    const response = await counter.click({ x: 210, y: 198 }) // the Increment button
    expect(response.windowPoint).toMatchObject({ x: 210, y: 198 })
    expect(response.screenPoint).toMatchObject({ x: 1910, y: 418 })
    expect(labels(desktop.accessibilityTree())).toContain('Count: 1')
    await connection.close()
  })

  it('converts a screen position for a window scroll, like the Runner', async () => {
    const { connection, desktop, runner } = await mock()
    const music = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'dev.auv.mock.music' } })
    const before = desktop.accessibilityTree().children.find(window => window.value === 'Music')!.children[1]!.frame!.y

    const response = await music.scroll(Position.screen(1020, 320), { deltaY: 96 })
    expect(response.point).toMatchObject({ x: 100, y: 200 })
    const after = desktop.accessibilityTree().children.find(window => window.value === 'Music')!.children[1]!.frame!.y
    expect(before - after).toBe(96)
    await connection.close()
  })

  it('rejects window input outside the window and window options on a global click', async () => {
    const { connection, runner } = await mock()
    const counter = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'dev.auv.mock.counter' } })

    await expect(counter.click({ x: 5000, y: 5 })).rejects.toMatchObject({ rpcCode: 3 })
    await expect(runner.input.click({ x: 10, y: 10 }, { policy: InputPolicy.BACKGROUND_ONLY })).rejects.toMatchObject({ rpcCode: 3 })
    await expect(runner.input.click({ x: 10, y: 10 })).resolves.toMatchObject({ screenPoint: { x: 10, y: 10 } })
    await connection.close()
  })

  it('types into the focused field through window keyboard input', async () => {
    const { connection, desktop, runner } = await mock()
    const todos = await runner.windows.resolve({ application: { case: 'applicationBundleId', value: 'dev.auv.mock.todos' } })

    await todos.click({ x: 100, y: 74 }) // the "Add a todo" field
    await todos.typeText('Write the mock Runner')
    await todos.pressKeys(['return'])
    expect(labels(desktop.accessibilityTree())).toContain('Write the mock Runner')
    await connection.close()
  })

  it('looks like a device to discovery and Run creation', async () => {
    const { connection } = await mock()
    const auv = createAuv(connection)

    expect((await auv.devices.list()).map(device => device.id)).toEqual([MOCK_DEVICE.id])
    const run = await auv.runs.create({ deviceIds: [MOCK_DEVICE.id] })
    expect(run.phase).toBe('running')
    const discovered = await discoverRunner(connection, { runnerClass: 'auv.core.local' })
    expect(discovered.apis.find(api => api.method === 'ClickPoint')?.presentation?.name).toBe('input.click')
    await connection.close()
  })
})
