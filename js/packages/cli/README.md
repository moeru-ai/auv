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

Supported packages currently cover macOS arm64/x64, glibc Linux arm64/x64, and
Windows x64. Installing with optional dependencies disabled leaves no binary;
`binaryPath()` reports that case with an actionable error.
