import { createLogg, Format, setGlobalFormat } from '@guiiai/logg'

export interface GuestSetupContext {
  caseId: string
  podName: string
}

export interface SetupCommandResult {
  error: string
  output: string
  returncode: number
  status: string
}

const maxAttempts = 60
const retryDelayMs = 2_000

setGlobalFormat(Format.Pretty)

const setupLog = createLogg('osworld:guest-setup').useGlobalConfig()

/**
 * Installs guest runtime libraries, waiting out transient apt lock contention.
 *
 * A fresh OSWorld VM can start PackageKit concurrently with the episode. Only
 * known apt/dpkg lock failures are retried; package, network, and repository
 * failures return immediately to preserve the original diagnostic.
 */
export async function installRuntimeLibraries(
  execute: () => Promise<SetupCommandResult>,
  signal: AbortSignal,
  context: GuestSetupContext,
): Promise<SetupCommandResult> {
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    signal.throwIfAborted()
    const result = await execute()
    if (!isAptLockContention(result) || attempt === maxAttempts) {
      return result
    }

    setupLog.withFields({
      ...context,
      attempt,
      maxAttempts,
      retryDelayMs,
    }).warn('apt lock is busy; retrying runtime library installation')
    await delay(retryDelayMs, signal)
  }

  throw new Error('Runtime library installation exhausted its bounded retry loop.')
}

async function delay(milliseconds: number, signal: AbortSignal): Promise<void> {
  await new Promise<void>((resolveDelay, reject) => {
    let timeout: ReturnType<typeof setTimeout>
    const onAbort = () => {
      clearTimeout(timeout)
      reject(signal.reason)
    }
    timeout = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolveDelay()
    }, milliseconds)
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

function isAptLockContention(result: SetupCommandResult): boolean {
  if (result.returncode === 0) {
    return false
  }
  const diagnostic = `${result.error}\n${result.output}`.toLowerCase()
  return diagnostic.includes('could not get lock')
    || diagnostic.includes('unable to lock directory')
    || diagnostic.includes('unable to acquire the dpkg frontend lock')
    || diagnostic.includes('is another process using it')
}
