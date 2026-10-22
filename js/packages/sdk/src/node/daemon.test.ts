import process from 'node:process'

import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { access, chmod, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'

import { isWindows } from 'std-env'
import { describe, expect, it } from 'vitest'

import { checkHealth, createAuv, createPairingToken, listDevices, pairDevice } from '../apis'
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
      noRegister: true,
      storeRoot: 'state',
      workingDirectory,
    })

    try {
      expect(daemon.pid).toBeGreaterThan(0)
      expect(daemon.storeRoot).toBe(join(workingDirectory, 'state'))
      // HTTP always requires a paired bearer, so the handle connects through
      // the owner endpoint that startAuv adds in front of the HTTP listener.
      expect(daemon.endpoints).toEqual(isWindows
        ? [expect.stringMatching(/^npipe:\/\/\.\/pipe\/auv-/), `http://127.0.0.1:${port}`]
        : [`unix://${join(workingDirectory, 'state', 'auv.sock')}`, `http://127.0.0.1:${port}`])
      expect(daemon.connectionOptions).toEqual(isWindows
        ? { credential: expect.stringMatching(/^[0-9a-f]{64}$/), endpoint: daemon.endpoints[0], local: true, transport: 'npipe' }
        : { credential: expect.stringMatching(/^[0-9a-f]{64}$/), endpoint: join(workingDirectory, 'state', 'auv.sock'), local: true, transport: 'unix' })
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
      noRegister: true,
      workingDirectory,
    })
    const connections = await Promise.all([daemon.connect(), daemon.connect()])
    try {
      const clients = connections.map(connection => createAuv(connection))
      const request = {
        frame: {
          backend: 'auv-js-reuse',
          bounds: { height: 16, width: 64, x: 0, y: 0 },
          image: { data: new Uint8Array(64 * 16 * 4).fill(255), height: 16, width: 64 },
          scaleFactor: 1,
        },
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
      noRegister: true,
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
        await expect(checkHealth(connection)).resolves.toMatchObject({ status: 'serving' })
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

  // ROOT CAUSE:
  // Startup parsed a human stdout announcement. Silencing or reformatting
  // logs made a healthy owned daemon appear unready.
  it.skipIf(isWindows)('starts through health when daemon stdout is silent', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-silent-'))
    const launcher = join(workingDirectory, 'quiet-auv')
    await writeFile(launcher, '#!/bin/sh\nexec "$AUV_TEST_BINARY" "$@" >/dev/null\n')
    await chmod(launcher, 0o700)
    let daemon: Awaited<ReturnType<typeof startAuv>> | undefined
    try {
      daemon = await startAuv({
        binaryPath: launcher,
        environment: { AUV_TEST_BINARY: join(workspace, 'target', 'debug', 'auv') },
        noRegister: true,
        startupTimeoutMs: 3000,
        workingDirectory,
      })
      const connection = await daemon.connect()
      try {
        await expect(checkHealth(connection)).resolves.toMatchObject({ status: 'serving' })
      }
      finally {
        await connection.close()
      }
    }
    finally {
      await daemon?.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it.runIf(isWindows)('uses a named pipe for the default app-owned daemon', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-npipe-'))
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      noRegister: true,
      storeRoot: 'state',
      workingDirectory,
    })

    try {
      expect(daemon.endpoints).toHaveLength(1)
      expect(daemon.endpoints[0]).toMatch(/^npipe:\/\/\.\/pipe\/auv-/)
      expect(daemon.connectionOptions).toEqual({
        credential: expect.stringMatching(/^[0-9a-f]{64}$/),
        endpoint: daemon.endpoints[0],
        local: true,
        transport: 'npipe',
      })
      const connections = await Promise.all([daemon.connect(), daemon.connect()])
      try {
        await Promise.all(connections.map(connection => expect(checkHealth(connection)).resolves.toMatchObject({ status: 'serving' })))
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

  // ROOT CAUSE:
  //
  // Local IPC admitted every process of the daemon's OS user, so a process
  // that read the socket path from `ps` could drive the Runner and mint a
  // pairing token. The launcher-only default now requires the launch token.
  it('rejects a same-user client that knows the endpoint but not the launcher token', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-launcher-token-'))
    const daemon = await startAuv({ binaryPath: join(workspace, 'target', 'debug', 'auv'), workingDirectory })
    const { credential: _, ...withoutToken } = daemon.connectionOptions
    const stranger = await connect(withoutToken)
    const owner = await daemon.connect()
    try {
      await expect(listDevices(stranger)).rejects.toThrow(/process that launched it/)
      await expect(createPairingToken(stranger)).rejects.toThrow(/process that launched it/)
      await expect(listDevices(await connect({ ...withoutToken, credential: 'f'.repeat(64) }))).rejects.toThrow(/process that launched it/)
      expect((await listDevices(owner)).some(device => device.local)).toBe(true)
      expect((await listDevices(await connect(daemon.connectionOptions))).some(device => device.local)).toBe(true)
    }
    finally {
      await Promise.all([stranger.close(), owner.close()])
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  // An app exposing its daemon to other local apps pairs each one, then can
  // revoke them one by one. Paired apps cannot mint further pairing tokens.
  it('admits paired apps on the owner endpoint under launcher-and-paired', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-paired-'))
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      localClientPolicy: 'launcher-and-paired',
      workingDirectory,
    })
    const { credential: _, ...withoutToken } = daemon.connectionOptions
    const owner = await daemon.connect()
    const otherApp = await connect(withoutToken)
    try {
      const token = await createPairingToken(owner)
      const enrollment = await pairDevice(otherApp, { deviceId: 'other-app', label: 'Other app', token })
      const paired = await connect({ ...withoutToken, credential: enrollment.credential })
      try {
        expect((await listDevices(paired)).length).toBeGreaterThan(0)
        await expect(createPairingToken(paired)).rejects.toThrow(/only be created by the daemon owner/)
      }
      finally {
        await paired.close()
      }
      await expect(listDevices(otherApp)).rejects.toThrow(/process that launched it/)
    }
    finally {
      await Promise.all([owner.close(), otherApp.close()])
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it('admits any same-user client under the any-process-of-os-user policy', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-os-user-'))
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      localClientPolicy: 'any-process-of-os-user',
      workingDirectory,
    })
    const stranger = await connect(daemon.connectionOptions)
    try {
      expect('credential' in daemon.connectionOptions).toBe(false)
      expect((await listDevices(stranger)).some(device => device.local)).toBe(true)
    }
    finally {
      await stranger.close()
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  // ROOT CAUSE:
  //
  // noRegister became true by default, but the CLI rejects --discovery-file
  // together with --no-register, so startAuv({ discoveryFile }) failed to
  // start. A discovery file now implies registration.
  it.skipIf(isWindows)('registers the daemon when a discovery file is requested', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-discovery-'))
    const discoveryFile = join(workingDirectory, 'api-server.json')
    const daemon = await startAuv({ binaryPath: join(workspace, 'target', 'debug', 'auv'), discoveryFile, workingDirectory })
    try {
      await expect(access(discoveryFile)).resolves.toBeUndefined()
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  it('reports a missing executable as a daemon start error', async () => {
    await expect(startAuv({
      binaryPath: join(tmpdir(), 'missing-auv-test-binary'),
      noRegister: true,
    })).rejects.toBeInstanceOf(AuvDaemonStartError)
  })

  // tinyexec stops the child with SIGTERM. Since #290 the daemon handles
  // SIGTERM like Ctrl-C: it shuts down gracefully, removes its socket, and
  // exits 0 instead of being killed by the signal.
  it.skipIf(isWindows)('lets tinyexec stop the daemon gracefully when its lifecycle signal aborts', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-abort-'))
    const controller = new AbortController()
    const daemon = await startAuv({
      binaryPath: join(workspace, 'target', 'debug', 'auv'),
      listeners: [`unix://${join(workingDirectory, 'auv.sock')}`],
      noRegister: true,
      signal: controller.signal,
      workingDirectory,
    })

    try {
      controller.abort()
      await expect(daemon.exited).resolves.toEqual({ code: 0, signal: null })
      await expect(access(join(workingDirectory, 'auv.sock'))).rejects.toThrow()
    }
    finally {
      await daemon.stop()
      await rm(workingDirectory, { force: true, recursive: true })
    }
  })

  // ROOT CAUSE:
  //
  // A killed owner never ran stop(), so its daemon was reparented and kept
  // running for days. The daemon now exits when the OS closes the stdin pipe
  // that startAuv holds open.
  it.skipIf(isWindows)('stops the daemon gracefully when its owner process is killed', async () => {
    const workspace = await repositoryRoot()
    const workingDirectory = await mkdtemp(join(tmpdir(), 'auv-js-owner-'))
    const owner = spawn(process.execPath, [
      '--import',
      'tsx',
      fileURLToPath(new URL('../tutils/auv-owner.ts', import.meta.url)),
      join(workspace, 'target', 'debug', 'auv'),
      workingDirectory,
    ], { stdio: ['ignore', 'pipe', 'inherit'] })
    const isAlive = (pid: number) => {
      try {
        process.kill(pid, 0)
        return true
      }
      catch {
        return false
      }
    }

    try {
      const [line] = await once(createInterface({ input: owner.stdout }), 'line') as [string]
      const pid = Number(line)
      expect(isAlive(pid)).toBe(true)

      owner.kill('SIGKILL')

      await expect.poll(() => isAlive(pid), { timeout: 15_000 }).toBe(false)
      await expect(access(join(workingDirectory, '.auv', 'store', 'auv.sock'))).rejects.toThrow()
    }
    finally {
      owner.kill('SIGKILL')
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
      noRegister: true,
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
          await expect(checkHealth(connection)).resolves.toEqual({ id: daemon.id, status: 'serving' })
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
    await expect(startAuv({ discoveryFile: 'api-server.json', noRegister: true })).rejects.toThrow('cannot be combined with noRegister: true')
    // @ts-expect-error The policy is a closed set; untyped callers still get a clear error.
    await expect(startAuv({ localClientPolicy: 'everyone' })).rejects.toThrow('unknown localClientPolicy')
  })
})
