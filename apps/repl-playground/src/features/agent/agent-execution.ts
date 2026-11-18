import type { RunnerClient } from '@auv-js/sdk'
import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/client-sdk/codemode'

import type { RunOutcomeKind } from '../../backend/types'

import { AuvConnection, createAuv } from '@auv-js/sdk'

import { RecordingBackend } from '../../backend/replay'
import { recordCall } from '../../runtime/record-call'
import { nowMs } from '../../store'
import { reserveDevice, useDeviceTarget } from '../device/device-target'
import { agentDesktop, agentWorkspace } from './agent-workspace'

const { actions, store } = agentWorkspace
let nextSourceId = 1
let active: AbortController | undefined

export function resetAgentExecution(): void {
  active?.abort()
  actions.resetSession()
}

export async function runAgent(
  { code, signal }: CodeModeRequest,
  execute: (runner: RunnerClient | undefined, signal: AbortSignal) => Promise<CodemodeResult>,
): Promise<CodemodeResult> {
  signal.throwIfAborted()
  const release = reserveDevice()
  if (!release)
    throw new Error('Another playground execution is running.')

  const reset = new AbortController()
  active = reset
  const cancellation = AbortSignal.any([reset.signal, signal])
  const callLifetime = new AbortController()
  const source = { code, id: nextSourceId++ }
  const backend = useDeviceTarget.getState().backend
  const recorder: null | RecordingBackend = backend && new RecordingBackend(backend, start =>
    recordCall(agentWorkspace, recorder, start, { hit: 1, line: null, source }, nextSeq, reset.signal))
  store.setState({ cursor: null, hoveredCallId: null, run: { currentLine: null, mode: 'live', startedAt: Date.now(), status: 'compiling' }, selectedCallId: null })

  let outcome: RunOutcomeKind = 'failed'
  try {
    const runId = await recorder?.beginRun()
    if (!reset.signal.aborted)
      actions.patchRun({ runId, status: 'running' })
    const runner = recorder && runnerFor(recorder, AbortSignal.any([cancellation, callLifetime.signal]))
    let result: CodemodeResult = cancellation.aborted
      ? { calls: [], error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: [] }
      : await execute(runner ?? undefined, cancellation)
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
    callLifetime.abort()
    try {
      await recorder?.endRun(outcome)
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

function runnerFor(recorder: RecordingBackend, signal: AbortSignal): RunnerClient {
  const { route, target: { headers, transport } } = recorder.sdk()
  const withHeaders = (existing: Headers) => {
    const merged = new Headers(existing)
    for (const [name, value] of Object.entries(headers ?? {}))
      merged.set(name, value)
    return merged
  }
  const connection = new AuvConnection({
    close() {},
    async connect() {},
    duplex: call => transport.duplex({ ...call, headers: withHeaders(call.headers) }),
    unary: call => transport.unary({ ...call, headers: withHeaders(call.headers) }),
  })
  return createAuv(connection).runner({ ...route, signal })
}
