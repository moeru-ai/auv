import type { BridgedCallEnd, RpcCallStart } from '@auv-js/sdk/transport/recording'

import type { RunBackend } from '../backend/types'
import type { CallRecord, Workspace } from '../store'

import { camelCaseName } from '@auv-js/sdk'
import { fromBinary, fromJsonString } from '@bufbuild/protobuf'

import { nowMs } from '../store'
import { CallScope, recordRpcResources } from './rpc-resources'

export async function recordCall(
  workspace: Workspace,
  backend: null | RunBackend,
  start: RpcCallStart,
  { hit, line, source }: Pick<CallRecord, 'hit' | 'line' | 'source'>,
  nextSeq: () => number,
  signal: AbortSignal,
) {
  const described = await backend?.sdk().describe(start.method)
  if (signal.aborted)
    return undefined
  const decode = (body: Uint8Array, as: 'request' | 'response') => {
    try {
      return as === 'request' ? described?.decodeRequest(body) : described?.decodeResponse(body)
    }
    catch {
      return `${body.byteLength} bytes`
    }
  }
  const seq = nextSeq()
  const callId = workspace.actions.beginCall({
    args: start.body && described ? [decode(start.body, 'request')] : [],
    effect: described?.effect === 'input' ? 'input' : 'read',
    hit,
    line,
    // The method's API name as JavaScript spells it (`windows.list`), or
    // `Service/Method` without the package for a method that has none.
    method: described?.presentation ? camelCaseName(described.presentation.name) : start.method.slice(start.method.lastIndexOf('.') + 1),
    rpc: { path: start.method, presentation: described?.presentation },
    seq,
    source,
    startedAt: nowMs(),
  })
  // TODO(playground-sdk-stream-progress): a stream's resources appear
  // when it ends; show each scroll-until step as it arrives.
  return (end: BridgedCallEnd) => {
    if (signal.aborted)
      return
    const responses = end.exchange.flatMap(frame => 'response' in frame ? [frame.response] : [])
    const results = end.json !== undefined ? [JSON.parse(end.json) as unknown] : responses.map(body => decode(body, 'response'))
    const scope = new CallScope(workspace, callId, seq, backend)
    if (described) {
      try {
        const request = start.body && fromBinary(described.input, start.body)
        const messages = end.json !== undefined
          ? [fromJsonString(described.output, end.json, { ignoreUnknownFields: true })]
          : responses.map(body => fromBinary(described.output, body))
        recordRpcResources(scope, start.method, request || undefined, messages)
      }
      catch (error) {
        console.warn(`Showing ${start.method} on the canvas failed`, error)
      }
    }
    workspace.actions.endCall(callId, {
      endSeq: nextSeq(),
      error: end.error?.message,
      refs: scope.refs,
      result: results.length === 1 ? results[0] : results,
      status: end.error === undefined ? 'ok' : 'error',
    })
  }
}
