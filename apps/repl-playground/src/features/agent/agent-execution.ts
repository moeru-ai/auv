import type { RunnerClient } from '@auv-js/sdk'
import type { CodeModeRequest, CodemodeResult } from '@exterioragentprotocol/client-sdk/codemode'

import type { RunBackend, RunOutcomeKind, SdkAccess } from '../../backend/types'
import type { BridgedCallEnd, BridgedCallStart } from '../../runtime/sdk-bridge'

import { AuvConnection, createAuv } from '@auv-js/sdk'

import { recordCall } from '../../runtime/record-call'
import { recordUnaryCall } from '../../runtime/sdk-bridge'
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
  let run: null | RunBackend = null
  store.setState({ cursor: null, hoveredCallId: null, run: { currentLine: null, mode: 'live', startedAt: Date.now(), status: 'compiling' }, selectedCallId: null })

  let outcome: RunOutcomeKind = 'failed'
  try {
    const openedRun = await backend?.beginRun() ?? null
    run = openedRun
    const access = openedRun?.sdk()
    if (!reset.signal.aborted)
      actions.patchRun({ runId: access?.route.runId, status: 'running' })
    const runner = access && runnerFor(access, AbortSignal.any([cancellation, callLifetime.signal]), start =>
      recordCall(agentWorkspace, openedRun, start, { hit: 1, line: null, source }, nextSeq, reset.signal))
    let result: CodemodeResult = cancellation.aborted
      ? { calls: [], error: { kind: 'aborted', message: 'Script canceled.' }, ok: false, output: [] }
      : await execute(runner, cancellation)
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

function runnerFor(
  { route, target: { headers, transport } }: SdkAccess,
  signal: AbortSignal,
  observe: (start: Pick<BridgedCallStart, 'body' | 'method'>) => ReturnType<typeof recordCall>,
): RunnerClient {
  const extraHeaders = Object.entries(headers ?? {})
  const addHeaders = (headers: Headers) => {
    for (const [name, value] of extraHeaders)
      headers.set(name, value)
  }
  const connection = new AuvConnection({
    close() {},
    async connect() {},
    duplex(call) {
      addHeaders(call.headers)
      return transport.duplex(call)
    },
    async unary(call) {
      addHeaders(call.headers)
      const finish = await observe(call)
      const entry: BridgedCallEnd = { exchange: [] }
      try {
        return await recordUnaryCall(transport, call, entry)
      }
      finally {
        finish?.(entry)
      }
    },
  })
  return createAuv(connection).runner({ ...route, signal })
}
