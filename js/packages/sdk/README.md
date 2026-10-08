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
- [Mock Runner](#mock-runner)
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

Call `startAuv()` once for the lifetime of your Node.js or Electron host, then
reuse its connection and `createAuv(connection)` client. Each `startAuv()` call
starts a new app-owned process; it does not discover or attach to another daemon.
Starting on an occupied endpoint fails. To use a daemon owned by another host,
call `connect({ endpoint, transport, local })` and close only that connection.

`await checkHealth(connection)` and `await auv.health.check()` return
`{ id, status: 'serving' }`. The same daemon reports the same `id` on every
listener. `daemon.id` is the fresh UUID assigned by `startAuv()` for that launch;
startup checks it through health and does not parse stdout. The ID is public
correlation data, independent of Device, Runner, and Run IDs, and grants no access.

The lazily started `auv.core.local` Runner stays available for five minutes after
its last request and Run attachment finish. Concurrent first calls share its
creation. Reusing this child preserves its desktop/Portal sessions between calls;
its PID and Runner ID are separate from Run IDs.

For Electron, if you wished to embed the AUV daemon and offer computer use capabilities without requiring the user to install it separately, you can start the daemon from your main process and connect to it:

```ts
import { join } from 'node:path'

import { createAuv, startAuv } from 'auv-js/node'
import { app } from 'electron'

const daemon = await startAuv({
  binaryPath: join(process.resourcesPath, 'bin', 'auv'),
  listeners: ['http://127.0.0.1:9847'],
  noRegister: true,
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
      cursorLabelBackground: '#336699',
      cursorLabelForeground: '#ffffff',
      cursorShadow: {
        blurRadius: 8,
        color: { alpha: 0.55, blue: 253 / 255, green: 1, red: 206 / 255 },
        offsetX: 0,
        offsetY: 2,
      },
      outlineColor: '#336699',
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

An application that ships its own macOS locked-session helper (its own name,
icon, bundle identifier, and Developer ID team) passes the embedded app to the
daemon so it trusts that helper instead of the official AUV Helper:

```ts
const daemon = await startAuv({
  binaryPath,
  platforms: {
    macos: { helperApp: '/Applications/YourApp.app/Contents/Library/Helpers/Your Computer Use.app' },
  },
})
```

Pass the same path to `installMacosHelper({ helperApp })` from `@auv-js/cli`.
`platforms.macos.helperApp` overrides any `AUV_MACOS_HELPER_APP` from the
inherited or supplied environment and is ignored on other platforms. See
[shipping your own macOS helper](../cli/README.md#shipping-your-own-macos-helper).

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

`windows.resolve` and `windows.list` return `WindowClient`s that carry their
`Window` metadata (`window.window.title`, `frame`, …). A listed window acts
directly, without resolving it again:

```ts
const windows = await runner.windows.list({ signal })
const music = windows.find(w => w.window.applicationBundleId === 'com.netease.163music')
await music?.click({ x: 400, y: 50 }, { click: { count: 1 } })
```

Clicks and scrolls take a point in any coordinate space. A plain `{ x, y }` is
window-local on a `WindowClient` and screen space on `input`; a `Position`
names its space; anything with screen `bounds`, such as a text match, is
clicked at its center. A window converts screen and display positions with its
current frame on the Runner, so OCR results need no manual offset:

```ts
const { matches } = await window.findText('Continue', { signal })
await window.click(matches[0]!) // delivered to this window
await runner.input.click({ x: 640, y: 400 }) // a global click in screen space
await runner.input.click(Position.display(displays[0]!, 10, 10)) // relative to the display's origin
```

A global click (screen or display position on `input`) has no target window,
so the Runner rejects `policy` and `windowStrategy` there.

Small pure helpers cover the geometry these calls return. They take any
object with the right fields (`ScreenRect`, a window `frame`, a match's
`bounds`) and never convert coordinate spaces:

```ts
import { center, contains, intersect, Position } from '@auv-js/sdk'

Position.screen(640, 400) // also Position.window(window, x, y), Position.display(display, x, y)
center(matches[0]!.bounds!) // { x, y }
contains(window.window.frame!, center(matches[0]!.bounds!)) // edges count as inside
intersect(area, window.window.frame!) // the overlap, or undefined
```

Keyboard input can name its window too. `typeText`, `pressKeys` and
`pasteText` on a `WindowClient` send `InputService/InputKeyboard` with that
window as the recipient. By default (`InputPolicy.FOREGROUND_PREFERRED`) the
window is brought to the front and focused first, so the input cannot land in
another app. `input.typeText` and `input.pressKey` go to whichever app has
keyboard focus when they run:

```ts
import { InputPolicy } from '@auv-js/sdk'

await music?.typeText('Reply')
await music?.pressKeys(['return'])
await music?.pressKeys(['cmd', 'a'], { policy: InputPolicy.BACKGROUND_ONLY }) // control must already have focus
```

Background policies post to the window's process without activating it. The
control must already have keyboard focus: in NetEase Cloud Music, a background
click on the search box did not give it focus while another app was in front.
`input.keyboard` and `input.pressKeys` take the full request, including an
application or foreground recipient and several ordered actions.

A window reference belongs to the Device, not to a Run. Bind a known window to
another route without a call, or refresh it by ID:

```ts
const next = auv.runner({ runId: nextRun.id, runnerClass: 'auv.core.local' })
const same = next.windows.from(window) // a client, Window, WindowRef or window ID
const fresh = await next.windows.get(window) // current metadata; NOT_FOUND once closed
```

Captures stay in the Runner. `capture()`, `findText()` and `scrollUntil()`
observations return a `CapturedFrame` with a `ref` and metadata (bounds,
`pixelSize`, origin), not pixels. Pass the frame back for OCR, and fetch pixels
only when you need them, bounded and encoded:

```ts
import { ImageEncoding } from '@auv-js/sdk'

const { capture: frame } = await window.capture({ signal })
const text = await runner.recognizeText(frame!, { region: { height: 1, width: 0.3, x: 0, y: 0 } })
const thumbnail = await runner.captures.image(frame!, {
  encoding: ImageEncoding.JPEG,
  maxSize: { height: 800, width: 1280 },
})
const bitmap = await createImageBitmap(new Blob([thumbnail.data], { type: 'image/jpeg' }))
```

Every `CapturedFrame` also carries `thumbhash`, a ThumbHash of the whole capture
(about 20 bytes). Decode it with the [`thumbhash`](https://www.npmjs.com/package/thumbhash)
package to show a blurred preview while the pixels load:

```ts
import { thumbHashToDataURL } from 'thumbhash'

const preview = frame!.thumbhash.length > 0 ? thumbHashToDataURL(frame!.thumbhash) : undefined
```

Captures are taken at native resolution (2x on Retina). For frames you only
display, or compare for motion, pass `{ resolution: CaptureResolution.LOGICAL }`
to `capture()` / `captureRegion()`: one pixel per point, a quarter of the
pixels.

Encodings: `RGBA` (default, raw rows), `PNG`, `JPEG` (quality 85, smallest for
photo-heavy screens) and `WEBP` (lossless, about JPEG's size for UI, exact
pixels).

OCR and image fetches take a screen area directly with `screenRegion` (or
`region` as fractions of the image); result bounds are screen rectangles.

A capture reference fails with NOT_FOUND once the Runner evicts it (least
recently used beyond its memory budget, or idle for ten minutes); capture
again. To OCR an image you own, pass `{ frame: { image, bounds, scaleFactor } }`.

Driver enums such as `MouseButton`, `InputDeliveryPath`, `ImageEncoding` and
`ScrollUntilStopReason` are exported from `@auv-js/sdk`.

Create a Run for each workflow that needs its own correlation identity:

```ts
const run = await auv.runs.create()
const runner = auv.runner({ runId: run.id, runnerClass: 'auv.core.local' })
try {
  await runner.displays.list()
  // Further steps in this workflow use this same route and Run ID.
  await auv.runs.stop({ outcome: 'succeeded', runId: run.id })
}
catch (error) {
  await auv.runs.stop({ outcome: 'failed', runId: run.id })
  throw error
}
```

Different Runs can share one Runner; stopping one Run does not stop another.
A route without `runId` does not create an implicit Run. A Run ID groups a
workflow, not each RPC within it. The control Run and its routing metadata do
not by themselves promise complete persisted tracing for every low-level call.

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

Methods describe themselves. `presentation` comes with the descriptors: an API
name (`window.find_text`, which `camelCaseName` spells `window.findText`), a
title and a one-paragraph description. Long-form Markdown docs and examples are
fetched on request; `docs()` resolves `undefined` when a method has none:

```ts
const core = await auv.runners.discover({ runnerClass: 'auv.core.local' })
const findText = core.describeMethod('/auv.api.driver.v1.TextRecognitionService/FindWindowText')
findText?.presentation // { name: 'window.find_text', title: 'Find text in a window', description: '…' }
const docs = await findText?.docs() // { markdown, examples: [{ language: 'ts', title, code }, …] }
```

## Mock Runner

`createMockTransport` builds a Runner in memory: an ordinary `Transport` that
answers Runner RPCs from typed implementations of the generated services, so
clients, discovery and tests run without a device. Methods you leave out
answer `UNIMPLEMENTED`; throw `AuvRpcError` to fail a call. The registration
style follows Connect-ES's `createRouterTransport`.

```ts
import { AuvRpcError, connect, createAuv, createMockTransport, serveMockDaemon, WindowService } from '@auv-js/sdk'

const transport = createMockTransport((router) => {
  serveMockDaemon(router, { id: 'mock', name: 'Mock desktop' }) // one Device and Runs
  router.service(WindowService, {
    listWindows: () => ({ windows: [{ ref: { windowId: 'w-1' }, title: 'Inbox' }] }),
    resolveWindow: () => {
      throw new AuvRpcError(5, 'no window matches')
    },
  })
})
const runner = createAuv(await connect({ transport })).runner({ runnerClass: 'auv.core.local' })
await runner.windows.list() // [WindowClient for w-1]
```

Requests arrive decoded and typed, results are checked against the response
message type, and streaming methods are async generators. The mock also
answers gRPC Reflection for the services it registers, so `discoverRunner`
reports their effects and `presentation`. The REPL playground's mock desktop
is built this way (`apps/repl-playground/src/backend/mock-runner.ts`).

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
