# AUV JavaScript SDK Spec

Date: 2026-08-11

Status: implemented on 2026-08-12; the package remains pre-1.0 while its public
surface receives use outside this repository.

Scope classification: `approved feature`

## Problem Statement

AUV has a canonical Rust operation interface and daemon-facing typed services.
Before this implementation, JavaScript callers did not have an equally direct
way to invoke those operations. Node.js and Electron applications need access to local transports,
while browser applications need a browser-compatible connection that can
control a local or remote Device without learning Rust, gRPC, Protobuf routing,
or daemon implementation details.

Without a JavaScript SDK, each application would need to rebuild connection,
authentication, pairing, Device/Run/Runner selection, request correlation,
streaming, cancellation, and error mapping. Separate hand-written clients would
also risk turning the REST, WebSocket, gRPC, and Unix-socket adapters into
different product APIs instead of transports for the same AUV operations.

The SDK must remain small enough for browser use and tree shaking. It must also
provide a convenient namespaced client without making that object the canonical
implementation. Every public asynchronous operation must participate in the
same cancellation contract.

## Solution

Provide an `@auv-js/sdk` package whose canonical implementation is a set of
individually exported functions. Each operation function receives an explicit
`AuvConnection`, domain-facing input, and an optional `AbortSignal`. A thin
`createAuv` binds a connection and optional default signal into an `AuvClient`, then exposes
the same functions under domain namespaces such as `devices`, `pairing`,
`runs`, and `runners`.

`connect` resolves a transport and returns an `AuvConnection`. Browser callers
use the HTTP transport, where unary requests use HTTP and ongoing bidirectional
or event traffic uses WebSocket. Node.js and Electron may additionally use gRPC
or a same-user Unix socket. Transport names are ergonomic shortcuts for the
corresponding transport factories; callers may also pass a constructed custom
transport.

The SDK projects the existing AUV operation interface rather than introducing
a JavaScript-only execution model. Device, Run, RunnerClass, Runner, pairing,
invoke results, tracing evidence, and artifacts retain their canonical AUV
meanings. Pairing uses the existing one-time bootstrap token and opaque Device
credential model. An authenticated local owner or paired Device bearer may
create a token; `PairDevice` is the token-authenticated enrollment operation.

All public functions that wait, perform I/O, or start ongoing work accept an
`AbortSignal`. Cancellation stops local waiting and asks the selected transport
and remote operation to cancel. It does not promise rollback after a mutating
operation has been dispatched.

The Node.js/Electron-main entry point also exposes an optional app-owned daemon
process host. `startAuv` runs the accepted `auv serve` foreground role and calls
the daemon's typed health API until every configured listener reports
`serving`. It requires explicit listener ports because `:0` is not a usable
connection endpoint. The same public health RPC is available through Unix
gRPC, TCP gRPC, and annotated HTTP. The returned lifecycle handle contains
connection information and explicit shutdown. This process helper owns only
the child lifecycle; daemon state, routing, recording, and serving semantics
remain in their existing Rust owners.

The Node.js entry point also parses the existing non-secret `AUV_CONTEXT`
contract through `contextFromEnv(env)`. The environment object is explicit and
defaults to `process.env`, which keeps embedded runtimes and tests independent
of process-global state. `connectFromContext` binds the resolved
`daemon_endpoint`, canonical `device_id`, and optional `run_id` to an
`AuvConnection`; routed operations inherit those values and reject conflicting
explicit placement before dispatch. `device_name` is retained only as context
metadata and is not a JavaScript-side selection input. Unknown context fields
remain additive. JavaScript profile-store and daemon discovery parity are
deferred; paired profile credentials remain application-owned and explicit.

## User Stories

1. As a browser application developer, I want to import `@auv-js/sdk`, so that I can control an AUV Device without a native addon.
2. As a Node.js developer, I want to use the same `@auv-js/sdk` package as browser callers, so that shared application code has one API.
3. As an Electron developer, I want to connect through a Unix socket, so that a trusted local application can use the owner-checked daemon transport.
4. As a JavaScript developer, I want to connect with a short transport name, so that common local configuration is concise.
5. As an advanced integrator, I want to pass a constructed transport, so that endpoint and adapter configuration remain explicit when needed.
6. As a browser developer, I want HTTP and WebSocket behavior presented as one HTTP transport, so that I do not manually coordinate unary and streaming channels.
7. As a package consumer, I want independently exported operation functions, so that bundlers can remove operations I do not use.
8. As an application developer, I want an optional namespaced AUV object, so that related operations are discoverable in an editor.
9. As a package consumer, I want the namespaced object to delegate to the exported functions, so that the two styles cannot drift in behavior.
10. As a caller using free functions, I want to pass an explicit connection, so that connection ownership and test substitution are visible.
11. As a caller using the client, I want the connection bound once, so that repeated operation calls remain concise.
12. As an AUV user, I want to list Devices from JavaScript, so that I can discover available local and paired resources.
13. As an AUV user, I want to select Device, Run, and RunnerClass through typed options, so that remote invoke follows the canonical placement model.
14. As an AUV user, I want to invoke typed AUV operations from JavaScript, so that CLI, Rust, MCP, and JavaScript use the same execution semantics.
15. As an extension author, I want generated extension clients to use an already connected and routed transport, so that extension services do not bypass AUV authentication and placement.
16. As a local owner, I want to create a one-time pairing token from Node.js or Electron, so that I can enroll another Device without a separate CLI flow.
17. As an authenticated paired Device, I want to create a pairing token through the same live operation, so that pairing administration matches AUV's shared authority model.
18. As a browser user, I want to consume a one-time pairing token and receive a Device credential, so that the browser can establish its own authenticated connection.
19. As a security-conscious caller, I want pairing tokens consumed only by enrollment, so that a bootstrap secret is not mistaken for a reusable API bearer.
20. As a caller, I want pairing credentials represented opaquely, so that application code does not depend on credential encoding.
21. As a frontend developer, I want every asynchronous SDK operation to accept an `AbortSignal`, so that component teardown can cancel outstanding work.
22. As a frontend developer, I want to bind a default signal to an AUV client, so that one lifecycle cancellation reaches all calls made through that client.
23. As a caller, I want a per-operation signal in addition to the client signal, so that a single request can have a narrower lifetime.
24. As a caller, I want either the default or per-operation signal to cancel the call, so that nested lifetimes compose predictably.
25. As a caller with an already-aborted signal, I want the operation rejected before dispatch, so that cancelled work never reaches the daemon.
26. As a caller cancelling an in-flight unary request, I want local waiting stopped and remote cancellation requested, so that unnecessary work is curtailed.
27. As a streaming caller, I want one signal to govern the stream lifetime, so that aborting also terminates asynchronous iteration and releases transport resources.
28. As a caller establishing a connection, I want the signal passed to `connect` to govern only connection establishment, so that later abort does not unexpectedly close a successful connection.
29. As a caller closing a connection, I want a documented asynchronous close operation, so that transport resources can be released deliberately.
30. As a caller, I want cancellation reported consistently as an abort rather than a generic transport failure, so that application control flow is portable across transports.
31. As a caller of a mutating operation, I want documentation that cancellation is not rollback, so that I do not automatically repeat an operation with an unknown outcome.
32. As a caller, I want domain errors separated from transport and protocol errors, so that application decisions do not depend on gRPC or WebSocket details.
33. As a caller, I want direct typed operation results, so that delivery evidence is not collapsed into a generic success boolean.
34. As a caller requiring semantic success, I want verification to remain a separate operation, so that input delivery is not misrepresented as application state change.
35. As a browser application, I want bounded artifacts and typed events available without a full remote-desktop video channel, so that basic AUV control remains lightweight.
36. As an SDK maintainer, I want every transport to implement one request, stream, cancellation, and close contract, so that new adapters do not change product semantics.
37. As an SDK maintainer, I want browser and Node entry points to avoid importing unavailable platform code, so that normal imports work without runtime stubs.
38. As an SDK consumer, I want public TypeScript declarations for inputs, results, errors, transports, and connections, so that editor and compiler feedback match runtime behavior.
39. As a test author, I want operation functions testable through a recording transport, so that public behavior can be verified without a live daemon.
40. As an AUV maintainer, I want JavaScript calls to preserve canonical Run recording and artifact production, so that a new frontend does not create an uninspectable execution lane.
41. As a Node.js or Electron-main developer, I want to start and stop an app-owned `auv serve` child with typed options and resolved connection information, so that every application does not rebuild process readiness and cleanup logic.
42. As a JavaScript CLI plugin or executable Runner, I want to parse a caller-supplied environment object and inherit its resolved Device and Run route, so that I do not reproduce parent selection policy or depend on global process state.

## Implementation Decisions

- The package name is `@auv-js/sdk`. The package exposes browser-safe entry points
  and platform-specific Node.js/Electron entry points without loading Unix or
  native transport code into browser bundles.
- The Node.js/Electron-main entry point may supervise an app-owned `auv serve`
  child. It maps explicit foreground CLI options, verifies readiness through
  the typed health API, and owns graceful child cleanup; it does not parse CLI
  logs to reconstruct connection configuration, is not a persistent service
manager, and does not replace a future `auv daemon start|status|stop`
frontend.
- `startAuv.signal` is the child lifecycle signal passed to tinyexec. Aborting
  it after readiness still terminates the child; callers that want handle-only
  shutdown omit it and call `AuvDaemon.stop()`.
- Individually exported functions are the canonical implementation and the
  primary tree-shaking boundary. The namespaced client contains no parallel
  operation logic.
- `connect` is asynchronous and returns an `AuvConnection`. The connection owns
  transport state, authentication material, request correlation, and negotiated
  protocol state; it does not become a second domain operation interface.
- `createAuv` synchronously binds an existing connection and optional client
  defaults. Its namespaces follow AUV domains, initially including `devices`,
  `pairing`, `runs`, and `runners`; a generic `daemon` bucket is not the public
  organization model.
- Free operations take an explicit connection. Client methods call those same
  free operations with the bound connection.

### Accepted public API shape

The following declaration sketch comes from the design conversation. It is
included because the relationship between connection, operation functions,
client methods, transport shortcuts, credentials, and cancellation is the
central decision of this spec; prose alone does not preserve that relationship
precisely enough.

```ts
export interface ConnectOptions extends OperationOptions {
  credential?: DeviceCredential
  endpoint?: string | URL
  local?: boolean
  transport?: 'grpc' | 'http' | 'unix' | Transport
}

export interface CreateClientOptions {
  /** Default lifetime inherited by every operation called through this client. */
  signal?: AbortSignal
}

export interface OperationOptions {
  signal?: AbortSignal
}

export function connect(options?: ConnectOptions): Promise<AuvConnection>

export function createAuv(
  connection: AuvConnection,
  options?: CreateClientOptions,
): AuvClient

export function createPairingToken(
  connection: AuvConnection,
  options?: OperationOptions & { ttlMs?: number },
): Promise<PairingToken>

export function listDevices(
  connection: AuvConnection,
  options?: OperationOptions,
): Promise<readonly Device[]>

export function pairDevice(
  connection: AuvConnection,
  input: OperationOptions & {
    label: string
    token: PairingToken
  },
): Promise<PairingEnrollment>
```

`connect` is awaited; the returned value is the `AuvConnection` passed to every
canonical operation function. `createAuv` is synchronous because it only binds
that connection and defaults. Its shape is deliberately `createAuv(connection,
options)`, not a second configuration object that reconnects independently.

The two calling styles are equivalent:

```ts
import {
  connect,
  createAuv,
  createPairingToken,
  listDevices,
} from '@auv-js/sdk'

const connection = await connect({
  local: true,
  signal,
  transport: 'http',
})

// Canonical, tree-shakeable function form.
const token = await createPairingToken(connection, { signal })
const devices = await listDevices(connection, { signal })

// Ergonomic client over the same functions.
const auv = createAuv(connection)
const sameToken = await auv.pairing.createToken({ signal })
const sameDevices = await auv.devices.list({ signal })
```

The client namespaces bind arguments rather than own behavior. Conceptually,
`auv.devices.list(options)` is `listDevices(connection, options)`, and
`auv.pairing.createToken(options)` is
`createPairingToken(connection, options)`. Runs, Runners, and typed capability
operations follow the same rule. There is no separately implemented object API
to maintain.

Transport shortcuts and factories are also equivalent:

```ts
const shortConnection = await connect({
  local: true,
  signal,
  transport: 'http',
})

const explicitConnection = await connect({
  signal,
  transport: createHttpTransport(),
})
```

The HTTP transport owns HTTP unary calls and the WebSocket lane used for
streaming or bidirectional calls. gRPC and Unix socket transports implement the
same `Transport` contract. Platform-specific package entry points may be used
to keep Node-only Unix-socket code out of browser module graphs; that packaging
choice does not change the `connect` contract.

Generated daemon resource bindings use ProtoJSON request and response bodies.
The dynamic routed invoke endpoint keeps Protobuf payloads opaque because its
extension-owned input and output types are selected at runtime. WebSocket invoke
uses one serialized `ClientMessage` or `ServerMessage` per WebSocket binary
message. These application messages own the `Open`/`Ready`, input/output,
half-close/cancel, and terminal `End` lifecycle; they do not model WebSocket
protocol frames. `Input.payload` and `Output.payload` carry the same opaque
method-specific Protobuf bytes as dynamic HTTP invoke, without a gRPC message
prefix.

### Accepted pairing flow

Creating a token and consuming it are separate operations. A local owner or an
already paired Device uses an authenticated connection to create a one-time
token:

```ts
const ownerConnection = await connect({
  local: true,
  signal,
  transport: 'unix',
})

const token = await createPairingToken(ownerConnection, { signal })
```

The new browser or JavaScript client connects to the target transport without a
Device credential, consumes the token through `pairDevice`, and then reconnects
using the returned opaque credential for ordinary operations:

```ts
const bootstrapConnection = await connect({
  endpoint,
  signal,
  transport: 'http',
})

const enrollment = await pairDevice(bootstrapConnection, {
  label: 'Browser controller',
  signal,
  token,
})

const connection = await connect({
  credential: enrollment.credential,
  endpoint,
  signal,
  transport: 'http',
})

const auv = createAuv(connection)
await auv.devices.list({ signal })
```

The bootstrap connection is allowed to call only the token-authenticated
enrollment operation. Passing a pairing token to `connect` as though it were a
long-lived bearer is not part of the API. A browser may receive the one-time
token from any already authenticated `@auv-js/sdk`, Rust, CLI, or MCP caller that is
authorized to create it; Unix socket access is one bootstrap path, not the only
token creator.

### Accepted cancellation shape

Callers create an `AbortController`, but every SDK function receives only its
signal:

```ts
const controller = new AbortController()

const pending = listDevices(connection, {
  signal: controller.signal,
})

controller.abort()
await pending // rejects with the normalized AbortError
```

A client may bind a broader lifecycle and a call may add a narrower one. Either
signal cancels the operation:

```ts
const page = new AbortController()
const request = new AbortController()
const auv = createAuv(connection, { signal: page.signal })

await auv.devices.list({ signal: request.signal })
```

The signal stays in SDK operation options; it is not serialized into a domain
or Protobuf request. The canonical function passes it separately to the common
transport contract. Unary, streaming, pairing, invoke, and asynchronous
connection-lifecycle functions all use this convention. Synchronous value and
transport factories do not pretend to be cancellable merely to add a parameter.

- Transport factories include HTTP, gRPC, and Unix socket adapters. A transport
  string is a shortcut for a factory with local discovery or documented default
  endpoint behavior. HTTP and gRPC default to loopback port 9847; Unix sockets
  require the owner path until JavaScript discovery-file resolution is accepted.
- The browser-compatible HTTP transport combines HTTP unary calls with
  WebSocket streaming and bidirectional calls. WebSocket is a transport lane,
  not a separate product API and not authorization by itself.
- A custom transport can be injected if it implements the common transport
  contract. The contract owns unary dispatch, streaming, cancellation, and
  close behavior, plus normalized transport/protocol failures.
- The SDK projects AUV's typed operation interface. It does not expose raw
  protocol messages as the default domain API, duplicate operation result
  models, or silently select a different Device after failure.
- Device, Run, RunnerClass, and Runner routing remains canonical. A `local`
  shortcut constrains selection to the local Device; it does not bypass
  authentication, resource lifecycle, operation execution, or Run recording.
- Browser HTTP and WebSocket traffic is served only by paired-bearer listeners.
  Caller-local listeners reject requests carrying a browser `Origin`; loopback
  transport location alone is not browser owner authority.
- Local Unix-socket access is owner-checked same-user authority, not an open or
  anonymous daemon mode.
- Pairing is exposed under the pairing namespace as operations such as creating
  a pairing token and enrolling a Device. It is not modeled as a top-level
  resource.
- Creating a pairing token requires an authenticated connection. Both the local
  owner and every active paired Device bearer have the same pairing
  administration capability under the current AUV contract.
- Pairing tokens are cryptographically random, displayed or transferred once,
  consumed once by enrollment, and have no deadline unless their creator
  explicitly supplies a TTL. They are not accepted as ordinary operation
  credentials.
- Successful enrollment returns an opaque long-lived Device credential. The
  SDK accepts and returns credential values without exposing their encoding as
  application semantics. Credential persistence remains application-owned;
  browser, Node.js, and Electron storage are not assumed to share one backend.
- All public asynchronous or I/O-producing operations accept an optional
  `AbortSignal` in their operation options. The SDK accepts signals, not
  controllers; the caller owns the `AbortController`.
- Pure synchronous factories do not accept a meaningless cancellation
  parameter. `createAuv` may accept a signal specifically as the default
  lifetime for operations invoked through that client.
- A client-level signal and a call-level signal are combined so that aborting
  either cancels the call. Combining signals must not leave event listeners
  attached after settlement.
- An already-aborted signal rejects before transport dispatch. In-flight abort
  stops local waiting, asks the transport to cancel the correlated request, and
  rejects with the SDK's normalized abort error.
- A stream signal governs opening and the full asynchronous-iteration lifetime.
  Aborting requests remote cancellation, terminates iteration, and releases
  local listeners and buffers.
- The signal passed to `connect` applies only until connection establishment
  succeeds. Established connection lifetime is controlled by explicit close or
  by a separately documented connection-lifetime facility, not by surprising
  reuse of the establishment signal.
- Cancellation is cooperative and is not transaction rollback. If a mutating
  request was dispatched before abort, its remote result may be unknown and the
  SDK must not automatically replay it.
- HTTP maps cancellation to the platform request signal. WebSocket maps it to a
  correlated protocol cancellation message. gRPC maps it to call cancellation.
  Unix socket behavior follows the same transport contract even where remote
  cancellation is best effort.
- Domain errors remain distinct from authentication, transport, protocol,
  cancellation, and context/configuration errors. Wire-specific errors are
  mapped before reaching normal operation callers.
- HTTP problem responses and gRPC/WebSocket status failures share a remote
  error base without being classified as transport failures; their concrete
  subclasses retain HTTP problem or RPC status detail.
- Typed driver results such as `InputActionResult` remain delivery evidence.
  Semantic verification remains a separate result and is never inferred from
  WebSocket acknowledgement or successful request completion.
- Operation execution continues through the existing frontend-owned Run
  context and canonical tracing/artifact path. The optional Node process host
  owns only the app child it starts; the SDK does not own daemon semantics, run
  persistence, system-service supervision, or a new aggregate runtime.
- Continuous video/audio remote desktop transport is not required for the
  initial SDK. Future media may use a separate plane correlated with Device and
  Run if a concrete consumer justifies it.

## Testing Decisions

- Fast protocol-mapping tests use the public `@auv-js/sdk` function/client API over
  a recording transport. The invoke integration seam starts a real `auv`
  daemon, enrolls a Device, authenticates with its credential, and reaches a
  real routed Runner over HTTP and WebSocket.
- The same operation scenarios run against the free-function form and verify
  that the client delegates with the bound connection and defaults. The client
  does not receive a duplicate behavior suite that would stabilize its
  internals.
- Transport tests cover HTTP unary behavior, WebSocket application-message encoding and streaming,
  gRPC unary behavior and abort, and the shared gRPC implementation used by TCP
  and Unix sockets. The daemon integration suite exercises HTTP unary invoke
  and WebSocket streaming against the same live routed Runner.
- Package conformance builds before inspecting conditional exports, verifies
  that the browser graph excludes Node/gRPC dependencies, and runs during the
  package test command. `prepack` always rebuilds publishable artifacts.
- Cancellation tests use observable boundaries: whether dispatch occurred,
  whether one cancel was emitted for the correlated request, which error the
  caller received, whether iteration terminated, and whether subsequent calls
  remain usable.
- Connection tests verify that abort during establishment rejects and releases
  resources, while aborting the establishment signal after a successful
  connection does not close it.
- Pairing tests verify authenticated token creation by both accepted authority
  classes, rejection of unauthenticated creation, one-time token consumption,
  optional TTL behavior, credential return, and immediate effect of disable,
  unpair, and credential revocation on the next authorization lookup.
- Placement tests verify local shortcut behavior, explicit remote Device
  selection, conflicting selectors, no implicit local fallback, and preservation
  of Run/RunnerClass routing.
- Invoke tests assert app-owned results and canonical delivery evidence. Tests
  requiring semantic success assert a separate verification result.
- Browser bundle tests verify that the browser entry point does not import
  Node.js, Unix-socket, or native modules and that unused operation exports can
  be removed by a representative bundler.
- Type-level tests cover public inputs, output inference, custom transports,
  domain namespaces, and the presence of `AbortSignal` on every asynchronous
  operation surface.
- Live integration tests are narrow evidence above the conformance suite: one
  local owner connection, one browser-compatible HTTP/WebSocket connection,
  one pairing enrollment, one Device list, and one typed invoke through the
  daemon. They verify interoperability rather than replace deterministic SDK
  tests.
- Existing Rust operation-interface tests and daemon server pairing tests are
  prior art: they already test typed public behavior, error mapping, shared
  pairing administration, and next-request revocation rather than private
  persistence details.

## Out of Scope

- Reintroducing the retired public Session resource or creating an aggregate
  `auv-runtime` package.
- Replacing canonical Rust domain contracts with Protobuf messages in local
  library calls.
- A generic raw gRPC escape hatch for core Device, Run, Runner, pairing, or
  first-party capability operations.
- An unauthenticated/open local HTTP daemon or token creation without an
  established local-owner or paired-Device caller.
- Treating a pairing token as a normal bearer credential.
- Choosing a final browser credential persistence backend or promising that
  browser storage has the same trust properties as an owner-checked Unix socket.
- Automatic retry of mutating requests whose terminal outcome is unknown.
- Transaction rollback as a consequence of `AbortSignal` cancellation.
- Full remote desktop framebuffer, live video/audio, WebRTC signaling, or raw
  mouse/keyboard tunneling as the initial JavaScript SDK product.
- Expanding the archived `candidate-action` vertical or creating a second input
  result schema.
- Stabilizing the pre-1.0 npm API or promising compatibility before real
  consumers have exercised the package.

## Further Notes

This spec records the API direction agreed in the design conversation. The
function-first surface is the source of behavior; the object client is an
ergonomic binding layer. This keeps bundle size and discoverability from
becoming competing implementations.

The related browser protocol research explains why WebSocket alone is not the
RPC or authorization contract. The implemented socket is deliberately scoped
to one routed operation: its Protobuf `Open`, input, half-close, cancel, output,
and terminal status frames preserve typed payloads and explicit lifecycle
without creating a second session resource.

The implementation lives in `js/packages/sdk`; browser-safe exports are kept
separate from the Node gRPC/Unix adapter, and all generated message code comes
from the checked-in Protobuf schemas through Buf.
