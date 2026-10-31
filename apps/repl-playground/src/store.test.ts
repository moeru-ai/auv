import type { ScriptSource } from './store'

import { afterEach, describe, expect, it } from 'vitest'

import { actions, usePlayground } from './store'

afterEach(() => actions.resetSession())

describe('execution history', () => {
  // ROOT CAUSE: codemode used the editor's per-run clear, so a later query
  // erased SDK calls and hid earlier captures even though device state persisted.
  it('retains calls and event positions when another script continues the timeline', () => {
    const script: ScriptSource = { id: 1, kind: 'codemode', source: 'await auv.windows.list()', startLine: 1 }
    actions.beginRun('live', script, false)
    const id = actions.beginCall({ args: [], effect: 'read', hit: 1, line: 1, method: 'windows.list', script, seq: 0, startedAt: 100 })
    actions.appendEvents([], [], [[0, 100]])
    const timelineId = usePlayground.getState().timelineId

    actions.beginRun('live', { id: 2, kind: 'codemode', source: 'text(api.help())', startLine: 1 }, true)

    expect(usePlayground.getState().timelineId).toBe(timelineId)
    expect(usePlayground.getState().seqAt).toEqual([100])
    expect(usePlayground.getState().calls).toMatchObject([{ id, script }])
  })

  it('starts an independent timeline for an editor run and clears it on reset', () => {
    const script: ScriptSource = { id: 1, kind: 'editor', source: '1', startLine: 1 }
    actions.beginRun('live', script, false)
    actions.appendEvents([], [], [[0, 100]])
    const timelineId = usePlayground.getState().timelineId

    actions.beginRun('live', { ...script, id: 2 }, false)

    expect(usePlayground.getState().timelineId).toBe(timelineId + 1)
    expect(usePlayground.getState().seqAt).toEqual([])
    actions.resetSession()
    expect(usePlayground.getState().run).toMatchObject({ status: 'idle' })
    expect(usePlayground.getState().run.script).toBeUndefined()
    expect(usePlayground.getState().calls).toEqual([])
  })
})
