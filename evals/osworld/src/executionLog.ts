import type { Options, Output, Result } from 'tinyexec'

import { createLogg, Format, setGlobalFormat } from '@guiiai/logg'
import { x } from 'tinyexec'

export interface CommandLogContext {
  /** Checked-in Vieval case that caused the command. */
  caseId: string
  /** Episode-owned Pod, when it is known. */
  podName?: string
  /** Where the process is executed. */
  runtime: 'guest' | 'host'
  /** Process family, used to filter AUV from kubectl activity. */
  tool: 'auv' | 'kubectl'
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

setGlobalFormat(Format.Pretty)

const commandLog = createLogg('osworld:command').useGlobalConfig()
const resourceLog = createLogg('osworld:kubernetes-resource').useGlobalConfig()

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

  commandLog.withFields(fields).log('command started')
  try {
    const result = await execute()
    const exitCode = exitCodeOf(result)
    commandLog.withFields({
      ...fields,
      durationMs: Date.now() - startedAt,
      exitCode,
      outcome: exitCode === 0 ? 'succeeded' : 'failed',
    }).log('command completed')
    return result
  }
  catch (error) {
    commandLog.withFields({
      ...fields,
      durationMs: Date.now() - startedAt,
      outcome: 'failed',
    }).withError(error).error('command failed before completion')
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
  const logger = resourceLog.withFields(fields)
  if (resource.outcome === 'failed') {
    logger.warn('Kubernetes resource action failed')
    return
  }
  logger.log('Kubernetes resource action completed')
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

  commandLog.withFields(fields).log('command started')
  const child = x(executable, args, options)
  const completion = Promise.resolve(child).then(
    (output) => {
      const outcome = output.exitCode === 0
        ? 'succeeded'
        : child.killed
          ? 'stopped'
          : 'failed'
      commandLog.withFields({
        ...fields,
        durationMs: Date.now() - startedAt,
        exitCode: output.exitCode,
        outcome,
        signalCode: child.signalCode,
      }).log('command completed')
      return output
    },
    (error: unknown) => {
      commandLog.withFields({
        ...fields,
        durationMs: Date.now() - startedAt,
        outcome: 'failed',
      }).withError(error).error('command failed before completion')
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
