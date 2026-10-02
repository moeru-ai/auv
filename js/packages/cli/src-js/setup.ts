import type { MacosHelperStatus as NativeMacosHelperStatus } from '../binding.js'

import { isMacOS } from 'std-env'

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

export interface MacosHelperStatus extends Omit<NativeMacosHelperStatus, 'state'> {
  state: MacosHelperState
}

/** Install and register the signed helper embedded in this build. */
export async function installMacosHelper(): Promise<MacosHelperStatus> {
  requireMacOS()
  return installNativeMacosHelper() as Promise<MacosHelperStatus>
}

/** Inspect the installed AUV Helper identity and current-user readiness. */
export async function macosHelperStatus(): Promise<MacosHelperStatus> {
  requireMacOS()
  return nativeMacosHelperStatus() as Promise<MacosHelperStatus>
}

/** Open System Settings at Privacy & Security > Accessibility. */
export function openMacosHelperAccessibilitySettings(): void {
  requireMacOS()
  openNativeMacosHelperAccessibilitySettings()
}

/** Open System Settings at General > Login Items & Extensions. */
export function openMacosHelperBackgroundItemsSettings(): void {
  requireMacOS()
  openNativeMacosHelperBackgroundItemsSettings()
}

/** Unregister the helper, reset Accessibility, and remove only its app bundle. */
export async function uninstallMacosHelper(): Promise<MacosHelperStatus> {
  requireMacOS()
  return uninstallNativeMacosHelper() as Promise<MacosHelperStatus>
}

function requireMacOS(): void {
  if (!isMacOS) {
    throw new Error('AUV Helper setup is available only on macOS.')
  }
}
