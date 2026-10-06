import type { BindSite, StepSite } from '../stepper/compile'

// ---- Language worker (TypeScript service + cell compiler) ------------------

export interface CompileRequest {
  cellId: number
  source: string
  /** Editor line where `source` starts. */
  startLine: number
}

export type CompileResult
  = | { binds: BindSite[], code: string, hoisted: string[], ok: true, prelude: string, steps: StepSite[] }
    | { column?: number, line?: number, message: string, ok: false }

export interface CompletionDetail {
  doc: string
  type: string
}

export interface CompletionInfo {
  /** TypeScript `ScriptElementKind`, e.g. `method`, `property`, `const`. */
  kind: string
  label: string
  /** TypeScript sort bucket; lower sorts first. */
  sortText: string
}

export interface CompletionResult {
  isMember: boolean
  items: CompletionInfo[]
}

export interface DiagnosticInfo {
  from: number
  message: string
  severity: 'error' | 'warning'
  to: number
}

export interface ExecWorkerApi {
  resume: (mode: ResumeMode) => void
  run: (request: RunRequest) => Promise<RunOutcome>
  setBreakpoints: (lines: number[]) => void
  stop: () => void
}

/** Host functions the execution worker calls. `on*` are fire-and-forget events. */
export interface HostApi {
  /** Invokes one script binding (`auv.*`) on the host. */
  call: (method: string, args: WireValue[], stepId: null | number) => Promise<WireValue>
  /** Values bound by a declaration or loop header, right after assignment. */
  onBind: (bindId: number, values: Record<string, WireValue>, at: number) => void
  /** Editor-only overlay from `draw()`: an area, rectangle or point. */
  onDraw: (target: WireValue, label: string | undefined, stepId: null | number) => void
  /** Editor-only camera move from `focus()`. */
  onFocus: (target: WireValue, options: undefined | { autoZoomOut?: boolean, zoom?: boolean }, stepId: null | number, at: number) => void
  onLog: (level: LogLevel, values: WireValue[], stepId: null | number, label?: string) => void
  onPause: (stepId: number, at: number) => void
  onResume: (at: number) => void
  onStep: (stepId: number, at: number) => void
  onVars: (vars: Record<string, WireValue>) => void
}

// ---- Execution worker --------------------------------------------------------

export interface HoverInfo {
  doc: string
  from: number
  to: number
  type: string
}

export interface LanguageWorkerApi {
  compile: (request: CompileRequest) => CompileResult
  /** Detail for one entry; `source` syncs the cell first when given. */
  completionDetail: (pos: number, name: string, source?: string) => CompletionDetail | null
  /** `source` is the current document; completions must not see a debounced, stale cell. */
  completions: (pos: number, source?: string) => CompletionResult
  diagnostics: () => DiagnosticInfo[]
  hover: (pos: number, source?: string) => HoverInfo | null
  init: (source: string) => Promise<void>
  update: (source: string) => void
}

export type LogLevel = 'error' | 'info' | 'log' | 'show' | 'warn'

export type ResumeMode = 'continue' | 'step'

export type RunOutcome
  = | { line?: number, message: string, stack?: string, status: 'error' }
    | { status: 'ok', value: WireValue }
    | { status: 'stopped' }

export interface RunRequest {
  binds: BindSite[]
  breakpoints: number[]
  code: string
  hoisted: string[]
  /** Pause at the first hook (`step`) or only at breakpoints (`run`). */
  mode: 'run' | 'step'
  prelude: string
  steps: StepSite[]
}

/**
 * Values crossing from the execution worker to the host. Resource handles stay
 * as plain `{ $ref, kind, ... }` objects; functions and cycles are summarized.
 */
export type WireValue = unknown

export const HOST_EVENTS = ['onBind', 'onDraw', 'onFocus', 'onLog', 'onPause', 'onResume', 'onStep', 'onVars'] as const satisfies (keyof HostApi)[]
