// eslint-disable-next-line no-restricted-syntax -- the napi binding is a runtime file that tsdown never bundles by this exact specifier.
import type { MacosHelperOptions, MacosHelperStatus as NativeMacosHelperStatus } from '../binding.js'

import { isMacOS } from 'std-env'

// eslint-disable-next-line no-restricted-syntax -- the napi binding is a runtime file that tsdown never bundles by this exact specifier.
import {
  installMacosHelper as installNativeMacosHelper,
  macosHelperStatus as nativeMacosHelperStatus,
  openMacosHelperAccessibilitySettings as openNativeMacosHelperAccessibilitySettings,
  openMacosHelperBackgroundItemsSettings as openNativeMacosHelperBackgroundItemsSettings,
  uninstallMacosHelper as uninstallNativeMacosHelper,
} from '../binding.js'

export type MacosHelperState
  = | 'busy'
    | 'frontend-outdated'
    | 'installed'
    | 'invalid'
    | 'not-installed'
    | 'requires-approval'
    | 'running'
    | 'unsupported'
    | 'update-required'

export type { MacosHelperOptions }

export interface MacosHelperStatus extends Omit<NativeMacosHelperStatus, 'state'> {
  state: MacosHelperState
}

/**
 * Install and register the signed helper embedded in this build, or
 * `options.helperApp` when the embedding application ships its own helper.
 */
export async function installMacosHelper(options?: MacosHelperOptions): Promise<MacosHelperStatus> {
  requireMacOS()
  return installNativeMacosHelper(options) as Promise<MacosHelperStatus>
}

/** Inspect the installed helper identity and current-user readiness. */
export async function macosHelperStatus(options?: MacosHelperOptions): Promise<MacosHelperStatus> {
  requireMacOS()
  return nativeMacosHelperStatus(options) as Promise<MacosHelperStatus>
}

/** Open System Settings at Privacy & Security > Accessibility. */
export function openMacosHelperAccessibilitySettings(): void {
  requireMacOS()
  openNativeMacosHelperAccessibilitySettings()
}

/** Open System Settings at General > Login Items & Extensions. */
export function openMacosHelperBackgroundItemsSettings(options?: MacosHelperOptions): void {
  requireMacOS()
  openNativeMacosHelperBackgroundItemsSettings(options)
}

/** Unregister the helper, reset Accessibility, and remove only its app bundle. */
export async function uninstallMacosHelper(options?: MacosHelperOptions): Promise<MacosHelperStatus> {
  requireMacOS()
  return uninstallNativeMacosHelper(options) as Promise<MacosHelperStatus>
}

function requireMacOS(): void {
  if (!isMacOS) {
    throw new Error('macOS helper setup is available only on macOS.')
  }
}
