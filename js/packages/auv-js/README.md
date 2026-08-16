# auv-js

TypeScript SDK for AUV Device, pairing, Run, Runner, and routed capability
operations. The package is function-first for tree shaking and also provides a
namespaced client over the same functions.

## Browser and universal JavaScript

```ts
import {
  connect,
  createAuv,
  createHttpTransport,
} from 'auv-js'

const controller = new AbortController()
const connection = await connect({
  credential,
  signal: controller.signal,
  transport: createHttpTransport({ endpoint: 'http://127.0.0.1:9847' }),
})
const auv = createAuv(connection)

const devices = await auv.devices.list({ signal: controller.signal })
const run = await auv.runs.create({
  deviceIds: [devices[0]!.id],
  signal: controller.signal,
})
```

`transport: 'http'` selects the same HTTP transport with the default local
endpoint. Daemon resource bindings use ProtoJSON, while dynamic routed invoke
uses opaque Protobuf payloads. Daemon problem responses remain available as
`AuvHttpError` values.

Browser traffic is accepted by paired-bearer listeners. Caller-local TCP and
Unix listeners retain owner authority for native clients and reject requests
carrying a browser `Origin`; a web page must enroll once and use its Device
credential against a paired-bearer listener, even when that listener is bound
to loopback.

## Node.js and Electron

```ts
import {
  connect,
  createAuv,
  createUnixSocketTransport,
} from 'auv-js/node'

const connection = await connect({
  transport: createUnixSocketTransport({ path: '/absolute/path/to/auv.sock' }),
})
const auv = createAuv(connection)
```

The Node entry point also exports `createGrpcTransport`. The browser entry point
does not import Node.js or gRPC modules.

Node.js and the Electron main process can also own an `auv serve` child for the
application lifetime:

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

`startAuv` maps the current `auv serve` options and waits for every configured
listener's typed health check to report `serving`. Listener ports must be
explicit; `:0` is not accepted because it is not a usable connection endpoint.
It returns the child PID, all endpoints, an `exited` promise, and idempotent
`stop()`. `connectionOptions` is the serializable description of the preferred
caller-local endpoint; `daemon.connect()` binds those defaults. Relative daemon
paths are resolved from `workingDirectory`, matching the CLI.

Passing `signal` gives tinyexec ownership of that child-process cancellation:
aborting it terminates the daemon even after `startAuv()` has returned. Omit the
signal and use `daemon.stop()` when the returned handle alone should own
shutdown.

Only import `startAuv` in Node.js or the Electron main process. An Electron
renderer remains a browser caller: give it a paired HTTP endpoint and Device
credential rather than exposing the child process handle or treating loopback
as browser owner authority.

CLI plugins and daemon-managed Node.js runners can inherit the resolved,
non-secret `AUV_CONTEXT` process contract:

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

`contextFromEnv` accepts any read-only environment-shaped object, so embedded
runtimes and tests do not need to mutate or read the global `process.env`.
Unknown JSON fields are ignored. `connectFromContext` inherits canonical
`device_id` and `run_id` for routed operations; `device_name` remains a display
snapshot and is never used to select another Device. A conflicting explicit
Device or Run is rejected before dispatch.

`AUV_CONTEXT` never contains credentials. If it names a `config_profile`, the
application must pass that profile's credential explicitly to
`connectFromContext`; JavaScript profile-store lookup remains intentionally
outside the SDK until credential persistence has an approved shared owner.

`local: true` constrains operation placement to the daemon's implicit local
Device. Supplying an explicit `deviceId` or non-empty `deviceIds` at the same
time rejects with `AuvConfigurationError` before dispatch.

## First-party Driver capabilities

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

The resolved window control stores only its `windowId`. Each operation sends
that ID to the Runner and returns the observation made by that operation; the
client does not retain the frame, title, or other resolve-time state as current
grounding.

The interface is transport-independent. Unary calls use native gRPC on the
Node gRPC and Unix transports and the dynamic HTTP invoke endpoint on the HTTP
transport. Streaming calls use native gRPC in Node and the WebSocket invoke
protocol through the HTTP transport.

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

`invokeServerStream` and `invokeDuplex` use the same schemas and route fields.
In browsers the HTTP transport opens one WebSocket per live operation; in
Node.js the gRPC and Unix transports use a native bidirectional gRPC stream.
Responses are exposed as typed async iterables, and aborting the operation
closes the remote stream.

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

The SDK has separate Vitest projects for the Node.js runtime, `jsdom`, and a
real headless Chromium instance. Install the Chromium binary once in a fresh
development or CI environment, then run the complete suite:

```sh
pnpm --filter auv-js exec playwright install chromium
pnpm --filter auv-js test
```

Use `test:node`, `test:browser`, or `test:jsdom` to run one runtime project on
its own. Package-condition, browser dependency-graph, and tree-shaking checks
remain in `test:package`; they complement the runtime projects rather than
replace them.
