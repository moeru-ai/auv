import type { AxNode } from './backend/types'
import type { WireValue } from './runtime/protocol'
import type { Point } from './script-api/api'
import type { CallRecord, PlaygroundState, Resource } from './store'

// Pure "as of time t" queries over the event log. A seq is a position in the
// run's event log (steps, binds, calls and logs share one counter); `null`
// means "follow the latest event".

export interface ValueSample {
  hit: number
  line: number
  seq: number
  value: WireValue
}

/** Every value a name was bound to during the run, oldest first. */
export function bindHistory(state: PlaygroundState, name: string): ValueSample[] {
  const samples: ValueSample[] = []
  for (const bind of state.binds) {
    if (name in bind.values)
      samples.push({ hit: bind.hit, line: bind.line, seq: bind.seq, value: bind.values[name] })
  }
  return samples
}

/** Names that were bound to a value referencing `ref`. */
export function bindingsOf(binds: PlaygroundState['binds'], ref: string): Array<{ hit: number, line: number, name: string, seq: number }> {
  const out: Array<{ hit: number, line: number, name: string, seq: number }> = []
  for (const bind of binds) {
    for (const [name, value] of Object.entries(bind.values)) {
      if (references(value, ref))
        out.push({ hit: bind.hit, line: bind.line, name, seq: bind.seq })
    }
  }
  return out
}

/** Last call that started at or before `seq`. */
export function callAt(state: PlaygroundState, seq: number): CallRecord | undefined {
  return state.calls.findLast(call => call.seq <= seq)
}

export function callsUpTo(state: PlaygroundState, seq: number): CallRecord[] {
  return state.calls.filter(call => call.seq <= seq)
}

/** Calls whose arguments reference `ref` (lineage: where a handle was used). */
export function consumersOf(calls: readonly CallRecord[], ref: string): CallRecord[] {
  return calls.filter(call => references(call.args, ref))
}

/** Effective cursor: the explicit time cursor, or the newest event. */
export function cursorSeq(state: PlaygroundState): number {
  return state.cursor ?? Math.max(0, state.seqAt.length - 1)
}

/** Frames captured from a source (e.g. `window:w-counter`) over time. */
export function framesOf(state: PlaygroundState, source: string): Array<Resource & { kind: 'frame' }> {
  return Object.values(state.resources)
    .filter((resource): resource is Resource & { kind: 'frame' } => resource.kind === 'frame' && resource.run === state.runIndex && resource.handle.source === source)
    .sort((a, b) => a.seq - b.seq)
}

export function isFollowingLatest(state: PlaygroundState): boolean {
  return state.cursor === null
}

/** The newest frame per source as of `seq`, oldest first (for painting order). */
export function latestFramesAt(state: PlaygroundState, seq: number): Array<Resource & { kind: 'frame' }> {
  const latest = new Map<string, Resource & { kind: 'frame' }>()
  for (const resource of Object.values(state.resources)) {
    // Seqs restart every run; only frames of the current run are comparable.
    if (resource.kind !== 'frame' || resource.run !== state.runIndex || resource.seq > seq)
      continue
    const current = latest.get(resource.handle.source)
    if (!current || current.seq <= resource.seq)
      latest.set(resource.handle.source, resource)
  }
  return [...latest.values()].sort((a, b) => a.seq - b.seq)
}

/** Editor line that was executing at `seq`, if any. */
export function lineAt(state: PlaygroundState, seq: number): null | number {
  const steps = state.steps
  let lo = 0
  let hi = steps.length - 1
  let found: null | number = null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (steps[mid]!.seq <= seq) {
      found = steps[mid]!.line
      lo = mid + 1
    }
    else {
      hi = mid - 1
    }
  }
  return found
}

/** Deepest accessibility node whose frame contains `point`. */
export function pickAxNode(node: AxNode, point: Point): AxNode | undefined {
  for (const child of node.children) {
    const hit = pickAxNode(child, point)
    if (hit)
      return hit
  }
  const f = node.frame
  if (f && point.x >= f.x && point.x <= f.x + f.width && point.y >= f.y && point.y <= f.y + f.height)
    return node
  return undefined
}

export function refOf(value: WireValue): string | undefined {
  return typeof value === 'object' && value !== null ? (value as { $ref?: unknown }).$ref as string | undefined : undefined
}

/** Index of the sample shown by default: the latest one at or before `seq`. */
export function sampleIndexAt(samples: { seq: number }[], seq: number): number {
  let index = -1
  for (let i = 0; i < samples.length; i++) {
    if (samples[i]!.seq <= seq)
      index = i
  }
  return index === -1 ? samples.length - 1 : index
}

/** Values of every bound name as of `seq` (the latest binding of each). */
export function varsAt(state: PlaygroundState, seq: number): Record<string, WireValue> {
  const vars: Record<string, WireValue> = {}
  for (const bind of state.binds) {
    if (bind.seq > seq)
      break
    Object.assign(vars, bind.values)
  }
  return vars
}

function references(value: WireValue, ref: string, depth = 0): boolean {
  if (depth > 6 || value === null || typeof value !== 'object')
    return false
  if (refOf(value) === ref)
    return true
  const values = Array.isArray(value) ? value : Object.values(value as Record<string, WireValue>)
  return values.some(item => references(item, ref, depth + 1))
}
