import type { OverlayTheme } from './index'

import process from 'node:process'

import { x } from 'tinyexec'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { startAuv } from './daemon'

// The process launcher is the external boundary under test. Throw before starting
// a child so the test can inspect its exact environment without a native display.
vi.mock('tinyexec', () => ({ x: vi.fn() }))

/** @example startAuv({ overlay: { theme: { outlineColor: '#336699' } } }) */
describe('startAuv overlay launch configuration', () => {
  const launchBoundary = new Error('captured process launch')

  beforeEach(() => {
    vi.mocked(x).mockReset().mockImplementation(() => {
      throw launchBoundary
    })
  })

  afterEach(() => {
    vi.unstubAllEnvs()
  })

  /** @example Nested camelCase keys become native snake_case while SVG viewBox remains intact. */
  it('serializes every theme field and leaves caller values intact', async () => {
    const source = '<svg viewBox="0 0 24 24"><path id="cursorBody" fill="#49E3E4"/></svg>'
    const theme: OverlayTheme = {
      cursorImage: { kind: 'svg', source },
      cursorLabelBackground: '#336699',
      cursorLabelForeground: '#ffffff',
      cursorShadow: {
        blurRadius: 8,
        color: { alpha: 0.55, blue: 253 / 255, green: 1, red: 206 / 255 },
        offsetX: 0,
        offsetY: 2,
      },
      outlineColor: { alpha: 1, blue: 0.6, green: 0.4, red: 0.2 },
      statusBackground: '#202020ee',
      statusForeground: '#ffffff',
    }
    const original = structuredClone(theme)
    vi.stubEnv('AUV_OVERLAY_THEME', '{"outline_color":"#111111"}')
    const environment = { AUV_OVERLAY_THEME: '{"outline_color":"#222222"}', AUV_THEME_TEST: 'retained' }

    await expect(startAuv({ environment, overlay: { theme } })).rejects.toBe(launchBoundary)
    const env = vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!
    expect(JSON.parse(env.AUV_OVERLAY_THEME!)).toEqual({
      cursor_image: { kind: 'svg', source },
      cursor_label_background: '#336699',
      cursor_label_foreground: '#ffffff',
      cursor_shadow: { blur_radius: 8, color: theme.cursorShadow!.color, offset_x: 0, offset_y: 2 },
      outline_color: theme.outlineColor,
      status_background: '#202020ee',
      status_foreground: '#ffffff',
    })
    expect(env.AUV_THEME_TEST).toBe('retained')
    expect(theme).toEqual(original)
    expect(environment.AUV_OVERLAY_THEME).toBe('{"outline_color":"#222222"}')
    expect(process.env.AUV_OVERLAY_THEME).toBe('{"outline_color":"#111111"}')
  })

  /** @example builtIn / auvClick are native enum values built_in / auv_click. */
  it('maps built-in cursor enum values separately from object keys', async () => {
    await expect(startAuv({
      overlay: { theme: { cursorImage: { kind: 'builtIn', variant: 'auvClick' } } },
    })).rejects.toBe(launchBoundary)
    expect(JSON.parse(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_OVERLAY_THEME!)).toEqual({
      cursor_image: { kind: 'built_in', variant: 'auv_click' },
    })
  })

  /** @example overlay.theme = {} clears inherited and explicit environment themes. */
  it('replaces environment themes with an explicitly empty theme', async () => {
    vi.stubEnv('AUV_OVERLAY_THEME', '{"outline_color":"#111111"}')
    await expect(startAuv({
      environment: { AUV_OVERLAY_THEME: '{"outline_color":"#222222"}' },
      overlay: { theme: {} },
    })).rejects.toBe(launchBoundary)
    expect(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_OVERLAY_THEME).toBe('{}')
  })

  /** @example Missing overlay.theme preserves raw environment configuration. */
  it('preserves explicit environment configuration when no typed theme is supplied', async () => {
    const raw = '{"outline_color":"#336699"}'
    await expect(startAuv({ environment: { AUV_OVERLAY_THEME: raw }, overlay: {} })).rejects.toBe(launchBoundary)
    expect(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_OVERLAY_THEME).toBe(raw)
  })

  /** @example startAuv() inherits a parent process theme without rewriting it. */
  it('preserves inherited environment configuration', async () => {
    const raw = '{"outline_color":"#336699"}'
    vi.stubEnv('AUV_OVERLAY_THEME', raw)
    await expect(startAuv()).rejects.toBe(launchBoundary)
    expect(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_OVERLAY_THEME).toBe(raw)
  })
})
