import { x } from 'tinyexec'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { startAuv } from './daemon'

// The process launcher is the external boundary under test. Throw before starting
// a child so the test can inspect its exact environment without a native helper.
vi.mock('tinyexec', () => ({ x: vi.fn() }))

/** @example startAuv({ platforms: { macos: { helperApp: '/Applications/App.app/Contents/Library/Helpers/Helper.app' } } }) */
describe('startAuv platform launch configuration', () => {
  const launchBoundary = new Error('captured process launch')
  const helperApp = '/Applications/Example.app/Contents/Library/Helpers/Example Computer Use.app'

  beforeEach(() => {
    vi.mocked(x).mockReset().mockImplementation(() => {
      throw launchBoundary
    })
  })

  afterEach(() => {
    vi.unstubAllEnvs()
  })

  /** @example platforms.macos.helperApp becomes the daemon's AUV_MACOS_HELPER_APP. */
  it('passes the shipped macOS helper app to the daemon', async () => {
    vi.stubEnv('AUV_MACOS_HELPER_APP', '/inherited/Helper.app')
    await expect(startAuv({
      environment: { AUV_MACOS_HELPER_APP: '/explicit/Helper.app' },
      platforms: { macos: { helperApp } },
    })).rejects.toBe(launchBoundary)
    expect(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_MACOS_HELPER_APP).toBe(helperApp)
  })

  /** @example Without platforms.macos.helperApp the daemon keeps any raw environment value. */
  it('preserves environment configuration when no helper app is supplied', async () => {
    await expect(startAuv({
      environment: { AUV_MACOS_HELPER_APP: '/explicit/Helper.app' },
      platforms: { macos: {} },
    })).rejects.toBe(launchBoundary)
    expect(vi.mocked(x).mock.calls[0]![2]!.nodeOptions!.env!.AUV_MACOS_HELPER_APP).toBe('/explicit/Helper.app')
  })
})
