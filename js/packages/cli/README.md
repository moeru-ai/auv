# `@auv-js/cli`

Installs the AUV command-line executable that matches the current operating
system and CPU architecture. The executable is delivered by a platform-specific
optional npm dependency, so installation does not run a network-downloading
`postinstall` script.

```sh
pnpm add @auv-js/cli
pnpm exec auv --help
```

Node applications can resolve the installed executable explicitly:

```ts
import { binaryPath } from '@auv-js/cli/binary'
import { startAuv } from '@auv-js/sdk/node'

const daemon = await startAuv({ binaryPath: binaryPath() })
```

On macOS, the native binding exposes the same helper setup module as the CLI:

```ts
import {
  installMacosHelper,
  macosHelperStatus,
  openMacosHelperAccessibilitySettings,
  openMacosHelperBackgroundItemsSettings,
  uninstallMacosHelper,
} from '@auv-js/cli'

const before = await macosHelperStatus()
const installed = await installMacosHelper()

if (installed.state === 'requires-approval') {
  openMacosHelperBackgroundItemsSettings()
}
else if (installed.state === 'running') {
  openMacosHelperAccessibilitySettings()
}

// Removes only the helper app and its Accessibility decision. Enrollment is
// retained so a later reinstall does not require credential entry again.
await uninstallMacosHelper()
```

`installMacosHelper()` validates and installs the embedded app under the
current user's AUV Application Support directory, then registers its embedded
LaunchAgent through `SMAppService`. It needs neither `sudo` nor an
administrator password. Development bindings report `helperEmbedded=false`;
official macOS release bindings carry the same signed and notarized helper app
as the CLI.

The CLI and every package that bundles `@auv-js/cli` are independent frontends
for one per-user helper. Any helper whose declared protocol range includes this
binding's protocol is usable, whatever its version. `installMacosHelper()`
replaces it only when it is older than the embedded helper or does not support
this protocol, and never downgrades it. If the installed helper dropped this
binding's protocol, status reports `frontend-outdated`: update this package
rather than the helper.

`uninstallMacosHelper()` waits for ServiceManagement to stop any in-flight
helper process, resets the helper bundle's Accessibility decision, and removes
only the installed app. It preserves enrollment and all other AUV Application
Support content.

## Electron packaging

Do not execute the binary from inside `app.asar`. During packaging, copy the
path returned by `binaryPath()` into an unpacked, app-owned location and resolve
that copied path at runtime.

- macOS: copy it to `YourApp.app/Contents/MacOS/auv`, and list it in
  electron-builder's `mac.binaries` so it is signed as nested code.
- Windows and Linux: copy it to `resources/bin/auv.exe` or
  `resources/bin/auv` through `extraResources`.

The Electron main process should own the child process and pass the resulting
absolute path to `startAuv()`. Importing `@auv-js/cli/binary` during staging
does not load the NAPI addon. If the application also imports the root NAPI
entrypoint at runtime, keep `*.node` files outside the ASAR with the packager's
native-module/`asarUnpack` support. Keep the SDK independent of packaging policy
so browser and remote-client consumers do not install a native executable.

### Shipping your own macOS helper

An application may ship the macOS helper under its own name, icon, bundle
identifier, and Developer ID team instead of the official `AUV Helper`. Build
it with `crates/auv-device-helper-macos/package/package.sh` and its
`AUV_MACOS_HELPER_*` overrides, notarize and staple it, and embed the unpacked
app in the application bundle without re-signing it (for example
`YourApp.app/Contents/Library/Helpers/`). Pass that absolute path to both the
daemon and setup:

```ts
import path from 'node:path'
import process from 'node:process'

import { installMacosHelper } from '@auv-js/cli'
import { startAuv } from '@auv-js/sdk/node'

const helperApp = path.join(process.resourcesPath, '..', 'Library', 'Helpers', 'Your Computer Use.app')

const status = await installMacosHelper({ helperApp })
const daemon = await startAuv({ binaryPath, platforms: { macos: { helperApp } } })
```

AUV reads the bundle identifier and Team ID from that app's signature, installs
a copy under `~/Library/Application Support/<bundle identifier>/`, and the
daemon trusts only that identity. The CLI equivalent is
`auv setup macos-helper install --helper-app <path>`. See the
[helper setup reference](../../../docs/ai/references/session-api/2026-10-01-macos-helper-setup.md#shipped-helper-identity).

Supported packages currently cover macOS arm64/x64, glibc Linux arm64/x64, and
Windows x64. Installing with optional dependencies disabled leaves no binary;
`binaryPath()` reports that case with an actionable error.
