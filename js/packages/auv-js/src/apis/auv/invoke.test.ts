import type { Transport } from '../../transport/types'

import { isWindows } from 'std-env'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import {
  ListDisplaysRequestSchema,
  ListDisplaysResponseSchema,
} from '../../gen/auv/api/driver/v1/display_pb'
import { InputService } from '../../gen/auv/api/driver/v1/input_pb'
import {
  AuvConfigurationError,
  connect,
  createAuv,
  createPairingToken,
  invokeServerStream,
  invokeUnary,
  pairDevice,
} from '../../node/index'
import { setupAuvDaemon } from '../../tutils/auv-daemon'

describe('typed remote invoke', () => {
  it('rejects explicit Device placement on a local-only connection before dispatch', async () => {
    let dispatched = false
    const transport: Transport = {
      close() {},
      async connect() {},
      async duplex() { throw new Error('unexpected dispatch') },
      async unary() {
        dispatched = true
        throw new Error('unexpected dispatch')
      },
    }
    const connection = await connect({ local: true, transport })

    await expect(invokeUnary(connection, {
      deviceId: 'remote-device',
      input: ListDisplaysRequestSchema,
      method: 'ListDisplays',
      output: ListDisplaysResponseSchema,
      request: {},
      runnerClass: 'auv.core.local',
      service: 'auv.api.driver.v1.DisplayService',
    })).rejects.toEqual(new AuvConfigurationError('local connection cannot select an explicit Device'))
    expect(dispatched).toBe(false)
  })
})

// https://github.com/moeru-ai/auv/actions/runs/31747696257/job/94606069166
// ROOT CAUSE:
//
// If Vitest skipped this suite on Windows, its async collection callback still
// started the Unix-socket fixture before any test could be skipped.
//
// Before the fix, collection failed on an invalid Windows Unix-socket URL. The
// fix keeps fixture side effects in hooks that do not run for a skipped suite.
describe.skipIf(isWindows)('invoke against an authenticated AUV daemon', () => {
  let credential = ''
  let daemon: Awaited<ReturnType<typeof setupAuvDaemon>> | undefined

  beforeAll(async () => {
    daemon = await setupAuvDaemon()

    const owner = await connect({ endpoint: daemon.ownerSocket, local: true, transport: 'unix' })
    const token = await createPairingToken(owner, { ttlMs: 60_000 })
    await owner.close()

    const bootstrap = await connect({ endpoint: daemon.remoteEndpoint, transport: 'http' })
    const enrollment = await pairDevice(bootstrap, { deviceId: 'auv-js-integration', label: 'auv-js integration test', token })
    credential = enrollment.credential
    await bootstrap.close()
  })

  afterAll(async () => {
    await daemon?.stop()
  })

  it('rejects unauthenticated requests to the remote HTTP API', async () => {
    const unauthenticated = await connect({ endpoint: daemon!.remoteEndpoint, transport: 'http' })
    const auv = createAuv(unauthenticated).runner({ runnerClass: 'auv.core.local' })
    await expect(auv.displays.list()).rejects.toMatchObject({ name: 'AuvHttpError', status: 401 })
    await unauthenticated.close()
  })

  // https://github.com/moeru-ai/auv/actions/runs/31709053172
  // ROOT CAUSE:
  //
  // If hosted CI had no compositor or only one attached display, the routed
  // Display request failed or its multi-display assertion rejected a valid
  // environment even though remote Runner routing had succeeded.
  //
  // Before the fix, this test coupled routing evidence to ambient display
  // state. The fix sends caller-owned pixels through the same real Runner.
  it('pairs a Device and invokes a headless-safe real Runner capability through the remote HTTP API', async () => {
    const paired = await connect({ credential, endpoint: daemon!.remoteEndpoint, transport: 'http' })

    {
      const auv = createAuv(paired).runner({ runnerClass: 'auv.core.local' })
      const recognition = await auv.recognizeText({
        backend: 'auv-js-integration',
        bounds: { height: 16, width: 64, x: 0, y: 0 },
        image: {
          data: new Uint8Array(64 * 16 * 4).fill(255),
          height: 16,
          width: 64,
        },
        scaleFactor: 1,
      })
      expect(recognition.$typeName).toBe('auv.api.driver.v1.RecognizeTextResponse')
      expect(recognition.text).toBe('')
      expect(recognition.regions).toEqual([])
    }

    await paired.close()
  }, 600_000)

  // https://github.com/moeru-ai/auv/actions/runs/31709053172
  // ROOT CAUSE:
  //
  // If hosted CI lacked a compositor-backed input session, successful remote
  // mouse movement could not run even though WebSocket routing was healthy.
  //
  // Before the fix, the test required live desktop input. The fix observes a
  // typed validation error returned by the real Runner before OS interaction.
  it('routes a real Runner validation error through the remote WebSocket API', async () => {
    const paired = await connect({ credential, endpoint: daemon!.remoteEndpoint, transport: 'http' })

    const method = InputService.method.moveMouse
    const responses = await invokeServerStream(paired, {
      input: method.input,
      method: method.name,
      output: method.output,
      request: {},
      runnerClass: 'auv.core.local',
      service: method.parent.typeName,
    })

    await expect(responses[Symbol.asyncIterator]().next()).rejects.toMatchObject({
      grpcStatus: 3,
      name: 'AuvWebSocketError',
      rpcCode: 3,
    })

    await paired.close()
  })
})
