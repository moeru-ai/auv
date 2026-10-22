import type { sendUnaryData, ServerUnaryCall } from '@grpc/grpc-js'

import { Buffer } from 'node:buffer'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { create, toBinary } from '@bufbuild/protobuf'
import { Server, ServerCredentials } from '@grpc/grpc-js'
import { isWindows } from 'std-env'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { ListDevicesResponseSchema } from '../gen/auv/api/daemon/v1/device_pb'
import { discoveryPath } from './discovery'
import { AuvConfigurationError, connect, connectFromContext, discoverEndpoint, listDevices } from './index'

describe('daemon discovery', () => {
  let directory: string
  let file: string

  beforeEach(async () => {
    directory = await mkdtemp(join(tmpdir(), 'auv-discovery-'))
    file = join(directory, 'api-server.json')
  })

  afterEach(async () => {
    await rm(directory, { force: true, recursive: true })
  })

  async function publish(descriptor: unknown): Promise<void> {
    await writeFile(file, typeof descriptor === 'string' ? descriptor : JSON.stringify(descriptor))
  }

  it('selects explicit endpoint, then AUV_ENDPOINT, then the descriptor', async () => {
    await publish({ endpoint: 'unix:///tmp/from descriptor.sock', instance_id: 'i' })
    const env = { AUV_DISCOVERY_FILE: file }

    await expect(discoverEndpoint({ env })).resolves.toEqual({
      endpoint: '/tmp/from descriptor.sock',
      local: false,
      transport: 'unix',
    })
    await expect(discoverEndpoint({
      env: { ...env, AUV_ENDPOINT: 'http://127.0.0.1:7000' },
    })).resolves.toEqual({ endpoint: 'http://127.0.0.1:7000', local: false, transport: 'grpc' })
    await expect(discoverEndpoint({
      endpoint: 'npipe://./pipe/auv-api',
      env: { ...env, AUV_ENDPOINT: 'http://127.0.0.1:7000' },
    })).resolves.toEqual({ endpoint: 'npipe://./pipe/auv-api', local: false, transport: 'npipe' })
  })

  it('treats a missing descriptor as no daemon', async () => {
    await expect(discoverEndpoint({ env: { AUV_DISCOVERY_FILE: file } })).resolves.toBeUndefined()
  })

  it('reads descriptors with or without store_root', async () => {
    const env = { AUV_DISCOVERY_FILE: file }
    await publish({ endpoint: 'http://[::1]:9847', future: 1, instance_id: 'i', store_root: '/srv/auv' })
    await expect(discoverEndpoint({ env })).resolves.toMatchObject({ endpoint: 'http://[::1]:9847' })
    await publish({ endpoint: 'http://127.0.0.1:9847', instance_id: 'old' })
    await expect(discoverEndpoint({ env })).resolves.toMatchObject({ endpoint: 'http://127.0.0.1:9847' })
  })

  it.each([
    ['{', 'not valid JSON'],
    ['[]', 'expected a JSON object'],
    ['{"instance_id":"i"}', 'endpoint must be a string'],
    ['{"endpoint":"http://127.0.0.1:1"}', 'instance_id must be a string'],
    ['{"endpoint":"http://127.0.0.1:1","instance_id":"i","store_root":1}', 'store_root must be a string'],
  ])('rejects malformed descriptor %s', async (text, message) => {
    await publish(text)
    const discovery = discoverEndpoint({ env: { AUV_DISCOVERY_FILE: file } })
    await expect(discovery).rejects.toBeInstanceOf(AuvConfigurationError)
    await expect(discovery).rejects.toThrow(message)
  })

  it('rejects a descriptor path that is not a file', async () => {
    await mkdir(file)
    await expect(discoverEndpoint({ env: { AUV_DISCOVERY_FILE: file } })).rejects.toThrow('failed to read daemon descriptor')
  })

  it.each([
    'unix://relative.sock',
    'npipe://./pipe/bad name',
    'https://127.0.0.1:9847',
    'http://127.0.0.1:9847/api',
    'http://127.0.0.1:9847?x=1',
    'not a uri',
  ])('rejects endpoint %s like the Rust parser', async (endpoint) => {
    await expect(discoverEndpoint({ endpoint, env: {} })).rejects.toThrow('invalid AUV API endpoint')
  })

  it('derives the Rust directories state path per platform', () => {
    expect(discoveryPath({ HOME: '/Users/a' }, 'darwin')).toBe(
      '/Users/a/Library/Application Support/ai.moeru.auv/api-server.json',
    )
    expect(discoveryPath({ HOME: '/home/a' }, 'linux')).toBe('/home/a/.local/state/auv/api-server.json')
    expect(discoveryPath({ HOME: '/home/a', XDG_STATE_HOME: '/state' }, 'linux')).toBe('/state/auv/api-server.json')
    expect(discoveryPath({ HOME: '/home/a', XDG_STATE_HOME: 'relative' }, 'linux')).toBe(
      '/home/a/.local/state/auv/api-server.json',
    )
    expect(discoveryPath({ LOCALAPPDATA: 'C:\\Users\\a\\AppData\\Local' }, 'win32')).toBe(
      'C:\\Users\\a\\AppData\\Local\\moeru\\auv\\data\\api-server.json',
    )
    expect(discoveryPath({ AUV_DISCOVERY_FILE: '/custom.json', HOME: '/home/a' }, 'linux')).toBe('/custom.json')
  })

  it('reports missing discovery and conflicting transports', async () => {
    const env = { AUV_DISCOVERY_FILE: file }
    await expect(connect({ discover: true, env })).rejects.toThrow('no AUV daemon discovered')
    await expect(connect({ discover: true, env, transport: 'unix' })).rejects.toThrow('discover selects the transport')
    await expect(connectFromContext({}, { env })).rejects.toThrow('no AUV daemon discovered')
    await expect(connectFromContext({ runId: 'run-1' }, { env })).rejects.toThrow(
      'AuvContext does not contain a resolved daemon_endpoint',
    )
  })

  it.skipIf(isWindows)('connects to a discovered Unix socket with the caller credential', async () => {
    const authorization: (string | undefined)[] = []
    const server = new Server()
    server.addService({
      listDevices: {
        path: '/auv.api.daemon.v1.DeviceService/ListDevices',
        requestDeserialize: value => value,
        requestSerialize: value => value,
        requestStream: false,
        responseDeserialize: value => value,
        responseSerialize: value => value,
        responseStream: false,
      },
    }, {
      listDevices(call: ServerUnaryCall<Buffer, Buffer>, callback: sendUnaryData<Buffer>) {
        authorization.push(call.metadata.get('authorization')[0]?.toString())
        callback(null, Buffer.from(toBinary(ListDevicesResponseSchema, create(ListDevicesResponseSchema, {}))))
      },
    })
    const socket = join(directory, 'auv.sock')
    await new Promise<void>((resolve, reject) => server.bindAsync(
      `unix:${socket}`,
      ServerCredentials.createInsecure(),
      error => error ? reject(error) : resolve(),
    ))
    await publish({ endpoint: `unix://${socket}`, instance_id: 'i' })

    try {
      const env = { AUV_DISCOVERY_FILE: file }
      const direct = await connect({ credential: 'device-secret', discover: true, env })
      const inherited = await connectFromContext({}, { credential: 'device-secret', env })
      expect(direct.local).toBe(false)
      await listDevices(direct)
      await listDevices(inherited)
      await direct.close()
      await inherited.close()
      expect(authorization).toEqual(['Bearer device-secret', 'Bearer device-secret'])
    }
    finally {
      server.forceShutdown()
    }
  })
})
