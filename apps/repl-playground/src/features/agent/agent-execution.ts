import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/sdk/client/codemode'

import type { RunBackend, RunOutcomeKind } from '../../backend/types'

import { recordCall } from '../../runtime/record-call'
import { nowMs } from '../../store'
import { reserveDevice, useDeviceTarget } from '../device/device-target'
import { agentDesktop, agentWorkspace } from './agent-workspace'
import { createRunnerHost } from './runner-host'

const { actions, store } = agentWorkspace
let nextSourceId = 1
let active: AbortController | undefined

export function resetAgentExecution(): void {
  active?.abort()
  actions.resetSession()
}

export async function runAgent(
  { code, signal }: CodeModeRequest,
  execute: (host: ReturnType<typeof createRunnerHost>, signal: AbortSignal) => Promise<CodemodeResult>,
): Promise<CodemodeResult> {
  signal.throwIfAborted()
  const release = reserveDevice()
  if (!release)
    throw new Error('Another playground execution is running.')

  const reset = new AbortController()
  active = reset
  const cancellation = AbortSignal.any([reset.signal, signal])
  let host: ReturnType<typeof createRunnerHost> | undefined
  const source = { code, id: nextSourceId++ }
  const backend = useDeviceTarget.getState().backend
  let run: null | RunBackend = null
  store.setState({ cursor: null, hoveredCallId: null, run: { currentLine: null, mode: 'live', startedAt: Date.now(), status: 'compiling' }, selectedCallId: null })

  let outcome: RunOutcomeKind = 'failed'
  try {
    const openedRun = await backend?.beginRun() ?? null
    run = openedRun
    const access = openedRun?.sdk()
    if (!reset.signal.aborted)
      actions.patchRun({ runId: access?.route.runId, status: 'running' })
    host = createRunnerHost(access, cancellation, start =>
      recordCall(agentWorkspace, openedRun, start, { hit: 1, line: null, source }, nextSeq, reset.signal))
    let result: CodemodeResult = cancellation.aborted
      ? { calls: [], error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: [] }
      : await execute(host, cancellation)
    if (result.ok && cancellation.aborted)
      result = { calls: result.calls, error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: result.output }
    outcome = cancellation.aborted || (!result.ok && ['aborted', 'timeout'].includes(result.error.kind))
      ? 'canceled'
      : result.ok ? 'succeeded' : 'failed'
    if (!reset.signal.aborted) {
      for (const item of result.output)
        actions.addLog({ at: Date.now(), level: 'show', line: null, seq: nextSeq(), values: [item.type === 'text' ? item.text : 'Image emitted to the agent.'] })
      if (result.ok && result.value !== undefined)
        actions.addLog({ at: Date.now(), label: 'result', level: 'show', line: null, seq: nextSeq(), values: [result.value] })
      actions.patchRun({
        endedAt: Date.now(),
        errorMessage: !result.ok && outcome === 'failed' ? result.error.message : undefined,
        status: outcome === 'succeeded' ? 'done' : outcome === 'canceled' ? 'stopped' : 'error',
      })
    }
    return result
  }
  catch (error) {
    outcome = cancellation.aborted ? 'canceled' : 'failed'
    if (!reset.signal.aborted)
      actions.patchRun({ endedAt: Date.now(), errorMessage: error instanceof Error ? error.message : String(error), status: outcome === 'canceled' ? 'stopped' : 'error' })
    throw error
  }
  finally {
    await host?.close()
    try {
      await run?.endRun(outcome)
    }
    finally {
      active = undefined
      release()
      if (!reset.signal.aborted)
        void agentDesktop.refreshAxTree().catch(() => {})
    }
  }
}

function nextSeq(): number {
  const seq = store.getState().seqAt.length
  actions.appendEvents([], [], [[seq, nowMs()]])
  return seq
}
