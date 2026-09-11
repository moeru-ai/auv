import { createServer } from 'node:net'

/** Finds an unused loopback port for a child-process integration fixture. */
export function unusedLoopbackPort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      if (address === null || typeof address === 'string') {
        server.close()
        reject(new Error('test listener did not bind an IP port'))
        return
      }
      server.close(error => error === undefined ? resolve(address.port) : reject(error))
    })
  })
}
