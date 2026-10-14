import type { AgentResult, AgentTelemetry, AgentTools, ModelEndpoint } from '../agent'

import process from 'node:process'

import { runAgent as runBaselineAgent } from './baseline/runAgent'
import { observationPolicyFrom, runAgent as runPiAgent } from './pi/runAgent'

/**
 * Which model -> tool loop (harness) drives an episode. It is the `harness`
 * run-matrix axis, so runs of different harnesses are separate rows.
 *
 * - `pi`: `@earendil-works/pi-agent-core` with harness observations after
 *   actions and a multi-screenshot window (see `./pi/runAgent.ts`).
 * - `baseline`: the frozen hand-written loop the 2026-10-12/13 runs used,
 *   kept to sanity-check scores and harness changes against
 *   (see `./baseline/runAgent.ts`).
 */
export type Harness = 'baseline' | 'pi'

/** Validates a run-matrix `harness` value (`OSWORLD_HARNESS` in `vieval.config.ts`). */
export function harnessFrom(value: unknown): Harness {
  if (value === 'pi' || value === 'baseline') {
    return value
  }
  throw new Error(`OSWORLD_HARNESS must list "pi" or "baseline", got ${JSON.stringify(value)}.`)
}

/** Runs `harness` for one episode; both return the same {@link AgentResult}. */
export async function runAgent(
  harness: Harness,
  instruction: string,
  endpoint: ModelEndpoint,
  tools: AgentTools,
  options: { maxSteps: number, signal: AbortSignal, telemetry?: AgentTelemetry },
): Promise<AgentResult> {
  if (harness === 'baseline') {
    return await runBaselineAgent(instruction, endpoint, tools, options)
  }
  return await runPiAgent(instruction, endpoint, tools, { ...options, observation: observationPolicyFrom(process.env) })
}
