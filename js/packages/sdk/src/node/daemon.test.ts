import { access, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { isWindows } from 'std-env'
import { describe, expect, it } from 'vitest'

import { checkHealth, createAuv, listDevices } from '../apis'
import { repositoryRoot } from '../tutils/dir'
import { unusedLoopbackPort } from '../tutils/port'
import { connect } from './connect'
import { AuvDaemonStartError, startAuv } from './daemon'

describe('startAuv', { timeout: 30_000 }, () => {
  // https://github.com/moeru-ai/auv/actions/runs/31747696257/job/94606069166
  // ROOT CAUSE:
  //
  // If this test stopped the daemon on Windows, Node reported the requested
  // SIGINT as the child exit signal instead of a zero exit code.
  //
  // Before the fix, the lifecycle behavior passed but its Unix-only exit
  // assertion failed. The fix preserves and checks each platform's raw result.
  it('starts, connects to, and idempotently stops an app-owned daemon', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-start-'))
    const port = await unusedLoopbackPort()
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      listeners: [`http://127.0.0.1:${port}`],
      noDiscovery: true,
      storeRoot: 'state',
      workingDirectory,
    })

    try {
      expect(daemon.pid).toBeGreaterThan(0)
      expect(daemon.storeRoot).toBe(join(workingDirectory, 'state'))
      expect(daemon.endpoints).toHaveLength(1)
      expect(daemon.connectionOptions).toEqual({
        endpoint: daemon.endpoints[0],
        local: true,
        transport: 'http',
      })
      await expect(access(daemon.storeRoot)).resolves.toBeUndefined()

      const connection = await daemon.connect()
      try {
        const devices = await listDevices(connection)
        expect(devices.some(device => device.local)).toBe(true)
      }
      finally {
        await connection.close()
      }

      const firstExit = await daemon.stop()
      const secondExit = await daemon.stop()
      expect(secondExit).toEqual(firstExit)
      expect(firstExit).toEqual(isWindows
        ? { code: null, signal: 'SIGINT' }
        : { code: 0, signal: null })
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it('reuses the daemon and local Runner while keeping explicit Runs distinct', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-reuse-'))
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      noDiscovery: true,
      workingDirectory,
    })
    const connections = await Promise.all([daemon.connect(), daemon.connect()])
    try {
      const clients = connections.map(connection => createAuv(connection))
      const request = {
        backend: 'auv-js-reuse',
        bounds: { height: 16, width: 64, x: 0, y: 0 },
        image: { data: new Uint8Array(64 * 16 * 4).fill(255), height: 16, width: 64 },
        scaleFactor: 1,
      }
      // Cold, concurrent first use must resolve to one local child.
      await Promise.all(clients.map(client => client.runner({ runnerClass: 'auv.core.local' }).recognizeText(request)))
      const initialRunners = await clients[0]!.runners.list()
      expect(initialRunners.filter(runner => runner.phase === 'ready')).toHaveLength(1)
      await Promise.all(clients.map(client => client.runner({ runnerClass: 'auv.core.local' }).recognizeText(request)))
      const finalRunners = await clients[1]!.runners.list()
      expect(finalRunners.filter(runner => runner.phase === 'ready').map(runner => runner.processId))
        .toEqual(initialRunners.filter(runner => runner.phase === 'ready').map(runner => runner.processId))
      // Capability RPCs do not manufacture Runs; operation roots create them.
      expect(await clients[0]!.runs.list()).toHaveLength(0)
      const runs = await Promise.all(clients.map(client => client.runs.create()))
      expect(new Set(runs.map(run => run.id)).size).toBe(2)
      await Promise.all(clients.map((client, index) => client.runner({ runId: runs[index]!.id, runnerClass: 'auv.core.local' }).recognizeText(request)))
      await clients[0]!.runs.stop({ outcome: 'succeeded', runId: runs[0]!.id })
      expect((await clients[1]!.runs.get({ runId: runs[1]!.id })).phase).toBe('running')
      await clients[1]!.runs.stop({ outcome: 'succeeded', runId: runs[1]!.id })
      expect((await clients[0]!.runs.list()).map(run => run.id).sort()).toEqual(runs.map(run => run.id).sort())
      expect((await clients[0]!.runners.list()).filter(runner => runner.phase === 'ready').map(runner => runner.processId))
        .toEqual(initialRunners.filter(runner => runner.phase === 'ready').map(runner => runner.processId))
    }
    finally {
      await Promise.all(connections.map(connection => connection.close()))
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  }, 60_000)

  // ROOT CAUSE:
  // An existing listener can answer health before a newly spawned child fails
  // to bind it. Startup must never return ownership of that existing daemon.
  it('rejects a second start on an occupied endpoint and preserves the first daemon', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-occupied-'))
    const port = await unusedLoopbackPort()
    const options = {
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      listeners: [`http://127.0.0.1:${port}`],
      noDiscovery: true,
      workingDirectory,
    }
    const first = await startAuv(options)
    let second: Awaited<ReturnType<typeof startAuv>> | undefined
    try {
      await expect(startAuv(options).then((value) => {
        second = value
        return value
      })).rejects.toBeInstanceOf(AuvDaemonStartError)
      const connection = await first.connect()
      try {
        await expect(checkHealth(connection)).resolves.toBe('serving')
      }
      finally {
        await connection.close()
      }
    }
    finally {
      await second?.stop()
      await first.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it.runIf(isWindows)('uses a named pipe for the default app-owned daemon', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-npipe-'))
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      noDiscovery: true,
      storeRoot: 'state',
      workingDirectory,
    })

    try {
      expect(daemon.endpoints).toHaveLength(1)
      expect(daemon.endpoints[0]).toMatch(/^npipe:\/\/\.\/pipe\/auv-/)
      expect(daemon.connectionOptions).toEqual({
        endpoint: daemon.endpoints[0],
        local: true,
        transport: 'npipe',
      })
      const connections = await Promise.all([daemon.connect(), daemon.connect()])
      try {
        await Promise.all(connections.map(connection => expect(checkHealth(connection)).resolves.toBe('serving')))
      }
      finally {
        await Promise.all(connections.map(connection => connection.close()))
      }
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it('reports a missing executable as a daemon start error', async () => {
    await expect(startAuv({
      binaryPath: join(tmpdir(), 'missing-auv-test-binary'),
      noDiscovery: true,
    })).rejects.toBeInstanceOf(AuvDaemonStartError)
  })

  it.skipIf(isWindows)('lets tinyexec stop the daemon when its lifecycle signal aborts', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-abort-'))
    const controller = new AbortController()
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      listeners: [`unix://${join(workingDirectory, 'auv.sock')}`],
      noDiscovery: true,
      signal: controller.signal,
      workingDirectory,
    })

    try {
      controller.abort()
      await expect(daemon.exited).resolves.toMatchObject({ code: null })
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it.skipIf(isWindows)('exposes public health over Unix, gRPC, and HTTP', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-health-'))
    const socket = join(workingDirectory, 'auv.sock')
    const port = await unusedLoopbackPort()
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      listeners: [`unix://${socket}`, `http://127.0.0.1:${port}`],
      noDiscovery: true,
      pairingStore: join(workingDirectory, 'pairings.json'),
      workingDirectory,
    })

    try {
      const httpEndpoint = daemon.endpoints.find(endpoint => endpoint.startsWith('http://'))!
      for (const options of [
        { endpoint: socket, local: true, transport: 'unix' as const },
        { endpoint: httpEndpoint, local: false, transport: 'grpc' as const },
        { endpoint: httpEndpoint, local: false, transport: 'http' as const },
      ]) {
        const connection = await connect(options)
        try {
          await expect(checkHealth(connection)).resolves.toBe('serving')
        }
        finally {
          await connection.close()
        }
      }
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it('rejects invalid lifecycle configuration before spawning', async () => {
    await expect(startAuv({ startupTimeoutMs: Number.NaN })).rejects.toThrow('startupTimeoutMs must be finite')
    await expect(startAuv({ shutdownTimeoutMs: 0 })).rejects.toThrow('shutdownTimeoutMs must be greater than 0')
    await expect(startAuv({ daemonIdleTimeoutSeconds: 1.5 })).rejects.toThrow('daemonIdleTimeoutSeconds must be an integer')
    await expect(startAuv({ listeners: ['http://127.0.0.1:0'] })).rejects.toThrow('port 0 cannot be used as a connection endpoint')
  })
})
