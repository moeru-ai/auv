# AUV TypeScript SDK

TypeScript SDK for AUV Device, pairing, Run, Runner, and routed capability
operations. The package is function-first for tree shaking and also provides a
namespaced client over the same functions.

<!-- START doctoc generated TOC please keep comment here to allow auto update -->
<!-- DON'T EDIT THIS SECTION, INSTEAD RE-RUN doctoc TO UPDATE -->
## Table of Contents

- [Installation](#installation)
- [Browser and universal JavaScript](#browser-and-universal-javascript)
- [Node.js and Electron](#nodejs-and-electron)
- [Pairing](#pairing)
- [Call Runner capabilities](#call-runner-capabilities)
- [Typed capability invocation](#typed-capability-invocation)
- [Discover extension operations](#discover-extension-operations)
- [Cancellation](#cancellation)
- [Tests](#tests)

<!-- END doctoc generated TOC please keep comment here to allow auto update -->

## Installation

```sh
npm install @auv-js/sdk
```

```sh
pnpm add @auv-js/sdk
```

## Browser and universal JavaScript

```ts
import { connect, createAuv, createHttpTransport } from '@auv-js/sdk'

const connection = await connect({
  credential,
  transport: createHttpTransport({ endpoint: 'http://127.0.0.1:9847' }),
  // If you need timeout control, supply an AbortSignal.
  // signal: controller.signal,
})
const auv = createAuv(connection)

const devices = await auv.devices.list()
const run = await auv.runs.create({ deviceIds: [devices[0]!.id] })
```

`transport: 'http'` selects the same HTTP transport with the default local
endpoint. By default, AUV daemon use ProtoJSON, while dynamic routed invoke
uses opaque Protobuf payloads. If an error occurs, it is available as
`AuvHttpError` values.

### Supported transports

- `createHttpTransport` — HTTP transport for browsers and Node.js
- `createWebSocketTransport` — WebSocket transport for browsers and Node.js
- `createUnixSocketTransport` — Unix socket transport for Node.js
- `createGrpcTransport` — gRPC transport for Node.js

## Node.js and Electron

### Connect through Unix socket / Named pipe (for Windows)

Since if no `--listen` specified when bootstrapping AUV daemon, by default it listens on a Unix socket or Windows named pipe. The SDK can connect to that socket or pipe directly without HTTP or WebSocket overhead:

```ts
import { connect, createAuv, createUnixSocketTransport } from 'auv-js/node'

const connection = await connect({
  transport: createUnixSocketTransport({ path: '/absolute/path/to/auv.sock' }),
})
const auv = createAuv(connection)
```

Yet if needed, gRPC can be connected too through `createGrpcTransport`.

### Connect and start the daemon

For Electron, if you wished to embed the AUV daemon and offer computer use capabilities without requiring the user to install it separately, you can start the daemon from your main process and connect to it:

```ts
import { join } from 'node:path'

import { createAuv, startAuv } from 'auv-js/node'
import { app } from 'electron'

const daemon = await startAuv({
  binaryPath: join(process.resourcesPath, 'bin', 'auv'),
  listeners: ['http://127.0.0.1:9847'],
  noDiscovery: true,
  storeRoot: join(app.getPath('userData'), 'auv'),
})

const connection = await daemon.connect()
const auv = createAuv(connection)

try {
  const devices = await auv.devices.list()
  console.info(devices)
}
finally {
  await connection.close()
  await daemon.stop()
}
```

> [!NOTE]
>
> Almost all the `@auv-js/sdk` functions and APIs supports [AbortSignal](https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal), passing `signal` to `startAuv()` makes the daemon process abortable. If the signal is aborted, the daemon will be terminated immediately. Omit the signal and use `daemon.stop()` when the returned handle alone should own shutdown.

> [!CAUTION]
>
> Only import `startAuv` in Node.js or the Electron main process. An Electron
renderer remains a browser caller: give it a paired HTTP endpoint and Device
credential rather than exposing the child process handle or treating loopback
as browser owner authority.

Configure overlay presentation with typed launch options:

```ts
import { startAuv } from '@auv-js/sdk/node'

const daemon = await startAuv({
  overlay: {
    theme: {
      outlineColor: '#336699',
      cursorLabelBackground: '#336699',
      cursorLabelForeground: '#ffffff',
      cursorShadow: {
        color: { red: 206 / 255, green: 1, blue: 253 / 255, alpha: 0.55 },
        blurRadius: 8,
        offsetX: 0,
        offsetY: 2,
      },
    },
  },
})
```

`overlay.theme` uses camelCase fields and overrides any `AUV_OVERLAY_THEME`
from the inherited or supplied environment. Unset fields retain native styles;
`theme: {}` clears inherited theme overrides. Restart the owned daemon to change
its theme. Native SVG and shadow support currently requires macOS; use an AUV
binary with overlay host theme support. See the [theme reference](../../../docs/ai/references/driver/2026-09-07-overlay-host-theme.md)
for SVG artwork, status colors, and the raw environment format for other hosts.

### Connect as a plugin/runner through `AUV_CONTEXT`

`auv` cli has similar plugin capability like `kubectl` or `git`. You can build a `auv` plugin in Node.js, and when you have `auv-some-plugin` in your `PATH`, you can invoke it as:

```sh
auv some-plugin
```

and `auv` will pass the `AUV_CONTEXT` environment variable to the plugin process. You can use this context to communicate to `auv` and registers your own runner or capability:

```ts
import { connectFromContext, contextFromEnv, createAuv } from 'auv-js/node'

const context = contextFromEnv(process.env)
const connection = await connectFromContext(context)
const auv = createAuv(connection)

const displays = await auv
  .runner({ runnerClass: 'auv.core.local' })
  .displays
  .list()
```

> [!NOTE]
>
> `AUV_CONTEXT` never contains credentials. If it names a `config_profile`, the application must pass that profile's credential explicitly to `connectFromContext`; JavaScript profile-store lookup remains intentionally outside the SDK until credential persistence has an approved shared owner.

### Selecting a Device

`local: true` constrains operation placement to the daemon's implicit local Device.

Supplying an explicit `deviceId` or non-empty `deviceIds` at the same time rejects with `AuvConfigurationError` before dispatch.

## Pairing

An authenticated local owner or paired Device creates a one-time bootstrap
token. A new caller consumes it without presenting an existing Device
credential, then reconnects with the returned opaque credential.

```ts
const token = await auv.pairing.createToken({ signal })

const bootstrap = await connect({ endpoint, signal, transport: 'http' })
const enrollment = await pairDevice(bootstrap, {
  label: 'Browser controller',
  signal,
  token,
})

const paired = await connect({
  credential: enrollment.credential,
  endpoint,
  signal,
  transport: 'http',
})
```

## Call Runner capabilities

Bind a Runner route once, then use the same capability hierarchy as the Rust
`auv::client::runner::RunnerClient` interface:

```ts
const runner = auv.runner({
  runId: run.id,
  runnerClass: 'auv.core.local',
})

const displays = await runner.displays.list({ signal })
const window = await runner.windows.resolve({
  application: {
    case: 'applicationBundleId',
    value: 'com.example.App',
  },
}, { signal })

const capture = await window.capture({ signal })
const matches = await window.findText('Continue', { signal })
```

## Typed capability invocation

`invokeUnary` accepts message schemas generated by `protoc-gen-es`. It routes
the encoded request through the selected Device, optional Run, and required
RunnerClass without teaching the daemon an extension-owned message type.

```ts
const result = await invokeUnary(connection, {
  deviceId,
  input: SearchRequestSchema,
  method: 'Search',
  output: SearchResponseSchema,
  request: { query: 'music' },
  runId,
  runnerClass: 'example.music',
  service: 'example.music.v1.Library',
  signal,
})
```

## Discover extension operations

A generic host can discover annotated operations without generating an
extension-specific client. Discovery uses gRPC Reflection, stays scoped to one
RunnerClass, and retains the same optional Device and Run route for dynamic
calls.

```ts
const netease = await auv.runners.discover({
  runId: run.id,
  runnerClass: 'auv.app.netease_music',
})

for (const method of netease.apis) {
  console.info(method.id, method.effect, method.inputSchema)
}

const result = await netease.invokeUnaryJson({
  input: { applicationBundleId: 'com.netease.163music' },
  method: '/auv.netease_music.v1.PlayerService/GetNowPlaying',
})

const events = await netease.invokeServerStreamJson({
  input: { dailyRecommended: {} },
  method: '/auv.netease_music.v1.SongService/ListSongs',
})
for await (const event of events)
  console.info(event)
```

`apis` contains only RPCs marked with AUV's `discoverable` method option. The
complete gRPC Reflection method surface remains private to the discovery
implementation. Dynamic ProtoJSON invocation supports unary and
server-streaming APIs. Generated clients are still preferable when the
extension API is known at build time.

## Cancellation

Every asynchronous public operation accepts an `AbortSignal`. A signal passed
to `connect` only controls connection establishment. A default client signal
and a per-call signal are combined, so aborting either cancels the call.

Cancellation stops local waiting and asks the transport to cancel. It is not a
rollback guarantee after a mutating request has reached the daemon.
Cancellation is reported as `AuvAbortError`; malformed AUV responses use
`AuvProtocolError`, and connection failures use `AuvTransportError`.
Remote failures share `AuvRemoteError`; gRPC and WebSocket status failures add
`AuvRpcError.rpcCode`, while HTTP problem responses add status and problem type.

## Tests

```sh
pnpm exec playwright install chromium
pnpm --filter @auv-js/sdk test:run
```

Use `test:node`, `test:browser`, or `test:jsdom` to run one project on
its own.
