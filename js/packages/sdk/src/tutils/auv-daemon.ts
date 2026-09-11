import type { AuvConnection } from '../node/index'

import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { isAbsolute, join, resolve } from 'node:path'

import { Format, LogLevel, setGlobalFormat, setGlobalLogLevel, useLogg } from '@guiiai/logg'

import { connect, createPairingToken, listDevices, pairDevice, startAuv } from '../node/index'
import { repositoryRoot } from './dir'
import { unusedLoopbackPort } from './port'

setGlobalFormat(Format.Pretty)
setGlobalLogLevel(LogLevel.Debug)

export interface AuvDaemonFixture {
  readonly ownerSocket: string
  readonly remoteEndpoint: string
  stop: () => Promise<void>
}

export interface AuvDaemonFixtureOptions {
  readonly runners?: readonly RunnerProvider[]
}

export interface RunnerProvider {
  readonly args?: readonly string[]
  readonly class: string
  readonly env?: Readonly<Record<string, string>>
  readonly executable: string
  readonly workingDirectory?: string
}

export const neteaseMusicRunner = {
  class: 'auv.app.netease_music',
  executable: join('target', 'debug', 'auv-runner-netease-music'),
} satisfies RunnerProvider

export interface PairedAuvDaemonFixture extends AuvDaemonFixture {
  readonly connection: AuvConnection
  readonly credential: string
  readonly deviceId: string
  readonly localDeviceId: string
}

/** Starts an isolated AUV daemon with local-owner and paired-bearer listeners. */
export async function setupAuvDaemon(options: AuvDaemonFixtureOptions = {}): Promise<AuvDaemonFixture> {
  const log = useLogg('setup:auv-daemon').useGlobalConfig()

  const workspace = await repositoryRoot()
  const root = await mkdtemp(join(tmpdir(), 'auv-js-daemon-'))
  const ownerSocket = join(root, 'auv.sock')
  const remotePort = await unusedLoopbackPort()
  const runnerProviders = await Promise.all((options.runners ?? []).map(async (provider, index) => {
    const manifest = join(root, `runner-provider-${index}.json`)
    await writeFile(manifest, JSON.stringify({
      runner_class: provider.class,
      runtime: {
        config: {
          arguments: provider.args ?? [],
          environment: provider.env ?? {},
          executable: workspacePath(workspace, provider.executable),
          working_directory: workspacePath(workspace, provider.workingDirectory ?? '.'),
        },
        type: 'executable',
      },
    }))
    return manifest
  }))

  log.withFields({ root }).log('starting AUV daemon for testing')

  const daemon = await startAuv({
    binaryPath: join(workspace, 'target', 'debug', 'auv'),
    listeners: [`unix://${ownerSocket}`, `http://127.0.0.1:${remotePort}`],
    noDiscovery: true,
    pairingStore: join(root, 'pairings.json'),
    runnerProviders,
    storeRoot: join(root, 'store'),
    workingDirectory: workspace,
  })

  log.withFields({ ownerSocket }).log('AUV daemon started for testing')

  const remoteEndpoint = daemon.endpoints.find(endpoint => endpoint.startsWith('http://'))
  if (remoteEndpoint === undefined) {
    await daemon.stop()
    await rm(root, { force: true, recursive: true })
    throw new Error('AUV daemon fixture did not bind its remote HTTP listener')
  }

  const stop = async () => {
    try {
      await daemon.stop()
    }
    finally {
      await rm(root, { force: true, recursive: true })
    }

    log.withFields({ ownerSocket }).log('AUV daemon stopped')
  }

  log.withFields({ endpoint: remoteEndpoint, ownerSocket }).log('AUV daemon ready for testing')

  return { ownerSocket, remoteEndpoint, stop }
}

/** Starts an isolated daemon and returns an authenticated paired-Device connection. */
export async function setupPairedAuvDaemon(deviceId = 'auv-js-test-device', options: AuvDaemonFixtureOptions = {}): Promise<PairedAuvDaemonFixture> {
  const log = useLogg('setup:auv-daemon-pairing').useGlobalConfig()

  const daemon = await setupAuvDaemon(options)
  try {
    log.debug('connecting to started AUV daemon for pairing and enrollment')
    const owner = await connect({ endpoint: daemon.ownerSocket, local: true, transport: 'unix' })

    log.log('creating pairing token as owner')
    const token = await createPairingToken(owner)
    log.withFields({ deviceId, token: redacted(token.value) }).log('enrolled successfully')

    await owner.close()

    log.withField('endpoint', daemon.remoteEndpoint).debug('connecting to started AUV daemon and pair for test Device')
    const bootstrap = await connect({ endpoint: daemon.remoteEndpoint, transport: 'http' })

    log.log('paring test Device')
    const enrollment = await pairDevice(bootstrap, { deviceId, label: '@auv-js/sdk test Device', token })
    log.log('paired successfully')

    await bootstrap.close()

    log.withField('endpoint', daemon.remoteEndpoint).debug('connecting to started AUV daemon with test Device only credential')
    const connection = await connect({ credential: enrollment.credential, endpoint: daemon.remoteEndpoint, transport: 'http' })

    log.withField('endpoint', daemon.remoteEndpoint).debug('testing with list devices')
    const localDevice = (await listDevices(connection)).find(device => device.local)
    if (localDevice === undefined)
      throw new Error('AUV daemon fixture did not expose its local Device')

    return {
      ...daemon,
      connection,
      credential: enrollment.credential,
      deviceId: enrollment.deviceId,
      localDeviceId: localDevice.id,
      async stop() {
        await connection.close()
        await daemon.stop()
      },
    }
  }
  catch (error) {
    await daemon.stop()
    throw error
  }
}

function redacted(str: string, startPad: number = 4, endPad: number = 0) {
  const maskLength = Math.max(0, str.length - startPad - endPad)

  return (
    str.slice(0, startPad)
    + '•'.repeat(maskLength)
    + (endPad > 0 ? str.slice(-endPad) : '')
  )
}

function workspacePath(workspace: string, path: string): string {
  return isAbsolute(path) ? path : resolve(workspace, path)
}
