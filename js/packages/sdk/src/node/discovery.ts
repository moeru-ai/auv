import type { AuvEnvironment } from './context'

import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { readFile } from 'node:fs/promises'

import { AuvConfigurationError } from '../transport/errors'

/** A daemon endpoint ready to pass to `connect()`. */
export interface DiscoveredEndpoint {
  readonly endpoint: string
  /** Placement stays automatic, matching the Rust client. */
  readonly local: false
  readonly transport: 'grpc' | 'npipe' | 'unix'
}

export interface DiscoverEndpointOptions {
  /** Explicit endpoint; wins over `AUV_ENDPOINT` and the descriptor. */
  endpoint?: string
  /** Environment to read; defaults to `process.env`. */
  env?: AuvEnvironment
}

interface Descriptor {
  readonly endpoint: string
  readonly instanceId: string
  readonly storeRoot?: string
}

/**
 * Resolves the daemon endpoint like the Rust client: explicit endpoint,
 * then `AUV_ENDPOINT`, then the current user's discovery descriptor.
 * Resolves `undefined` when no descriptor is published.
 */
export async function discoverEndpoint(options: DiscoverEndpointOptions = {}): Promise<DiscoveredEndpoint | undefined> {
  const env = options.env ?? process.env
  const selected = options.endpoint
    ?? env.AUV_ENDPOINT
    ?? (await readDescriptor(discoveryPath(env)))?.endpoint
  return selected === undefined ? undefined : parseEndpoint(selected)
}

/** Descriptor path, overridable by `AUV_DISCOVERY_FILE`. */
export function discoveryPath(
  env: AuvEnvironment = process.env,
  platform: NodeJS.Platform = process.platform,
): string {
  const override = env.AUV_DISCOVERY_FILE
  if (override !== undefined)
    return override
  const paths = platform === 'win32' ? path.win32 : path.posix
  return paths.join(stateDirectory(env, platform), 'api-server.json')
}

function absolute(value: string | undefined, paths: typeof path.posix): string | undefined {
  return value !== undefined && paths.isAbsolute(value) ? value : undefined
}

function homeDirectory(env: AuvEnvironment): string {
  // dirs-sys ignores an empty HOME and falls back to the passwd entry.
  const home = env.HOME
  if (home !== undefined && home !== '')
    return home
  const fallback = os.userInfo().homedir
  if (fallback === '')
    throw new AuvConfigurationError('could not resolve the current user\'s AUV state directory')
  return fallback
}

/** Mirrors Rust `ConnectEndpoint::from_str`. */
function parseEndpoint(value: string): DiscoveredEndpoint {
  const invalid = (reason: string): never => {
    throw new AuvConfigurationError(`invalid AUV API endpoint ${JSON.stringify(value)}: ${reason}`)
  }

  if (value.startsWith('unix://')) {
    // Keep the raw path; URL parsing would percent-encode spaces.
    const socket = value.slice('unix://'.length)
    if (!path.posix.isAbsolute(socket))
      invalid('Unix socket path must be absolute')
    return { endpoint: socket, local: false, transport: 'unix' }
  }
  if (value.startsWith('npipe://./pipe/')) {
    if (!/^[\w.-]+$/.test(value.slice('npipe://./pipe/'.length)))
      invalid('invalid named pipe name')
    return { endpoint: value, local: false, transport: 'npipe' }
  }

  let url: URL
  try {
    url = new URL(value)
  }
  catch {
    return invalid('not a valid URI')
  }
  if (url.protocol !== 'http:')
    invalid(`unsupported scheme ${url.protocol.replace(/:$/, '')}`)
  if (url.hostname === '')
    invalid('TCP endpoint omitted host')
  if (url.pathname !== '/' || url.search !== '')
    invalid('TCP endpoint must not include a path or query')
  return { endpoint: `http://${url.host}`, local: false, transport: 'grpc' }
}

/** Reads a descriptor; a missing file is not an error. */
async function readDescriptor(file: string): Promise<Descriptor | undefined> {
  let text: string
  try {
    text = await readFile(file, 'utf8')
  }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT')
      return undefined
    throw new AuvConfigurationError(`failed to read daemon descriptor ${file}: ${String(error)}`)
  }

  let decoded: unknown
  try {
    decoded = JSON.parse(text)
  }
  catch {
    throw new AuvConfigurationError(`invalid daemon descriptor ${file}: not valid JSON`)
  }
  if (typeof decoded !== 'object' || decoded === null || Array.isArray(decoded))
    throw new AuvConfigurationError(`invalid daemon descriptor ${file}: expected a JSON object`)

  const source = decoded as Readonly<Record<string, unknown>>
  const { endpoint, instance_id: instanceId, store_root: storeRoot } = source
  if (typeof endpoint !== 'string')
    throw new AuvConfigurationError(`invalid daemon descriptor ${file}: endpoint must be a string`)
  if (typeof instanceId !== 'string')
    throw new AuvConfigurationError(`invalid daemon descriptor ${file}: instance_id must be a string`)
  // Descriptors before 2026-10-11 omit store_root.
  if (storeRoot !== undefined && storeRoot !== null && typeof storeRoot !== 'string')
    throw new AuvConfigurationError(`invalid daemon descriptor ${file}: store_root must be a string`)
  return { endpoint, instanceId, storeRoot: storeRoot ?? undefined }
}

/**
 * Mirrors `ProjectDirs::from("ai", "moeru", "auv")` from the Rust
 * `directories` crate: `state_dir`, else `data_local_dir`.
 */
function stateDirectory(env: AuvEnvironment, platform: NodeJS.Platform): string {
  if (platform === 'win32') {
    // NOTICE: Rust reads FOLDERID_LocalAppData; Node has no known-folder
    // API, so LOCALAPPDATA approximates it.
    const local = absolute(env.LOCALAPPDATA, path.win32)
      ?? path.win32.join(homeDirectory(env), 'AppData', 'Local')
    return path.win32.join(local, 'moeru', 'auv', 'data')
  }
  const home = homeDirectory(env)
  if (platform === 'darwin')
    return path.posix.join(home, 'Library', 'Application Support', 'ai.moeru.auv')
  const state = absolute(env.XDG_STATE_HOME, path.posix)
    ?? path.posix.join(home, '.local', 'state')
  return path.posix.join(state, 'auv')
}
