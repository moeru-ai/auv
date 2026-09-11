import type { AuvConnection, ConnectOptions } from '../transport/connection'
import type { Transport } from '../transport/types'

import { connectTransport } from '../transport/connection'
import { createHttpTransport } from './http'

const DEFAULT_HTTP_ENDPOINT = 'http://127.0.0.1:9847'

/** Connects with the browser-safe HTTP transport or a caller-provided transport. */
export async function connect(options: ConnectOptions = {}): Promise<AuvConnection> {
  return connectTransport(resolveTransport(options), options)
}

function resolveTransport(options: ConnectOptions): Transport {
  if (options.transport !== undefined && typeof options.transport !== 'string') {
    return options.transport
  }

  const transport = options.transport ?? 'http'
  if (transport === 'http') {
    return createHttpTransport({ endpoint: options.endpoint ?? DEFAULT_HTTP_ENDPOINT })
  }

  throw new Error(`${transport} transport is available from 'auv-js/node'`)
}
