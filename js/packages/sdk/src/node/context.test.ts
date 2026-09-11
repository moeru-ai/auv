import type { Transport, UnaryCall } from '../transport/types'

import { create, toBinary } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'

import { ListDisplaysResponseSchema } from '../gen/auv/api/driver/v1/display_pb'
import {
  AuvConfigurationError,
  connectFromContext,
  contextFromEnv,
  createAuv,
} from './index'

describe('auv context', () => {
  it('parses a caller-provided environment object as additive non-secret context', () => {
    const context = contextFromEnv({
      AUV_CONTEXT: JSON.stringify({
        daemon_endpoint: 'unix:///tmp/auv.sock',
        device_id: 'device-1',
        device_name: 'Studio Mac',
        future_field: true,
        invocation_id: 'invocation-1',
        run_id: 'run-1',
      }),
    })

    expect(context).toEqual({
      configProfile: undefined,
      daemonEndpoint: 'unix:///tmp/auv.sock',
      deviceId: 'device-1',
      deviceName: 'Studio Mac',
      invocationId: 'invocation-1',
      runId: 'run-1',
    })
    expect(Reflect.get(context, 'futureField')).toBeUndefined()
    expect(Reflect.get(context, 'credential')).toBeUndefined()
  })

  it('rejects unavailable, malformed, and structurally invalid context', () => {
    expect(() => contextFromEnv({})).toThrow('AUV_CONTEXT is unavailable')
    expect(() => contextFromEnv({ AUV_CONTEXT: '{' })).toThrow('AUV_CONTEXT is not valid JSON')
    expect(() => contextFromEnv({ AUV_CONTEXT: '[]' })).toThrow('AUV_CONTEXT must be a JSON object')
    expect(() => contextFromEnv({ AUV_CONTEXT: '{"run_id":1}' })).toThrow(
      'AUV_CONTEXT.run_id must be a string',
    )
  })

  it('inherits canonical Device and Run placement for routed operations', async () => {
    const calls: UnaryCall[] = []
    const transport = recordingTransport(calls)
    const context = contextFromEnv({
      AUV_CONTEXT: JSON.stringify({
        daemon_endpoint: 'unix:///tmp/auv.sock',
        device_id: 'device-1',
        device_name: 'display-only-name',
        run_id: 'run-1',
      }),
    })
    const connection = await connectFromContext(context, { transport })

    await createAuv(connection).runner({ runnerClass: 'auv.core.local' }).displays.list()

    expect(calls).toHaveLength(1)
    expect(calls[0]!.headers.get('auv-device-id')).toBe('device-1')
    expect(calls[0]!.headers.get('auv-run-id')).toBe('run-1')
    expect(calls[0]!.headers.get('auv-device-name')).toBeNull()
  })

  it('rejects an explicit route that conflicts with inherited context before dispatch', async () => {
    const calls: UnaryCall[] = []
    const connection = await connectFromContext({
      daemonEndpoint: 'unix:///tmp/auv.sock',
      deviceId: 'device-1',
      runId: 'run-1',
    }, { transport: recordingTransport(calls) })

    const operation = createAuv(connection).runner({
      deviceId: 'different-device',
      runnerClass: 'auv.core.local',
    }).displays.list()

    await expect(operation).rejects.toBeInstanceOf(AuvConfigurationError)

    const runConflict = createAuv(connection).runner({
      runId: 'different-run',
      runnerClass: 'auv.core.local',
    }).displays.list()

    await expect(runConflict).rejects.toBeInstanceOf(AuvConfigurationError)
    expect(calls).toHaveLength(0)
  })

  it('keeps profile credential lookup application-owned', async () => {
    await expect(connectFromContext({
      configProfile: 'remote-studio',
      daemonEndpoint: 'https://remote.example',
    }, { transport: recordingTransport([]) })).rejects.toThrow(
      'AuvContext config_profile requires an application-owned credential',
    )
  })

  it('requires the parent to resolve a daemon endpoint', async () => {
    await expect(connectFromContext({}, { transport: recordingTransport([]) })).rejects.toThrow(
      'AuvContext does not contain a resolved daemon_endpoint',
    )
  })
})

function recordingTransport(calls: UnaryCall[]): Transport {
  return {
    close() {},
    async connect() {},
    async duplex() {
      throw new Error('unexpected duplex operation')
    },
    async unary(call) {
      calls.push(call)
      return toBinary(ListDisplaysResponseSchema, create(ListDisplaysResponseSchema, {
        displays: [],
      }))
    },
  }
}
