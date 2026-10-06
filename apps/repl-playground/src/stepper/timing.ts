import type { StepSite } from './compile'

/** Aggregated timing for one editor line. */
export interface LineTiming {
  hits: number
  line: number
  /** Time attributed to this line until the next hook fired, excluding pauses. */
  selfMs: number
}

/** Inclusive timing for one top-level statement of a cell. */
export interface StatementTiming {
  endLine: number
  line: number
  /** Time from this top-level hook to the next one (or cell end), excluding pauses. */
  ms: number
  stepId: number
}

export interface TimingSnapshot {
  lines: LineTiming[]
  statements: StatementTiming[]
}

/**
 * Attributes wall time between consecutive step hooks to the line whose hook
 * fired first. Time spent paused by the debugger is excluded.
 *
 * Timestamps are supplied by the caller so the same logic works with
 * `performance.now()` in a worker and with fixed values in tests.
 */
export class StepTimer {
  #active?: { at: number, line: number, paused: number }
  #activeTop?: { at: number, paused: number, site: StepSite }
  readonly #lines = new Map<number, LineTiming>()
  #pausedAt?: number
  readonly #statements: StatementTiming[] = []

  constructor(private readonly sites: readonly StepSite[]) {}

  /** Closes all open segments at `at`; call once when the cell settles. */
  finish(at: number): void {
    this.#closeLine(at)
    this.#closeTop(at)
  }

  pause(at: number): void {
    this.#pausedAt ??= at
  }

  resume(at: number): void {
    if (this.#pausedAt === undefined)
      return
    const paused = at - this.#pausedAt
    this.#pausedAt = undefined
    if (this.#active)
      this.#active.paused += paused
    if (this.#activeTop)
      this.#activeTop.paused += paused
  }

  snapshot(): TimingSnapshot {
    return {
      lines: [...this.#lines.values()].sort((a, b) => a.line - b.line),
      statements: [...this.#statements],
    }
  }

  /** Records that hook `stepId` fired at `at`. */
  step(stepId: number, at: number): StepSite | undefined {
    const site = this.sites[stepId]
    if (!site)
      return undefined
    this.#closeLine(at)
    if (site.top)
      this.#closeTop(at)
    const timing = this.#lines.get(site.line) ?? { hits: 0, line: site.line, selfMs: 0 }
    timing.hits += 1
    this.#lines.set(site.line, timing)
    this.#active = { at, line: site.line, paused: 0 }
    if (site.top)
      this.#activeTop = { at, paused: 0, site }
    return site
  }

  #closeLine(at: number): void {
    if (!this.#active)
      return
    const timing = this.#lines.get(this.#active.line)
    if (timing)
      timing.selfMs += Math.max(0, at - this.#active.at - this.#active.paused)
    this.#active = undefined
  }

  #closeTop(at: number): void {
    if (!this.#activeTop)
      return
    const { paused, site } = this.#activeTop
    this.#statements.push({
      endLine: site.endLine,
      line: site.line,
      ms: Math.max(0, at - this.#activeTop.at - paused),
      stepId: site.id,
    })
    this.#activeTop = undefined
  }
}
