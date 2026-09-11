import { describe, expect, inject, it } from 'vitest'

import { InputService } from '../../gen/auv/api/driver/v1/input_pb'
import { connect, createAuv, invokeServerStream } from '../../web/index'

const daemon = inject('auvBrowserDaemon')

describe('typed remote invoke from a browser', () => {
  it.skipIf(!daemon.available)('rejects unauthenticated requests to the remote HTTP API', async () => {
    if (!daemon.available)
      throw new Error('browser daemon fixture is unavailable')

    const connection = await connect({ endpoint: daemon.endpoint, transport: 'http' })

    try {
      const auv = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
      await expect(auv.displays.list()).rejects.toMatchObject({
        name: 'AuvHttpError',
        status: 401,
      })
    }
    finally {
      await connection.close()
    }
  })

  // https://github.com/moeru-ai/auv/actions/runs/31709053172
  // ROOT CAUSE:
  //
  // If hosted CI had no compositor, browser fetch reached the real Runner but
  // Display enumeration failed because the ambient desktop did not exist.
  //
  // Before the fix, this test coupled routing evidence to display state. The
  // fix sends caller-owned pixels through a headless-safe Driver capability.
  it.skipIf(!daemon.available)('invokes a headless-safe real Driver capability through browser fetch', async () => {
    if (!daemon.available)
      throw new Error('browser daemon fixture is unavailable')

    const connection = await connect({
      credential: daemon.credential,
      endpoint: daemon.endpoint,
      transport: 'http',
    })

    try {
      const auv = createAuv(connection).runner({ runnerClass: 'auv.core.local' })
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
    finally {
      await connection.close()
    }
  }, 600_000)

  // https://github.com/moeru-ai/auv/actions/runs/31709053172
  // ROOT CAUSE:
  //
  // If hosted CI lacked a compositor-backed input session, browser WebSocket
  // routing succeeded but live mouse movement could not complete.
  //
  // Before the fix, the test required desktop input. The fix observes a typed
  // validation error returned by the real Runner before OS interaction.
  it.skipIf(!daemon.available)('routes a real Runner validation error through browser WebSocket', async () => {
    if (!daemon.available)
      throw new Error('browser daemon fixture is unavailable')

    const connection = await connect({
      credential: daemon.credential,
      endpoint: daemon.endpoint,
      transport: 'http',
    })

    try {
      const method = InputService.method.moveMouse
      const responses = await invokeServerStream(connection, {
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
    }
    finally {
      await connection.close()
    }
  })

  it.skipIf(!daemon.available || !daemon.isMacOS)('reflects and invokes the registered NetEase Runner through browser transports', async () => {
    if (!daemon.available)
      throw new Error('browser daemon fixture is unavailable')

    const connection = await connect({
      credential: daemon.credential,
      endpoint: daemon.endpoint,
      transport: 'http',
    })

    try {
      const runner = await createAuv(connection).runners.discover({
        runnerClass: 'auv.app.netease_music',
      })
      const nowPlaying = runner.apis.find(method => method.method === 'GetNowPlaying')
      const listSongs = runner.apis.find(method => method.method === 'ListSongs')

      expect(runner.apis).toHaveLength(14)
      expect(nowPlaying).toMatchObject({
        effect: 'read_only',
        methodKind: 'unary',
      })
      expect(listSongs).toMatchObject({
        effect: 'input',
        methodKind: 'server_streaming',
      })
      expect(listSongs?.inputSchema).toMatchObject({
        $defs: {
          'auv.netease_music.v1.ListSongsRequest': {
            properties: {
              dailyRecommended: {
                $ref: '#/$defs/auv.netease_music.v1.DailyRecommendedRef',
              },
              playlist: {
                $ref: '#/$defs/auv.netease_music.v1.PlaylistRef',
              },
            },
          },
        },
      })
      await expect(runner.invokeUnaryJson({
        input: { applicationBundleId: 'dev.auv.nonexistent-browser-player' },
        method: nowPlaying!,
      })).resolves.toEqual({})
      const responses = await runner.invokeServerStreamJson({ input: {}, method: listSongs! })
      await expect(responses[Symbol.asyncIterator]().next()).rejects.toMatchObject({
        grpcStatus: 3,
        name: 'AuvWebSocketError',
        rpcCode: 3,
      })
    }
    finally {
      await connection.close()
    }
  }, 600_000)
})
