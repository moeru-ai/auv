import type { RunPhase as ProtoRunPhase } from '../../gen/auv/api/daemon/v1/run_pb'
import type { Transport } from '../../web/index'

import { create, toBinary } from '@bufbuild/protobuf'
import { isWindows } from 'std-env'
import { describe, expect, it } from 'vitest'

import { CreateRunResponseSchema } from '../../gen/auv/api/daemon/v1/run_pb'
import { createRunner, deleteRunner, listRunnerClasses } from '../../node/index'
import { setupPairedAuvDaemon } from '../../tutils/auv-daemon'
import { AuvProtocolError, connect, createRun } from '../../web/index'

describe('run and Runner operations', () => {
  it('rejects successful resource responses that omit their required resource', async () => {
    const transport: Transport = {
      close() {},
      async connect() {},
      async duplex() { throw new Error('unexpected dispatch') },
      async unary() {
        return toBinary(CreateRunResponseSchema, create(CreateRunResponseSchema))
      },
    }
    const connection = await connect({ transport })

    await expect(createRun(connection)).rejects.toEqual(
      new AuvProtocolError('AUV response omitted CreateRunResponse.run'),
    )
  })

  it('rejects unknown resource enum values instead of passing them through', async () => {
    const transport: Transport = {
      close() {},
      async connect() {},
      async duplex() { throw new Error('unexpected dispatch') },
      async unary() {
        return toBinary(CreateRunResponseSchema, create(CreateRunResponseSchema, {
          run: {
            devices: [],
            phase: 99 as ProtoRunPhase,
            ref: { runId: 'run-a' },
          },
        }))
      },
    }
    const connection = await connect({ transport })

    await expect(createRun(connection)).rejects.toEqual(
      new AuvProtocolError('AUV response returned unknown Run.phase value 99'),
    )
  })
})

describe.skipIf(isWindows)('run and Runner operations against an AUV daemon', () => {
  it('creates a Run with explicit Device placement and labels', async () => {
    const daemon = await setupPairedAuvDaemon('run-device')
    try {
      const run = await createRun(daemon.connection, {
        deviceIds: [daemon.localDeviceId],
        labels: { purpose: 'test' },
      })

      expect(run.id).not.toBe('')
      expect(run.deviceIds).toEqual([daemon.localDeviceId])
      expect(run.labels).toEqual({ purpose: 'test' })
      expect(run.phase).toBe('running')
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)

  it('creates and deletes a first-party Runner without inventing optional values', async () => {
    const daemon = await setupPairedAuvDaemon('runner-device')
    try {
      const runner = await createRunner(daemon.connection, {
        lifecycle: 'ephemeral',
        runnerClass: 'auv.core.local',
      })

      expect(runner.id).not.toBe('')
      expect(runner.idleDeadline).toBeUndefined()
      expect(runner.idleTimeoutMs).toBeUndefined()
      expect(runner.lifecycle).toBe('ephemeral')
      expect(runner.phase).toBe('ready')
      expect(runner.runnerClass).toBe('auv.core.local')

      await expect(deleteRunner(daemon.connection, { force: true, runnerId: runner.id })).resolves.toMatchObject({
        id: runner.id,
        phase: 'stopped',
      })
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)

  it('lists the canonical first-party RunnerClass', async () => {
    const daemon = await setupPairedAuvDaemon('runner-class-device')
    try {
      await expect(listRunnerClasses(daemon.connection)).resolves.toContainEqual(expect.objectContaining({
        available: true,
        id: 'auv.core.local',
      }))
    }
    finally {
      await daemon.stop()
    }
  }, 30_000)
})
