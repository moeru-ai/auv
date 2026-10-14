import type { Options, Output, Result } from 'tinyexec'

import process from 'node:process'

import { appendFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'

import { errorMessageFrom } from '@moeru/std'
import { x } from 'tinyexec'

import { packageDirectory } from './cases'

export interface CommandLogContext {
  /** Checked-in Vieval case that caused the command. */
  caseId: string
  /** Episode-owned Pod, when it is known. */
  podName?: string
  /** Where the process is executed. */
  runtime: 'guest' | 'host'
  /** Process family, used to filter AUV from kubectl activity. */
  tool: 'auv' | 'kubectl' | 'osworld'
}

export interface KubernetesResourceLog {
  action: 'create' | 'delete'
  apiVersion: string
  caseId: string
  exitCode?: number
  kind: string
  /** Exact desired resource submitted to Kubernetes, when locally available. */
  manifest?: Readonly<Record<string, unknown>>
  name: string
  namespace: string
  outcome: 'failed' | 'succeeded'
}

export interface LoggedProcess {
  completion: Promise<Output>
  process: Result
}

/**
 * JSONL file that receives every execution record of this process.
 *
 * NOTICE: Records go to a file, not the console. With several concurrent
 * episodes, per-command console lines bury Vieval's own progress output.
 * Each record carries `caseId` and `podName`, so `jq` or `grep` can still
 * select one episode. Set `OSWORLD_LOG_FILE` to choose the path.
 */
export const executionLogFile = process.env.OSWORLD_LOG_FILE
  ?? join(packageDirectory, '.vieval', 'logs', `execution-${new Date().toISOString().replaceAll(':', '-')}-${String(process.pid)}.jsonl`)

let executionLogReady = false

/** Appends one structured record to {@link executionLogFile}. */
export function writeExecutionLog(
  scope: string,
  level: 'error' | 'log' | 'warn',
  message: string,
  fields: object,
  error?: unknown,
): void {
  if (!executionLogReady) {
    mkdirSync(dirname(executionLogFile), { recursive: true })
    executionLogReady = true
  }
  const record = {
    level,
    message,
    scope,
    time: new Date().toISOString(),
    ...fields,
    ...(error === undefined ? {} : { error: errorMessageFrom(error) ?? String(error) }),
  }
  // Synchronous append keeps concurrent episodes' lines whole and ordered.
  appendFileSync(executionLogFile, `${JSON.stringify(record)}\n`)
}

const commandLog = {
  error: (fields: Readonly<Record<string, unknown>>, error: unknown, message: string) => writeExecutionLog('osworld:command', 'error', message, fields, error),
  log: (fields: Readonly<Record<string, unknown>>, message: string) => writeExecutionLog('osworld:command', 'log', message, fields),
}

/**
 * Records a command whose process is started across the OSWorld setup API.
 * The callback keeps HTTP and response validation in the episode module.
 */
export async function observeGuestCommand<T>(
  executable: string,
  args: readonly string[],
  context: Omit<CommandLogContext, 'runtime'>,
  execute: () => Promise<T>,
  exitCodeOf: (result: T) => number,
): Promise<T> {
  const startedAt = Date.now()
  const fields = {
    ...context,
    args,
    command: formatCommand(executable, args),
    executable,
    runtime: 'guest',
  }

  commandLog.log(fields, 'command started')
  try {
    const result = await execute()
    const exitCode = exitCodeOf(result)
    commandLog.log({
      ...fields,
      durationMs: Date.now() - startedAt,
      exitCode,
      outcome: exitCode === 0 ? 'succeeded' : 'failed',
    }, 'command completed')
    return result
  }
  catch (error) {
    commandLog.error({
      ...fields,
      durationMs: Date.now() - startedAt,
      outcome: 'failed',
    }, error, 'command failed before completion')
    throw error
  }
}

/** Records the outcome and identity of a Kubernetes resource lifecycle action. */
export function recordKubernetesResource(resource: KubernetesResourceLog): void {
  // TODO(osworld-resource-mutations): Add update/patch actions when an
  // owner-approved runner path actually mutates resources after creation.
  const fields = resource.kind.toLowerCase() === 'secret' && resource.manifest != null
    ? { ...resource, manifest: '[redacted]', manifestRedacted: true }
    : resource
  if (resource.outcome === 'failed') {
    writeExecutionLog('osworld:kubernetes-resource', 'warn', 'Kubernetes resource action failed', fields)
    return
  }
  writeExecutionLog('osworld:kubernetes-resource', 'log', 'Kubernetes resource action completed', fields)
}

/** Runs a local process through the same observable lifecycle as long-running commands. */
export async function runLoggedCommand(
  executable: string,
  args: readonly string[],
  options: Partial<Options>,
  context: CommandLogContext,
): Promise<Output> {
  return await startLoggedCommand(executable, args, options, context).completion
}

/**
 * Starts a local process and records its exact argv, duration, and outcome.
 *
 * stdin and environment values are deliberately excluded: pairing credentials
 * and other secrets can cross those channels. `hasStdin` still makes the
 * otherwise invisible input boundary debuggable.
 */
export function startLoggedCommand(
  executable: string,
  args: readonly string[],
  options: Partial<Options>,
  context: CommandLogContext,
): LoggedProcess {
  const startedAt = Date.now()
  const fields = {
    ...context,
    args,
    command: formatCommand(executable, args),
    executable,
    hasStdin: options.stdin != null,
  }

  commandLog.log(fields, 'command started')
  const child = x(executable, args, options)
  const completion = Promise.resolve(child).then(
    (output) => {
      const outcome = output.exitCode === 0
        ? 'succeeded'
        : child.killed
          ? 'stopped'
          : 'failed'
      commandLog.log({
        ...fields,
        durationMs: Date.now() - startedAt,
        exitCode: output.exitCode,
        outcome,
        signalCode: child.signalCode,
        // A long-lived `kubectl port-forward` reports dropped streams only on
        // stderr; keep its tail when a command does not succeed.
        ...(outcome === 'succeeded' ? {} : { stderrTail: output.stderr.slice(-2_000) }),
      }, 'command completed')
      return output
    },
    (error: unknown) => {
      commandLog.error({
        ...fields,
        durationMs: Date.now() - startedAt,
        outcome: 'failed',
      }, error, 'command failed before completion')
      throw error
    },
  )

  return { completion, process: child }
}

/** Produces a copy-pasteable POSIX command while retaining argv separately. */
function formatCommand(executable: string, args: readonly string[]): string {
  return [executable, ...args].map(quoteArgument).join(' ')
}

function quoteArgument(argument: string): string {
  if (/^[\w./:=@+,-]+$/.test(argument)) {
    return argument
  }
  return `'${argument.replaceAll('\'', String.raw`'"'"'`)}'`
}
