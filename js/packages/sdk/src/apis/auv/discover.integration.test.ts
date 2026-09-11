import { isMacOS } from 'std-env'
import { describe, expect, it } from 'vitest'

import { connect } from '../../node/index'
import { neteaseMusicRunner, setupAuvDaemon } from '../../tutils/auv-daemon'
import { createAuv } from './client'

describe('runner discovery through a real AUV daemon', () => {
  it.skipIf(!isMacOS)('invokes discovered unary and server-streaming methods through spawned Runners', async () => {
    const daemon = await setupAuvDaemon({ runners: [neteaseMusicRunner] })
    const connection = await connect({ endpoint: daemon.ownerSocket, local: true, transport: 'unix' })

    try {
      const runner = await createAuv(connection).runners.discover({
        runnerClass: 'auv.app.netease_music',
      })
      const nowPlaying = runner.apis.find(method => method.method === 'GetNowPlaying')
      const play = runner.apis.find(method => method.method === 'Play')
      const listPlaylists = runner.apis.find(method => method.method === 'ListPlaylists')
      const listSongs = runner.apis.find(method => method.method === 'ListSongs')
      const openWindow = runner.apis.find(method => method.method === 'OpenWindow')

      expect(runner.apis.map(method => method.method).sort()).toEqual([
        'GetNowPlaying',
        'GetStatus',
        'ListPlaylists',
        'ListSongs',
        'Next',
        'OpenWindow',
        'Pause',
        'Play',
        'PlayDailyRecommended',
        'PlayPlaylist',
        'Previous',
        'Seek',
        'SelectPlaylist',
        'TogglePlayer',
      ])
      expect(Object.fromEntries(runner.apis.map(method => [method.method, method.effect]))).toEqual({
        GetNowPlaying: 'read_only',
        GetStatus: 'input',
        ListPlaylists: 'input',
        ListSongs: 'input',
        Next: 'input',
        OpenWindow: 'input',
        Pause: 'input',
        Play: 'input',
        PlayDailyRecommended: 'input',
        PlayPlaylist: 'input',
        Previous: 'input',
        Seek: 'input',
        SelectPlaylist: 'input',
        TogglePlayer: 'input',
      })
      expect(nowPlaying).toMatchObject({
        effect: 'read_only',
        methodKind: 'unary',
        service: 'auv.netease_music.v1.PlayerService',
      })
      expect([...new Set(runner.apis.map(method => method.service))].sort()).toEqual([
        'auv.netease_music.v1.ApplicationService',
        'auv.netease_music.v1.PlayerService',
        'auv.netease_music.v1.PlaylistService',
        'auv.netease_music.v1.RecommendationService',
        'auv.netease_music.v1.SongService',
      ])
      expect(play?.inputSchema).toMatchObject({
        $defs: {
          'auv.netease_music.v1.PlayRequest': {
            additionalProperties: false,
            properties: {
              applicationBundleId: { type: 'string' },
            },
            type: 'object',
          },
        },
        $ref: '#/$defs/auv.netease_music.v1.PlayRequest',
      })
      expect(openWindow?.inputSchema).toMatchObject({
        $defs: {
          'auv.netease_music.v1.OpenWindowRequest': {
            properties: {
              settleMilliseconds: { pattern: '^[0-9]+$', type: 'string' },
            },
          },
        },
      })
      expect(listSongs).toMatchObject({
        methodKind: 'server_streaming',
        service: 'auv.netease_music.v1.SongService',
      })
      expect(listPlaylists).toMatchObject({
        methodKind: 'server_streaming',
        service: 'auv.netease_music.v1.PlaylistService',
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
          'auv.netease_music.v1.PlaylistRef': {
            properties: {
              label: {
                type: 'string',
              },
              section: {
                enum: [
                  'PLAYLIST_SECTION_UNSPECIFIED',
                  'PLAYLIST_SECTION_CREATED',
                  'PLAYLIST_SECTION_FAVORITE',
                ],
                type: 'string',
              },
            },
          },
        },
      })
      await expect(runner.invokeUnaryJson({
        input: { applicationBundleId: 'dev.auv.nonexistent-test-player' },
        method: nowPlaying!,
      })).resolves.toEqual({})

      const songResponses = await runner.invokeServerStreamJson({ input: {}, method: listSongs! })
      await expect(songResponses[Symbol.asyncIterator]().next()).rejects.toMatchObject({
        name: 'AuvRpcError',
        rpcCode: 3,
      })

      const local = await createAuv(connection).runners.discover({ runnerClass: 'auv.core.local' })
      const moveMouse = local.apis.find(method => method.method === 'MoveMouse')
      expect(moveMouse).toMatchObject({
        effect: 'input',
        methodKind: 'server_streaming',
      })
      const responses = await local.invokeServerStreamJson({ input: {}, method: moveMouse! })
      await expect(responses[Symbol.asyncIterator]().next()).rejects.toMatchObject({
        name: 'AuvRpcError',
        rpcCode: 3,
      })
    }
    finally {
      await connection.close()
      await daemon.stop()
    }
  }, 600_000)
})
