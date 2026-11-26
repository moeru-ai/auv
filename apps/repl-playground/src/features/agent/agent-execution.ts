import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/sdk/client/codemode'

import type { RunOutcomeKind } from '../../backend/types'

import { recordCall } from '../../runtime/record-call'
import { nowMs } from '../../store'
import { reserveDevice, withDeviceRun } from '../device/device-target'
import { agentDesktop, agentWorkspace } from './agent-workspace'
import { createRunnerHost } from './runner-host'

const { actions, store } = agentWorkspace
const runStatus = { canceled: 'stopped', failed: 'error', succeeded: 'done' } as const
let nextSourceId = 1
let active: AbortController | undefined

export function cancelSuccessfulResult(result: CodemodeResult, signal: AbortSignal): CodemodeResult {
  return result.ok && signal.aborted ? canceledResult(result) : result
}

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
  const source = { code, id: nextSourceId++ }

  try {
    actions.beginRun('live')
    const result = await withDeviceRun(cancellation, async (run) => {
      const access = run?.sdk()
      if (!reset.signal.aborted)
        actions.patchRun({ runId: access?.route.runId, status: 'running' })
      const host = createRunnerHost(access, cancellation, start =>
        recordCall(agentWorkspace, run, start, { hit: 1, line: null, source }, nextSeq, reset.signal))
      try {
        const result = cancellation.aborted
          ? canceledResult()
          : cancelSuccessfulResult(await execute(host, cancellation), cancellation)
        const outcome = executionOutcome(result, cancellation)
        if (!reset.signal.aborted)
          publishExecution(result, outcome)
        return { outcome, value: result }
      }
      finally {
        await host.close()
      }
    })
    return cancelSuccessfulResult(result, cancellation)
  }
  catch (error) {
    const outcome = cancellation.aborted ? 'canceled' : 'failed'
    if (!reset.signal.aborted)
      actions.patchRun({ endedAt: Date.now(), errorMessage: error instanceof Error ? error.message : String(error), status: runStatus[outcome] })
    throw error
  }
  finally {
    active = undefined
    release()
    if (!reset.signal.aborted)
      void agentDesktop.refreshAxTree().catch(() => {})
  }
}

function canceledResult({ calls, output }: Pick<CodemodeResult, 'calls' | 'output'> = { calls: [], output: [] }): CodemodeResult {
  return { calls, error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output }
}

function executionOutcome(result: CodemodeResult, signal: AbortSignal): RunOutcomeKind {
  if (signal.aborted)
    return 'canceled'
  if (result.ok)
    return 'succeeded'
  return result.error.kind === 'aborted' || result.error.kind === 'timeout' ? 'canceled' : 'failed'
}

function nextSeq(): number {
  const seq = store.getState().seqAt.length
  actions.appendEvents([], [], [[seq, nowMs()]])
  return seq
}

function publishExecution(result: CodemodeResult, outcome: RunOutcomeKind): void {
  for (const item of result.output)
    actions.addLog({ at: Date.now(), level: 'show', line: null, seq: nextSeq(), values: [item.type === 'text' ? item.text : 'Image emitted to the agent.'] })
  if (result.ok && result.value !== undefined)
    actions.addLog({ at: Date.now(), label: 'result', level: 'show', line: null, seq: nextSeq(), values: [result.value] })
  actions.patchRun({
    endedAt: Date.now(),
    errorMessage: !result.ok && outcome === 'failed' ? result.error.message : undefined,
    status: runStatus[outcome],
  })
}
