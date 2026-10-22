import type { AuvConnection, ConnectOptions } from '../transport/connection'
import type { Transport } from '../transport/types'
import type { AuvEnvironment } from './context'

import { connectTransport } from '../transport/connection'
import { AuvConfigurationError } from '../transport/errors'
import { createHttpTransport } from '../web/http'
import { discoverEndpoint } from './discovery'
import { createGrpcTransport, createNamedPipeTransport, createUnixSocketTransport } from './grpc'

const DEFAULT_HTTP_ENDPOINT = 'http://127.0.0.1:9847'

export interface NodeConnectOptions extends ConnectOptions {
  /**
   * Resolves the daemon like the Rust client: `endpoint`, then
   * `AUV_ENDPOINT`, then the discovery descriptor of `auv serve`.
   * The resolved endpoint selects the transport.
   */
  discover?: boolean
  /** Environment read by `discover`; defaults to `process.env`. */
  env?: AuvEnvironment
}

/** Connects with HTTP, gRPC, local IPC, or a caller-provided transport. */
export async function connect(options: NodeConnectOptions = {}): Promise<AuvConnection> {
  if (!options.discover)
    return connectTransport(resolveTransport(options), options)

  if (options.transport !== undefined)
    throw new AuvConfigurationError('discover selects the transport; omit transport')
  const discovered = await discoverEndpoint({ endpoint: options.endpoint?.toString(), env: options.env })
  if (discovered === undefined) {
    throw new AuvConfigurationError(
      'no AUV daemon discovered; run `auv serve`, set AUV_ENDPOINT, or pass endpoint',
    )
  }
  const resolved = { ...options, ...discovered, local: options.local ?? discovered.local }
  return connectTransport(resolveTransport(resolved), resolved)
}

function resolveTransport(options: ConnectOptions): Transport {
  if (options.transport !== undefined && typeof options.transport !== 'string') {
    return options.transport
  }
  switch (options.transport ?? 'http') {
    case 'http':
      return createHttpTransport({ endpoint: options.endpoint ?? DEFAULT_HTTP_ENDPOINT })
    case 'grpc':
      return createGrpcTransport({ endpoint: options.endpoint })
    case 'npipe':
      if (options.endpoint === undefined)
        throw new Error('npipe transport requires an endpoint; use discover: true to find one')
      return createNamedPipeTransport({ path: options.endpoint })
    case 'unix':
      if (options.endpoint === undefined)
        throw new Error('unix transport requires an endpoint path; use discover: true to find one')
      return createUnixSocketTransport({ path: options.endpoint })
  }
}
