import type { Result } from 'tinyexec'

import type { AuvConnection, DeviceCredential } from '../transport/connection'
import type { OperationOptions } from '../transport/types'

import process from 'node:process'

import { randomUUID } from 'node:crypto'
import { join, resolve } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'

import { merge } from '@moeru/std/merge'
import { isWindows } from 'std-env'
import { x } from 'tinyexec'

import { checkHealth } from '../apis/auv-daemon/health'
import { abortError } from '../transport/errors'
import { connect } from './connect'

/** One running, app-owned AUV daemon child process. */
export interface AuvDaemon {
  readonly binaryPath: string
  /** Connects to the preferred endpoint selected from this daemon's listeners. */
  connect: (options?: AuvDaemonConnectOptions) => Promise<AuvConnection>
  /** Preferred endpoint defaults suitable for serialization across Electron IPC. */
  readonly connectionOptions: AuvDaemonConnectionOptions
  /** Every listener URI configured for this daemon. */
  readonly endpoints: readonly string[]
  /** Resolves whenever the daemon exits, including exits it initiates itself. */
  readonly exited: Promise<AuvDaemonExit>
  readonly pid: number
  /** Gracefully stops the child, escalating to a forced stop after its deadline. */
  stop: () => Promise<AuvDaemonExit>
  /** Absolute root used for daemon control state and recorded Runs. */
  readonly storeRoot: string
}

/** Serializable defaults for connecting to the preferred bound endpoint. */
export interface AuvDaemonConnectionOptions {
  readonly endpoint: string
  readonly local: boolean
  readonly transport: 'http' | 'npipe' | 'unix'
}

/** Options accepted when connecting to a daemon started by this process. */
export interface AuvDaemonConnectOptions extends OperationOptions {
  credential?: DeviceCredential
  transport?: 'grpc' | 'http' | 'npipe' | 'unix'
}

/** Observable completion of an app-owned AUV child process. */
export interface AuvDaemonExit {
  readonly code: null | number
  readonly signal: NodeJS.Signals | null
}

/** Options for starting one app-owned `auv serve` child process. */
export interface StartAuvOptions extends OperationOptions {
  /**
   * AUV executable path or command name resolved through `PATH`.
   * @default 'auv'
   */
  binaryPath?: string
  /** Seconds without live Runners before the daemon exits. */
  daemonIdleTimeoutSeconds?: number
  /** Path at which the daemon publishes discovery metadata. */
  discoveryFile?: string
  /** Environment values overlaid on the current Node.js process environment. */
  environment?: NodeJS.ProcessEnv
  /**
   * Listener URIs passed as repeated `--listen` arguments. Ports must be
   * explicit. An empty list uses a caller-local Unix socket below `storeRoot`,
   * or an owner-protected named pipe on Windows.
   * @default []
   */
  listeners?: readonly string[]
  /**
   * Prevents this child daemon from publishing discovery metadata.
   * @default false
   */
  noDiscovery?: boolean
  /** Durable short-token and Device-bearer authentication store. */
  pairingStore?: string
  /**
   * Operator-trusted custom Runner provider manifests.
   * @default []
   */
  runnerProviders?: readonly string[]
  /**
   * Maximum time to wait for graceful shutdown before killing the child.
   * @default 5000
   */
  shutdownTimeoutMs?: number
  /** Aborting this signal terminates the daemon child through tinyexec. */
  signal?: AbortSignal
  /**
   * Maximum time to observe every configured listener serving API calls.
   * @default 10000
   */
  startupTimeoutMs?: number
  /**
   * Root directory used for daemon control state and recorded Runs.
   * @default '<workingDirectory>/.auv/store'
   */
  storeRoot?: string
  /**
   * Directory against which the CLI resolves relative paths.
   * @default process.cwd()
   */
  workingDirectory?: string
}

type AuvChildProcess = Result
interface ResolvedStartAuvOptions extends StartAuvOptions {
  binaryPath: string
  listeners: readonly string[]
  noDiscovery: boolean
  runnerProviders: readonly string[]
  shutdownTimeoutMs: number
  startupTimeoutMs: number
  workingDirectory: string
}

/** Failure to spawn `auv serve` or observe all listeners becoming healthy. */
export class AuvDaemonStartError extends Error {
  /** Recent stdout and stderr captured by tinyexec. */
  readonly output: string

  constructor(message: string, output: string, cause?: unknown) {
    super(message, { cause })
    this.name = 'AuvDaemonStartError'
    this.output = output
  }
}

/** Starts an app-owned foreground daemon and waits for every listener to become healthy. */
export async function startAuv(options: StartAuvOptions = {}): Promise<AuvDaemon> {
  const {
    binaryPath,
    daemonIdleTimeoutSeconds,
    discoveryFile,
    environment,
    listeners,
    noDiscovery,
    pairingStore,
    runnerProviders,
    shutdownTimeoutMs,
    signal,
    startupTimeoutMs,
    storeRoot: configuredStoreRoot,
    workingDirectory: configuredWorkingDirectory,
  } = merge<ResolvedStartAuvOptions, StartAuvOptions>({
    binaryPath: 'auv',
    listeners: [],
    noDiscovery: false,
    runnerProviders: [],
    shutdownTimeoutMs: 5_000,
    startupTimeoutMs: 10_000,
    workingDirectory: process.cwd(),
  }, options)

  if (!Number.isFinite(startupTimeoutMs))
    throw new RangeError('startupTimeoutMs must be finite')
  if (startupTimeoutMs <= 0)
    throw new RangeError('startupTimeoutMs must be greater than 0')
  if (!Number.isFinite(shutdownTimeoutMs))
    throw new RangeError('shutdownTimeoutMs must be finite')
  if (shutdownTimeoutMs <= 0)
    throw new RangeError('shutdownTimeoutMs must be greater than 0')
  if (daemonIdleTimeoutSeconds !== undefined && !Number.isFinite(daemonIdleTimeoutSeconds))
    throw new RangeError('daemonIdleTimeoutSeconds must be finite')
  if (daemonIdleTimeoutSeconds !== undefined && !Number.isInteger(daemonIdleTimeoutSeconds))
    throw new RangeError('daemonIdleTimeoutSeconds must be an integer')
  if (daemonIdleTimeoutSeconds !== undefined && daemonIdleTimeoutSeconds <= 0)
    throw new RangeError('daemonIdleTimeoutSeconds must be greater than 0')

  const workingDirectory = resolve(configuredWorkingDirectory)
  const storeRoot = resolve(workingDirectory, configuredStoreRoot ?? join('.auv', 'store'))
  const endpoints = Object.freeze(listeners.length === 0
    ? [isWindows ? `npipe://./pipe/auv-${randomUUID()}` : `unix://${join(storeRoot, 'auv.sock')}`]
    : [...listeners])

  for (const endpoint of endpoints) {
    const url = new URL(endpoint)
    if (url.protocol === 'http:' && url.port === '0') {
      throw new RangeError('startAuv listener ports must be explicit; port 0 cannot be used as a connection endpoint')
    }
  }

  const child = x(binaryPath, serveArguments({
    daemonIdleTimeoutSeconds,
    discoveryFile,
    listeners: endpoints,
    noDiscovery,
    pairingStore,
    runnerProviders,
    storeRoot: configuredStoreRoot,
  }), {
    nodeOptions: {
      cwd: workingDirectory,
      env: { ...process.env, ...environment },
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    },
    nodePath: false,
    signal,
  })

  const startupDeadline = AbortSignal.timeout(startupTimeoutMs)
  const startupSignal = signal === undefined ? startupDeadline : AbortSignal.any([signal, startupDeadline])

  const completion = Promise.resolve(child).then(
    ({ stderr, stdout }) => ({
      code: child.exitCode ?? null,
      error: undefined,
      output: [stdout, stderr].filter(Boolean).join('\n').slice(-(64 * 1024)),
      signal: child.signalCode as NodeJS.Signals | null,
    }),
    error => ({
      code: child.exitCode ?? null,
      error,
      output: '',
      signal: child.signalCode as NodeJS.Signals | null,
    }),
  )

  const exited = completion.then(({ code, signal }) => ({ code, signal }))

  try {
    await Promise.race([
      waitForHealth(endpoints, pairingStore !== undefined, startupSignal),
      completion.then((result) => {
        throw new AuvDaemonStartError(
          `AUV daemon exited before becoming healthy (code ${String(result.code)}, signal ${String(result.signal)})`,
          result.output,
          result.error,
        )
      }),
    ])

    const connectionOptions = preferredConnection(endpoints, pairingStore !== undefined)
    let stopping: Promise<AuvDaemonExit> | undefined

    return {
      binaryPath,
      connect(connectOptions = {}) {
        return connect({
          credential: connectOptions.credential,
          endpoint: connectionOptions.endpoint,
          local: connectionOptions.local,
          signal: connectOptions.signal,
          transport: connectOptions.transport ?? connectionOptions.transport,
        })
      },
      connectionOptions,
      endpoints,
      exited,
      pid: child.pid!,
      stop() {
        stopping ??= stopChild(child, exited, shutdownTimeoutMs)
        return stopping
      },
      storeRoot,
    }
  }
  catch (error) {
    if (!signal?.aborted)
      await stopChild(child, exited, shutdownTimeoutMs).catch(() => {})

    const result = await completion
    if (signal?.aborted)
      throw abortError(signal)
    if (startupDeadline.aborted) {
      throw new AuvDaemonStartError(
        `AUV daemon did not become healthy within ${startupTimeoutMs}ms`,
        result.output,
        error,
      )
    }

    if (error instanceof AuvDaemonStartError)
      throw error

    throw new AuvDaemonStartError('Failed to start AUV daemon', result.output, error)
  }
}

function preferredConnection(endpoints: readonly string[], pairedHttp: boolean): AuvDaemonConnectionOptions {
  const endpoint = endpoints.find(value => value.startsWith('unix://') || value.startsWith('npipe://')) ?? endpoints[0]!
  if (endpoint.startsWith('npipe://')) {
    return {
      endpoint,
      local: true,
      transport: 'npipe',
    }
  }
  if (endpoint.startsWith('unix://')) {
    return {
      endpoint: endpoint.slice('unix://'.length),
      local: true,
      transport: 'unix',
    }
  }
  return {
    endpoint,
    local: !pairedHttp,
    transport: 'http',
  }
}

function serveArguments(options: StartAuvOptions): string[] {
  const args = ['serve']
  for (const listener of options.listeners ?? [])
    args.push('--listen', listener)

  if (options.pairingStore !== undefined)
    args.push('--pairing-store', options.pairingStore)
  if (options.storeRoot !== undefined)
    args.push('--store-root', options.storeRoot)
  if (options.discoveryFile !== undefined)
    args.push('--discovery-file', options.discoveryFile)

  if (options.noDiscovery)
    args.push('--no-discovery')
  if (options.daemonIdleTimeoutSeconds !== undefined)
    args.push('--daemon-idle-timeout', String(options.daemonIdleTimeoutSeconds))
  for (const provider of options.runnerProviders ?? [])
    args.push('--runner-provider', provider)

  return args
}

async function stopChild(
  child: AuvChildProcess,
  exited: Promise<AuvDaemonExit>,
  shutdownTimeoutMs: number,
): Promise<AuvDaemonExit> {
  if (child.exitCode !== undefined || child.signalCode !== null || child.pid === undefined)
    return exited

  child.kill('SIGINT')

  let cancelTimeout!: () => void
  const shutdownTimeout = new Promise<void>((resolveTimeout) => {
    const handle = setTimeout(resolveTimeout, shutdownTimeoutMs)
    cancelTimeout = () => clearTimeout(handle)
  })

  const graceful = await Promise.race([
    exited.then(value => ({ case: 'exit' as const, value })),
    shutdownTimeout.then(() => ({ case: 'timeout' as const })),
  ])
  if (graceful.case === 'exit') {
    cancelTimeout()
    return graceful.value
  }

  child.kill('SIGKILL')

  return exited
}

async function waitForHealth(endpoints: readonly string[], pairedHttp: boolean, signal: AbortSignal): Promise<void> {
  await Promise.all(endpoints.map(async (endpoint) => {
    while (true) {
      let connection: AuvConnection | undefined
      try {
        const unix = endpoint.startsWith('unix://')
        const namedPipe = endpoint.startsWith('npipe://')
        connection = await connect({
          endpoint: unix ? endpoint.slice('unix://'.length) : endpoint,
          local: unix || namedPipe || !pairedHttp,
          signal,
          transport: unix ? 'unix' : namedPipe ? 'npipe' : 'http',
        })
        await checkHealth(connection, { signal })
        return
      }
      catch (error) {
        if (signal.aborted)
          throw error
        await delay(25, undefined, { signal })
      }
      finally {
        await connection?.close().catch(() => {})
      }
    }
  }))
}
