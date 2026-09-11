import type { sendUnaryData, ServerDuplexStream, ServerUnaryCall, ServiceDefinition } from '@grpc/grpc-js'

import type { AuvAbortError } from './index'

import { Buffer } from 'node:buffer'

import { create, toBinary } from '@bufbuild/protobuf'
import { Server, ServerCredentials } from '@grpc/grpc-js'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import {
  ListDevicesRequestSchema,
  ListDevicesResponseSchema,
} from '../gen/auv/api/daemon/v1/device_pb'
import { ListDisplaysResponseSchema } from '../gen/auv/api/driver/v1/display_pb'
import {
  connect,
  createAuv,
  createGrpcTransport,
  invokeDuplex,
  listDevices,
} from './index'

describe('node gRPC transport', () => {
  const server = new Server()
  let endpoint: string

  beforeAll(async () => {
    const service: ServiceDefinition = {
      listDevices: {
        path: '/auv.api.daemon.v1.DeviceService/ListDevices',
        requestDeserialize: value => value,
        requestSerialize: value => value,
        requestStream: false,
        responseDeserialize: value => value,
        responseSerialize: value => value,
        responseStream: false,
      },
      listDisplays: {
        path: '/auv.api.driver.v1.DisplayService/ListDisplays',
        requestDeserialize: value => value,
        requestSerialize: value => value,
        requestStream: false,
        responseDeserialize: value => value,
        responseSerialize: value => value,
        responseStream: false,
      },
      slowListDevices: {
        path: '/test.Slow/Wait',
        requestDeserialize: value => value,
        requestSerialize: value => value,
        requestStream: false,
        responseDeserialize: value => value,
        responseSerialize: value => value,
        responseStream: false,
      },
      watchDevices: {
        path: '/test.Stream/Watch',
        requestDeserialize: value => value,
        requestSerialize: value => value,
        requestStream: true,
        responseDeserialize: value => value,
        responseSerialize: value => value,
        responseStream: true,
      },
    }
    server.addService(service, {
      listDevices(_call: ServerUnaryCall<Buffer, Buffer>, callback: sendUnaryData<Buffer>) {
        callback(null, Buffer.from(toBinary(ListDevicesResponseSchema, create(ListDevicesResponseSchema, {
          devices: [{
            labels: {},
            local: false,
            name: 'Remote Mac',
            platform: 2,
            ref: { deviceId: 'remote-device' },
          }],
        }))))
      },
      listDisplays(_call: ServerUnaryCall<Buffer, Buffer>, callback: sendUnaryData<Buffer>) {
        callback(null, Buffer.from(toBinary(ListDisplaysResponseSchema, create(ListDisplaysResponseSchema, {
          displays: [{ displayId: 'main', primary: true, scaleFactor: 2 }],
        }))))
      },
      slowListDevices(_call: ServerUnaryCall<Buffer, Buffer>, callback: sendUnaryData<Buffer>) {
        setTimeout(() => callback(null, Buffer.from(toBinary(
          ListDevicesResponseSchema,
          create(ListDevicesResponseSchema, { devices: [] }),
        ))), 100)
      },
      watchDevices(call: ServerDuplexStream<Buffer, Buffer>) {
        call.on('data', () => call.write(Buffer.from(toBinary(
          ListDevicesResponseSchema,
          create(ListDevicesResponseSchema, { devices: [] }),
        ))))
        call.on('end', () => call.end())
      },
    })
    const port = await new Promise<number>((resolve, reject) => {
      server.bindAsync('127.0.0.1:0', ServerCredentials.createInsecure(), (error, value) => {
        if (error)
          reject(error)
        else resolve(value)
      })
    })
    endpoint = `http://127.0.0.1:${port}`
  })

  afterAll(async () => {
    await new Promise<void>((resolve, reject) => server.tryShutdown((error) => {
      if (error)
        reject(error)
      else resolve()
    }))
  })

  it('calls the standard daemon gRPC method', async () => {
    const connection = await connect({
      transport: createGrpcTransport({ endpoint }),
    })

    await expect(listDevices(connection)).resolves.toEqual([{
      id: 'remote-device',
      labels: {},
      local: false,
      name: 'Remote Mac',
      platform: 'macos',
    }])

    await connection.close()
  })

  it('calls a generated Driver capability through the route-bound control surface', async () => {
    const connection = await connect({ transport: createGrpcTransport({ endpoint }) })

    const displays = await createAuv(connection)
      .runner({ runnerClass: 'auv.core.local' })
      .displays
      .list()

    expect(displays.map(display => display.displayId)).toEqual(['main'])
    await connection.close()
  })

  it('cancels an in-flight gRPC call with the caller signal reason', async () => {
    const connection = await connect({
      transport: createGrpcTransport({ endpoint }),
    })
    const controller = new AbortController()
    const pending = connection.unary({
      input: ListDevicesRequestSchema,
      method: '/test.Slow/Wait',
      output: ListDevicesResponseSchema,
    }, {}, { signal: controller.signal })

    controller.abort()

    await expect(pending).rejects.toEqual(expect.objectContaining<AuvAbortError>({
      message: 'AUV operation was aborted',
      name: 'AuvAbortError',
    }))
    await connection.close()
  })

  it('uses the shared typed duplex contract for gRPC streams', async () => {
    const connection = await connect({ transport: createGrpcTransport({ endpoint }) })
    const stream = await invokeDuplex(connection, {
      input: ListDevicesRequestSchema,
      method: 'Watch',
      output: ListDevicesResponseSchema,
      runnerClass: 'test.runner',
      service: 'test.Stream',
    })

    await stream.send({})
    await stream.halfClose()
    const responses = []
    for await (const response of stream.responses) responses.push(response)

    expect(responses).toHaveLength(1)
    await connection.close()
  })
})
