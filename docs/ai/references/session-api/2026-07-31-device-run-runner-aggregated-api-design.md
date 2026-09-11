# Device, Run, Runner, and Aggregated API Design

Date: 2026-07-31

Status: historical design record, superseded by
`2026-08-03-auv-facade-daemon-runner-architecture.md`. The claim/lease model,
capability manifests, custom `RunnerRuntimeService`, and typed daemon
forwarding described below are no longer current. This document remains as the
decision history for the earlier Device/Run/Runner prototype.

## Decision summary

AUV's public control model has three primary resources:

- A `Device` is an addressable execution and trust boundary.
- A `Run` is one correlation and control scope from creation through a terminal
  phase. A Run may use several Devices and lease several Runners.
- A `Runner` is a daemon-owned process/runtime on exactly one Device. It
  implements a declared set of versioned typed services and uses an
  `ephemeral`, `unless-idle`, or `unless-shutdown` lifecycle policy.

The root CLI has two distinct execution surfaces:

- `auv invoke <core-operation>` calls only atomic operations registered by
  `auv-cli-invoke`.
- `auv <plugin> <plugin-command>` executes an external `auv-<plugin>` frontend.
  The root CLI may resolve Device and Run context before process replacement;
  the plugin owns its nested command tree and uses AUV client utilities to
  consume that context.

The daemon owns external authentication, authorization, Runner creation,
private Runner IPC, routing, health, draining, and discovery. A Runner does not
open a public AUV listener. First-party and custom Runners implement dedicated
protobuf services. Their method registration is flat; generated clients and
application facades may compose those services into hierarchical APIs.

The design deliberately defers per-call Inspect persistence. A Run must be
available as correlation and control context, but this document does not define
how every remote API call becomes an Inspect record. Video and real-time media
streaming also remain outside this design.

## Why the session prototype was retired

The retired implementation established several useful mechanisms:

- one service implementation can accept loopback TCP, Unix domain socket, and
  authenticated remote transports;
- transport evidence can establish a stable caller before a handler
  executes;
- daemon-owned resources can initialize lazily, drain active calls, and expire
  after an idle period;
- Rust and TypeScript clients can be generated from one protobuf contract.

Its public model nevertheless makes one daemon-local `Session` the parent of
all operations. Generic `Invoke` exposes `command_id + JSON bytes`, while only a
few driver/inference methods have dedicated RPCs. This shape cannot express a
Run that captures on one Device, recognizes text on another, and runs object
detection on a third. An opaque Invoke request also prevents per-method schema
validation, authorization, compatibility checks, SDK generation, and community
implementation of an AUV-compatible service subset.

The accepted target keeps the proven transport and lifecycle mechanisms while
replacing the public resource hierarchy.

## Domain vocabulary

### Device

A Device is an addressable AUV execution node and trust boundary. It has:

- a stable unique Device ID;
- an optional non-unique human-facing name;
- one or more connection/discovery profiles;
- labels for selection and administration;
- authenticated identity and trust material;
- observed availability;
- registered Runner classes and live Runners;
- an observed set of services the Device can currently provide.

The current machine is represented by a local Device. Omitting a Device flag
selects this local Device; it does not bypass the Device model.

Pairing is the process by which another Device is enrolled and trusted.
`pairing` is not a top-level resource name. The intended CLI resource is:

```text
auv devices list
auv devices ls
auv devices get <device>
auv devices pair ...
auv devices unpair <device>
auv devices enable|disable <device>
```

Pairing uses a kubeadm-like short bootstrap token. The daemon owner creates a
one-time token through the owner-checked Unix socket, then the client runs
`auv devices pair connect --endpoint <endpoint> --token <token> --label
<label>`. `PairDevice` consumes the token through the remote endpoint and
returns one opaque long-lived Device bearer. The CLI immediately saves that
bearer in the local Device profile store, reports the profile and file path,
and never prints the bearer. There is no `pair list`: paired Devices are the
resource, so callers inspect them with `auv devices list` and manage stored
connection details with `auv devices profiles`.

Tokens and bearers are unversioned hexadecimal strings generated directly from
the operating system CSPRNG; they do not contain encoded metadata and do not
have `auv1`/`auvd1` protocol prefixes. Only SHA-256 digests are persisted by
the daemon and comparisons are constant-time. A token has no expiry unless the
owner explicitly supplies a TTL, and it can never be consumed twice. Every
later request resolves the bearer against the current pairing-store snapshot,
so disable and revoke take effect on the next request without certificates,
CRLs, or refresh protocols. The pairing store's file-schema version is an
independent persistence concern and remains explicit.

### Run

A Run is one explicitly created correlation and control scope. It exists while
work is pending/running and remains the same resource after work terminates.
`Workflow` and `Execution` are not separate public concepts for this purpose.

Expected phases are:

```text
Pending -> Running -> Succeeded | Failed | Canceled
```

A Run may:

- select several Devices;
- hold leases on several Runners;
- contain several typed operations;
- provide Device/Runner context to application plugins;
- correlate future trace records, artifacts, and Inspect projections.

An ordinary one-shot CLI command may create and close an implicit Run. A caller
may instead create a longer-lived Run and append later plugin/core calls to it.
This document does not require every API call to be durably stored yet.

### Session

Session is no longer a public AUV control-plane resource. It may remain an
implementation term inside a Driver, ONNX runtime, SDK connection pool, or
Runner. Such an internal session must not become canonical Run identity or a
cross-service authentication credential.

### RunnerClass and RunnerProvider

A `RunnerClass` is the public description of a kind of Runner a Device can
create. It identifies lifecycle/configuration schema and the service bundle the
resulting process may claim.

A `RunnerProvider` is the daemon-side implementation that realizes and manages
a RunnerClass. It may start an arbitrary executable, connect an existing gRPC
endpoint, validate its runtime protocol and business descriptors, and translate
Runner resource lifecycle into transport-specific actions.
RunnerProvider is an implementation boundary; RunnerClass is the discoverable
control-plane resource.

The provider transport is a Rust/configuration concern, not a protobuf union:

```rust
enum RunnerRuntime {
  Executable(ExecutableRunnerRuntime),
  RemoteGrpc(RemoteGrpcRunnerRuntime),
}
```

`Executable` contains an absolute executable plus arguments. The executable
has no required filename prefix and may be `auv` itself, an app binary, or any
operator-approved compatible program. `RemoteGrpc` contains an endpoint for an
already-running compatible runtime. Both variants implement the same typed
gRPC runtime and business-service contracts, so Device/Run/Runner resources do
not expose which variant was selected.

Examples of provisional RunnerClass identities are:

```text
auv.core.local
auv.app.netease_music
auv.app.apple_music
auv.game.balatro
```

A RunnerClass identity does not imply one RPC or one CLI command. A NetEase
Music Runner may implement playback, playlist, search, and recommendation
services while the `auv-netease-music` frontend owns many nested subcommands.

### Runner

A Runner is one live daemon-owned process/runtime on exactly one Device. It has:

- a stable Runner ID for its lifetime;
- a RunnerClass reference;
- labels;
- a lifecycle policy;
- readiness/health state;
- private daemon IPC;
- an authoritative typed capability manifest derived from registered protobuf
  services;
- active Run leases and operations.

A Runner does not span several Devices. Composition across Devices belongs to a
Run.

Runner lifecycle policies are:

- `ephemeral`: stop after the last Run releases it;
- `unless-idle`: stop after it has no active Run leases and no active
  operations for the configured idle timeout;
- `unless-shutdown`: do not stop automatically when idle; remain ready until
  explicit stop or Device/daemon shutdown.

For lifecycle decisions, a Runner is idle only when both
`active_run_leases == 0` and `active_operations == 0`. An `unless-idle` Runner
starts its idle timeout after both conditions become true; becoming idle does
not stop it immediately.

Stopping a Run releases its Runner leases. It does not necessarily destroy a
Runner using `unless-idle` or `unless-shutdown`.

### Runner lease and claim

A Run uses a Runner through a lease. The lease is retention evidence, not
authentication identity.

The scheduler input should be modeled as a `RunnerClaim` (name provisional),
not an `AcquireSession` verb. A claim can describe:

- target Device or Device selector;
- required typed services/method capabilities;
- RunnerClass constraint;
- label selector;
- resource/provider constraints;
- reuse/lifecycle preference;
- lease deadline.

The daemon may bind the claim to a compatible ready Runner or create another
Runner. Reuse is a scheduling result rather than a special action on a session
collection.

### Labels and capabilities

Labels are user/provider metadata for selection, for example:

```text
gpu=4090
location=lan
workload=balatro
```

Capabilities are authoritative versioned API facts, for example:

```text
auv.api.driver.v1.WindowService
auv.api.driver.v1.TextRecognitionService
auv.game.balatro.v1.BalatroDetectionService
auv.netease_music.v1.PlaylistService
```

The daemon may project capabilities into system labels such as
`auv.dev/capability.ocr=true` to support selectors. A caller-writable label
cannot prove that a Runner implements a service. Scheduling and authorization
must validate the registered descriptor/capability manifest.

### Caller

A Caller is the authenticated identity established by the daemon's transport
authentication layer. A Device is the target execution/trust boundary; it does
not replace caller identity.

Authorization evaluates at least:

```text
Caller + target Device + optional Run + Runner + protobuf service/method
```

Runner processes trust only their parent daemon connection and do not repeat
external authentication. The daemon may forward read-only caller/Run
metadata for domain logging, but that metadata is not a credential.

## CLI contract

### Core operations and plugins are separate surfaces

Core atomic operations remain under:

```text
auv invoke display.list
auv invoke window.capture
auv invoke input.clickWindowPoint
```

Only operations registered by `auv-cli-invoke` are reachable through this
surface. Application/game plugins are not converted to core command IDs and
are not routed through a generic protobuf Invoke RPC.

`auv runner` is limited to Runner resources (`create`, `list`, `classes`,
`get`, and `stop`). Driver capabilities do not become Runner subcommands:
display and window listing remain `auv invoke display.list` and
`auv invoke window.list`. Human-facing Device, Device profile, Run,
RunnerClass, and Runner output uses the shared `auv-cli-common` typed table
schema; `--json` retains the stable structured projection.

External frontends use top-level plugin names:

```text
auv netease-music playlist list
auv netease-music playlist play "Daily Mix"
auv netease-music daily-recommended songs
auv apple-music search "Song"
auv balatro observe
```

The root resolves `auv-<plugin>` on `PATH`, preserves stdin/stdout/stderr and
exit status, and gives built-ins precedence. The plugin owns every argument
after its top-level name.

### Parent context flags

Device and Run selection are root context. They must work before every built-in
or external top-level command:

```text
auv --device mac-mini --run 5c67cfa51dd1 netease-music playlist play "Daily Mix"
auv --device-id 9a72c153c6de balatro observe
auv --run 5c67cfa51dd1 invoke display.list
```

Expected Device resolution:

1. `--device-id` selects a stable ID exactly or by a unique hexadecimal prefix.
2. `--device` selects by human-facing name.
3. A name with one match succeeds.
4. A duplicate name returns an ambiguous-selection error with candidate IDs.
5. Name and ID supplied together must resolve to the same Device.
6. No Device option selects the implicit local Device.

Expected Run behavior:

- `--run <id>` appends the call to an existing target Run;
- absence creates an implicit Run according to the frontend's one-shot policy;
- the plugin receives the resolved Run reference, not permission to invent a
  Run ID.

Arguments after the plugin name remain plugin-owned:

```text
auv netease-music --device mac-mini playlist list
```

This form works only when that plugin elects to expose and parse a Device
override. AUV client/CLI utilities should make this inexpensive. The guaranteed
cross-plugin inheritance form puts root flags before the plugin name.

### Kubectl comparison

Kubectl provides the executable naming/process model but does not inject its
resolved namespace, context, or kubeconfig into plugins. Current kubectl:

- searches for `kubectl-*` executables;
- rejects flags before an unresolved plugin name;
- forwards arguments after the matched name unchanged;
- inherits the existing environment and adds `KUBECTL_PATH`;
- expects plugins to add `genericclioptions.ConfigFlags`, reload kubeconfig,
  and construct their own typed/dynamic clients.

AUV intentionally defines a stronger contract: the root may parse parent
Device/Run flags, resolve ambiguity once, and inject a normalized context.
Evidence and fixed source links are in
[`../invoke-cli/2026-07-31-kubectl-plugin-context-research.md`](../invoke-cli/2026-07-31-kubectl-plugin-context-research.md).

### AuvContext process contract

The root injects the resolved invocation context directly as a small JSON
environment value:

```text
AUV_CONTEXT={"device_id":"9a72c153c6de...","run_id":"5c67cfa51dd1...","daemon_endpoint":"unix:///...","config_profile":"default"}
AUV_PATH=/absolute/path/to/auv
```

`AUV_CONTEXT` carries references, not secrets:

- selected config profile;
- canonical Device ID and optional display name;
- optional Run ID;
- daemon endpoint/discovery reference;
- invocation identity metadata.

It must not contain bearer tokens, private keys, or reusable Device credentials.
The normal AUV client transport resolves the selected profile and performs
bearer authentication. The root passes the value through the process API rather than
shell interpolation, so the JSON does not require shell-specific quoting.

The context remains small enough for an environment value. New optional fields
may be added, parsers ignore unknown fields, and missing fields use the normal
configuration/default resolution rules. There is no separate context version
variable. If a future contract cannot evolve additively, it requires a new
process contract rather than an independently versioned context file.

`AUV_PATH` is independent of `AUV_CONTEXT`. It is the absolute path of the root
`auv` executable that launched the plugin, allowing a plugin to invoke the same
AUV installation instead of resolving another binary through `PATH`. Plugins
that do not invoke the root CLI may ignore it. It does not select Device, Run,
configuration, credentials, or daemon transport.

Library entry points should separate parsing from connection construction:

```rust
let context = AuvContext::from_env()?;
let client = Client::from_context(context).await?;
```

A convenience `Client::from_env()` may compose both steps. Plugin-owned
flags can override injected context only when the plugin deliberately supports
that override. The expected precedence is:

```text
plugin explicit flags
  > root-injected AuvContext
  > selected user configuration
  > implicit local Device / implicit Run policy
```

Plugin stdout remains user/data output and stderr remains diagnostics. Run
recording must use the Run/client API instead of interpreting arbitrary stdout
as a hidden protocol.

## Local and remote hierarchical clients

The Device/Runner layer selects transport. Application and driver code should
consume capability clients whose public hierarchy does not expose that choice.
Every Device-, Run-, and Runner-producing entry point that can operate locally
or remotely uses automatic target selection by default. Automatic selection
combines explicit options, inherited `AuvContext`, an existing Run's placement,
Runner claims, and configured Device availability. With no stronger selection,
it resolves to the implicit local Device.

The same entry points provide a `local` shortcut that constrains the resulting
Device, Run placement, or Runner claim to the implicit local Device and forbids
remote fallback or offload. The shortcut does not bypass the Device, Run,
Runner, lease, capability, or authorization models; it only fixes placement.
Whether local execution uses an in-process driver backend or a daemon-owned
local Runner remains an implementation choice of the selected API surface.

The exact Rust spelling remains provisional, but the paired behavior is:

```rust
let run = auv.run(options).await?;                 // automatic local/remote selection
let run = auv.local().run(options).await?;         // force local placement

let runner = run.runner(options).await?;           // automatic local/remote selection
let runner = run.local().runner(options).await?;   // force a local Runner
```

An explicit `local` shortcut combined with a conflicting remote Device ID or
selector is a validation error. The client must not silently ignore either
constraint.

```rust
let runner = auv.runner(options).await?;
let window = runner.windows().resolve(selector).await?;
let match_ = window.find_text(query).await?;
```

Implementation status (updated 2026-08-02): `auv-api-client` now exposes the
business hierarchy `Client -> RunClient -> RunnerClient -> capability child`.
The package-shaped `protocol::grpc::clients::{daemon,driver,inference}` modules
own each protobuf service implementation, retain explicit `v1` levels, and
nest macOS below `driver`. `protocol::grpc::Client` supplies their shared
channel, authorization, and Runner-lease transport. `Client`
inherits `AUV_CONTEXT` when present and otherwise discovers the current user's
local daemon. Run options can explicitly attach to a Run, explicitly create a
new one, or inherit/create automatically. Device ID/name ambiguity and Run
membership are validated before creation or claim. A client-created Run is
owned and has an explicit outcome cleanup path; a context/explicit Run is
borrowed and remains open. Failed claims compensate an implicitly created Run
with a canceled outcome.

The business `Client` currently contains the concrete gRPC adapter. A
protocol-neutral adapter interface remains intentionally deferred until an
owner-approved non-gRPC protocol supplies a second implementation and its
producer/consumer requirements can define that seam.

`Client::local` and `RunClient::local` are placement constraints: they reject
paired remote transports and explicit remote Devices instead of silently
overriding them. An explicitly constructed paired `protocol::grpc::Client`
enters the business hierarchy through `Client::from_grpc`. `RunnerOptions` owns class, capability, label,
reuse, lifecycle, and idle policy while the parent injects the
canonical Run and selected Device refs. Selected core invoke adapters consume
this hierarchy rather than constructing raw claims in the CLI frontend.
When an invoke appends to a selected control-plane Run, its tracing `RunId`
uses the same canonical UUID payload instead of allocating a second unrelated
identity; malformed external Run IDs fail closed at that projection boundary.

`RunnerClient -> WindowsClient -> WindowClient` retains the returned
`WindowRef`; `WindowClient::capture`, `find_text`, and `click` send that exact
child reference rather than repeating a selector. `DisplaysClient` and
`InputClient` provide the adjacent portable capability surfaces. Platform
extensions remain hierarchical rather than being flattened into
`RunnerClient`: the implemented macOS branches are
`runner.macos().permissions().probe()` and
`runner.macos().media().now_playing()`, with adjacent application activation
and accessibility focus clients. Portable overlays remain at
`runner.overlay()`. `RunnerExecution` forwards these child clients, as well as
display, window, input, OCR, and inference access, without exposing the chosen
transport. The selected Device/Runner determines
whether those calls use a local child or a paired remote daemon. A non-macOS
local Runner does not advertise either macOS service, so an exact claim fails
with gRPC `UNIMPLEMENTED` rather than constructing a client for a capability
that is not present.

Configured paired Device selection is implemented. The provisional management
surface is:

```text
auv devices profiles list
auv devices profiles get <profile>
auv devices profiles create <profile> \
  --device-id <canonical-id> --device-name <name> \
  --endpoint http://host:port \
  --device-credential <opaque-bearer>
auv devices profiles update <profile> ...
auv devices profiles delete <profile>
```

One profile contains endpoint, canonical target Device identity, and the opaque
bearer. `AUV_CONTEXT` contains only the profile name and never copies the
bearer into a plugin environment. Profile parsing accepts unknown fields for
forward evolution and works on non-Unix platforms; it does not impose owner,
mode, symlink, size, or certificate-file admission policy. Writes retain an
inter-process mutation lock plus atomic replacement so concurrent writers do
not silently lose updates and damaged JSON is not overwritten.

`auv devices list` merges daemon-visible Devices with configured paired
Devices and concurrently probes every configured profile. A profile is
`online` only after its endpoint accepts the stored bearer and returns the
configured canonical Device; transport failure is `offline`, rejected
authentication is `unauthorized`, profile/identity mismatch is `invalid`, and
other probe failure is `error`. Therefore `offline` never means merely
"configured but not checked." Same-name Devices remain distinct by canonical
ID. `AUV_CONTEXT` carries only profile names and Device/Run references; bearer
material is resolved only inside the client library.

New Device and Runner resources use opaque, unversioned 256-bit random
hexadecimal IDs. Runs use an opaque 128-bit random identity rendered as compact
hexadecimal by the control API; the same bytes remain the existing
tracing/run-record UUID identity without a second mapping ID. Structured output
and API messages retain the full canonical ID.
Human-facing tables show the first 12 hexadecimal characters, and CLI
resource arguments accept any unambiguous prefix. Names remain the preferred
human selector where the resource has a name; prefixes are a convenience, not
an authentication mechanism. Existing persisted `device_<UUID>` identities
remain readable only as a migration boundary and are not generated for new
stores.

Root `-v` enables connection and RPC diagnostics on stderr; repeating it raises
the tracing level. Diagnostics may include endpoint, service, method, selected
profile, and credential file path, but must never include bootstrap tokens,
bearers, or serialized command arguments containing secrets.

Automatic cross-daemon offload/coordinated multi-Device scheduling remains
incomplete. The daemon's current built-in pool still contains only its
persistent local Device, so this implementation must not claim that
cross-Device scheduling has been proven.

The local backend calls `auv-driver`; the remote backend calls generated gRPC
clients. A child client retains the references needed by later methods:

```text
DeviceRef -> RunnerRef -> WindowRef -> method request
```

Capability boundaries should include, as needed:

- `DisplayApi`;
- `WindowApi`;
- `CaptureApi`;
- `TextRecognitionApi`;
- `InputApi`;
- `InferenceApi`;
- app-owned APIs such as NetEase Music playlists/playback/search.

Service registration is flat. Facades can remain hierarchical:

```rust
client
  .netease_music()
  .playlists()
  .play("Daily Mix")
  .await?;
```

Child resources preserve parent references instead of re-resolving Device,
Runner, app, and window state at every method.

## Daemon and Runner process model

### Foreground serving and service supervision

The intended foreground command is:

```text
auv serve --listen unix:///path/to/auv.sock --listen http://0.0.0.0:9847 \
  --pairing-store /path/to/pairings.json
```

Listener type is transport configuration, not a distinct server role. A future
service-manager frontend may provide:

```text
auv daemon start
auv daemon status
auv daemon stop
```

It should manage launchd/systemd/brew-service integration and ultimately run
the same `auv serve` implementation. Until supervision exists, `auv serve` is
the honest foreground surface.

Implementation status (2026-07-31): repeated `--listen` is implemented. One
foreground daemon binds every listener before publishing readiness, then serves
them through one shared control plane and Runner supervisor. Unix/loopback
listeners retain local authority; TCP listeners configured with a pairing store
require a live Device bearer on every ordinary request. `PairDevice` is the
sole unauthenticated bootstrap RPC. Failure to bind any listener drops listeners already
bound in the same attempt and removes their owned Unix sockets. An unexpected
failure of one serving task cancels the others; daemon idle monitoring and
handler shutdown remain process-wide and run once.

The discovery descriptor publishes one caller-local endpoint only, preferring
Unix and then loopback TCP. Remote endpoints are printed as bound endpoints
but are not published as credential-free local discovery. A remote-only
foreground command simply has no local descriptor to publish; it does not
require an otherwise meaningless `--no-discovery` flag. Runs remain
Caller-scoped even though all listeners share the canonical Device,
RunnerClass registry, and daemon-global resources.

### Daemon-managed Runner runtimes

The daemon creates and governs Runner resources. Clients always call the
authenticated Device daemon; they never select a provider transport directly.
For an executable runtime, the process relationship is:

```text
client
  -> authenticated daemon listener
  -> method authorization and Runner routing
  -> private inherited IPC
  -> Runner child process
```

On Unix, the daemon can create a socketpair and pass one connected descriptor
to the child. Windows can use an inherited named-pipe/handle equivalent. The
Runner is not an API server visible on the Device network. If a third-party
process independently exposes another endpoint, that endpoint is outside the
AUV Runner protocol and is not used for daemon discovery, authentication, or
routing.

The private stream should carry standard HTTP/2 gRPC where practical. This
retains unary/streaming semantics, deadlines, cancellation, metadata, flow
control, health, and reflection without inventing another multiplexed protocol.
An inherited connected stream is an implementation adapter, not a public
listener. There is no `auv-` filename requirement for Runner executables. The
first-party `auv.core.local` provider spawns the `auv` executable with a private
internal role argument. Application capabilities such as Balatro object
detection are owned and served by their application Runner binaries, then
registered explicitly as custom providers; they are not bundled into `auv
serve`. Internal roles are not CLI subcommands and do not appear in help.

For `RemoteGrpc`, the daemon connects an already-running endpoint and subjects
it to the same Health, Reflection, runtime-metadata, status, and reflected
business-service readiness checks. The daemon still owns the local Runner
resource, leases, authorization, and routing, but does not own the remote
process. Deleting/stopping the local Runner drops that attachment; it does not
kill the endpoint and does not implicitly call endpoint-wide `Drain`.
Authenticated outbound endpoint credentials and remote status-watch
retry/backoff are deliberate follow-up slices.

The daemon owns:

- executable/provider selection;
- process creation and termination;
- inherited handle setup;
- readiness and health;
- crash/restart policy;
- lease and idle lifecycle;
- operation tracking and draining;
- runtime reflection and discovery projection;
- external authentication/authorization;
- gRPC/HTTP routing.

The Runner runtime owns app/inference/driver service behavior and protobuf
decoding for its own methods.

The implemented crash policy detects child exit and retains the Runner record
in `FAILED`; a later claim can create a replacement. Automatic restart,
backoff, and crash-loop suppression are intentionally deferred until their
operator-visible policy is approved.

Runner executables are operator-trusted code in the first implementation. The
daemon inherits its environment, applies configured per-Runner overrides, and
adds the fixed private IPC descriptor contract. This preserves XDG, Wayland,
DBus, loader, GPU, and future platform launch context without requiring AUV to
enumerate every environment variable. A socketpair is private
routing, not a sandbox: another process with the daemon's uid may still reach
same-user resources, including a local daemon socket. Treating community code
as untrusted requires a later platform sandbox or separate uid together with a
distinct child Caller. The daemon must not claim that process isolation is
already an authorization boundary.

Executable configuration accepts an absolute path, a manifest-relative path,
or a bare name resolved through the daemon's `PATH`. Registration does not
preflight ownership, mode bits, symlinks, descriptor files, or executable
contents. The operator's act of registering the provider is the trust decision;
process startup and reflection report real runtime failures.

### Required Runner runtime protocol

Every Executable or RemoteGrpc runtime implements the stable typed control
service below in addition to gRPC Health, Reflection, and its business
services:

```proto
service RunnerRuntimeService {
  rpc GetMetadata(GetMetadataRequest) returns (GetMetadataResponse);
  rpc GetStatus(GetStatusRequest) returns (GetStatusResponse);
  rpc WatchStatus(WatchStatusRequest)
      returns (stream WatchStatusResponse);
  rpc Drain(DrainRequest) returns (DrainResponse);
}

message RunnerRuntimeStatus {
  RunnerRuntimePhase phase = 1;
  RunnerRuntimeOperationsStatus operations = 2;
  google.protobuf.Timestamp observed_at = 3;
}

message RunnerRuntimeOperationsStatus {
  uint64 active = 1;
}
```

Metadata is stable identity/admission evidence: RunnerClass, display name, and
labels. Status is dynamic runtime evidence. Its phase is one of
`STARTING`, `READY`, `DRAINING`, `STOPPING`, or `FAILED`; `UNSPECIFIED` is never
ready. `active` is an instantaneous gauge and does not replace daemon-owned Run
lease accounting. The common Runner adapter holds an
operation permit from business-RPC admission until the complete response body
or stream is finished or dropped, so `active` does not fall back to zero merely
after response headers are produced.

`WatchStatus` sends a complete snapshot immediately and then another complete
snapshot after each observable change. After interruption a caller reconnects
and calls `GetStatus`; it does not rely on replay tokens. `Drain` idempotently
stops new operation admission and waits for active operations indefinitely when
`grace_period` is absent. When present, the duration is the maximum wait; a
present zero duration performs an immediate deadline check. A successful Drain
signals a daemon-owned Executable Runner to leave its serving loop; absent an
explicit deadline, the daemon waits for that orderly exit without a hidden
timeout. `--force` is the explicit immediate-termination path. Ordinary
RemoteGrpc detach never invokes Drain and never owns or terminates the remote
endpoint.

`RunnerRuntimeCondition`/`conditions` are intentionally absent. They should be
added only when a scheduler or Inspector consumer needs structured reasoned
health history; phase, operation gauges, timestamp, Health, and RPC status are
the current contract. Runtime lifecycle policy remains daemon-owned rather than
self-declared by runtime metadata.

## Protobuf and custom Runner APIs

### Daemon control-plane schema ownership

The daemon protobuf package contains the shared control-plane resources and stable
annotations, for example:

```text
auv/api/daemon/v1/device.proto
auv/api/daemon/v1/run.proto
auv/api/daemon/v1/runner.proto
auv/api/runner/v1/runtime.proto
auv/api/annotations/v1/annotations.proto
```

AUV does not define a generic `types`, `common`, or `shared` protobuf package.
A reusable message belongs to the narrow domain that owns its meaning and
compatibility. A message without a clear shared owner remains in its consuming
package until that ownership is established.

### Image values and coordinate-space ownership

Reusable encoded/raw image representations belong to a narrow image value
package that Driver, inference, and custom APIs may import:

```text
auv/api/image/v1/image.proto
auv/api/image/v1/region.proto
```

`auv.api.image.v1` may own values such as `RgbFrame`, `ImageSize`,
`PixelFormat`, `PixelRect`, and `NormalizedRect`. It does not own capture,
recognition, or inference service behavior.

AUV does not define one global `geometry` package or a coordinate-free `Rect`.
Coordinate spaces are part of the type's meaning:

- screen/window geometry such as `ScreenPoint`, `ScreenRect`, `WindowPoint`,
  and `WindowRect` belongs to `auv.api.driver.v1`, with source in
  `auv/api/driver/v1/geometry.proto`;
- image regions such as `PixelRect` and `NormalizedRect` belong to
  `auv.api.image.v1`;
- an inference-specific box remains in its inference package when its contract
  differs from the shared image-region semantics.

The retired `auv.api.types.v1.Rect`, `RatioRect`, and `RgbFrame` were migrated
by meaning rather than moved together under a new catch-all package. Every
former `Rect` use now declares whether its coordinates are screen, window,
image-pixel, or normalized space.

Each public RPC has a dedicated method-specific request and response. Generic
`command_id`, JSON bytes, `Struct`, `Any`, or a catch-all operation envelope do
not represent first-class AUV operations.

### First-party Driver schema ownership

The protobuf projection of portable `auv-driver` capabilities lives under
`auv.api.driver.v1`. Platform-specific Driver APIs use a platform subpackage:

```text
auv/api/driver/v1/display.proto
auv/api/driver/v1/window.proto
auv/api/driver/v1/capture.proto
auv/api/driver/v1/geometry.proto
auv/api/driver/v1/text_recognition.proto
auv/api/driver/v1/input.proto
auv/api/driver/macos/v1/accessibility.proto
auv/api/driver/windows/v1/ui_automation.proto
```

Portable Driver domains define dedicated services rather than placing their
methods in the control-plane package:

```proto
service DisplayService {
  rpc ListDisplays(ListDisplaysRequest) returns (ListDisplaysResponse);
}

service WindowService {
  rpc ListWindows(ListWindowsRequest) returns (ListWindowsResponse);
  rpc ResolveWindow(ResolveWindowRequest) returns (ResolveWindowResponse);
}

service CaptureService {
  rpc CaptureDisplay(CaptureDisplayRequest) returns (CaptureDisplayResponse);
  rpc CaptureWindow(CaptureWindowRequest) returns (CaptureWindowResponse);
}

service TextRecognitionService {
  rpc RecognizeText(RecognizeTextRequest) returns (RecognizeTextResponse);
  rpc FindWindowText(FindWindowTextRequest)
      returns (FindWindowTextResponse);
}

service InputService {
  rpc ClickWindowPoint(ClickWindowPointRequest)
      returns (ClickWindowPointResponse);
  rpc ClickScreenPoint(ClickScreenPointRequest)
      returns (ClickScreenPointResponse);
  rpc TypeText(TypeTextRequest) returns (TypeTextResponse);
  rpc PressKey(PressKeyRequest) returns (PressKeyResponse);
}

service OverlayService {
  rpc ShowOverlay(ShowOverlayRequest) returns (ShowOverlayResponse);
  rpc RemoveOverlay(RemoveOverlayRequest) returns (RemoveOverlayResponse);
}
```

Protobuf services do not inherit from one another. A platform extension is an
additional service in `auv.api.driver.<platform>.v1`, not a platform-dependent
reinterpretation of a portable service. A Runner advertises the exact portable
and platform services/methods it implements through its validated capability
manifest and reflection descriptors.

Calling a Driver method that the selected Runner or its platform does not
implement returns gRPC `UNIMPLEMENTED` (code 12), and the daemon preserves that
status. The daemon should reject the call from its capability snapshot before
forwarding when possible; the Runner remains authoritative if the snapshot is
stale. `UNIMPLEMENTED` is not used for a missing OS permission, temporary
unavailability, a missing target resource, or an invalid request. Those map to
`PERMISSION_DENIED` or `FAILED_PRECONDITION`, `UNAVAILABLE`, `NOT_FOUND`, and
`INVALID_ARGUMENT`, respectively.

### Custom schema ownership

Application/game Runner schemas live with their owning projects/packages:

```text
auv-netease-music/proto/auv/netease_music/v1/...
auv-game-balatro/proto/auv/balatro/v1/...
```

They do not move into the root AUV proto tree merely because a daemon can route
them. A custom schema may depend on the versioned AUV annotation module when it
wants DevTools discovery, and imports only the explicitly owned AUV value
packages it needs. It generates its own SDK artifacts.

A NetEase Runner may expose several services:

```text
auv.netease_music.v1.NeteaseMusicService
auv.netease_music.v1.PlaylistService
auv.netease_music.v1.PlaybackService
auv.netease_music.v1.DailyRecommendedService
```

### AUV annotations

AUV annotations are optional developer-tool metadata. They let DevTools,
Inspector, capability panels, and a future JavaScript REPL select concrete typed
RPCs from reflected descriptors. They do not define routing, authentication,
authorization, Device context, or Run context.

The annotation module extends protobuf method options with one exposure marker
and one effect classification. Exact field numbers and enum names remain
provisional until the first schema slice is approved:

```proto
enum MethodEffect {
  METHOD_EFFECT_UNSPECIFIED = 0;
  METHOD_EFFECT_READ_ONLY = 1;
  METHOD_EFFECT_MUTATION = 2;
  METHOD_EFFECT_INPUT = 3;
}

extend google.protobuf.MethodOptions {
  bool discoverable = 51001;
  MethodEffect effect = 51002;
}
```

`discoverable = true` means developer tools may display the method, construct
its concrete request from the reflected descriptor, and offer an interactive
typed call. `effect`, when supplied, lets tools label read-only, mutation, and
input-delivery calls and request appropriate user confirmation. There is no separate `callable` option because
interactive calling is part of discoverability.

An unannotated method remains a normal typed gRPC method and generated clients
may call it. It is only absent from generic developer-tool discovery. A
`discoverable` annotation does not grant permission: the target Device's daemon
still authenticates the Caller and applies its trusted method policy before
forwarding. Every Runner call is already scoped to its Device. Run association,
when present, is propagated uniformly by the client/control plane and is not a
per-method annotation concern.

API package/version, service and method identity, request/response shape, and
streaming cardinality come from the standard protobuf descriptor. RunnerClass
ownership comes from the RunnerClass selected by the operator. Authentication
and authorization remain daemon policy rather than Runner annotations.

HTTP bindings use standard `google.api.http`; they are not duplicated in an
AUV-specific path annotation. Every HTTP-exposed request must use typed fields,
Protovalidate rules, and matching OpenAPI documentation. gRPC-only services do
not import HTTP/OpenAPI annotations.

### Reflection, registration, and routing

A Runner provides gRPC health, reflection, and its typed services on private
IPC. During readiness, the daemon:

1. obtains the Runner's descriptors through reflection;
2. verifies mandatory runtime metadata matches the selected RunnerClass;
3. builds the live route/capability snapshot from reflected business services;
4. derives a developer-tool catalog from `discoverable` and `effect` options;
5. marks the Runner ready only after health and reflection succeed.

Reflection describes the live schema; it is not caller authority. A child may
not replace daemon-reserved core, runtime, health, or reflection services, but
operator-registered custom business services do not require a preinstalled
descriptor snapshot.

Implementation status (2026-07-31): `auv serve` and the legacy
`auv api-server serve` frontend accept repeatable
`--runner-provider <manifest.json>` options. Each manifest is operator-owned
daemon configuration, loaded before bind succeeds. It contains only the
RunnerClass and one Rust `RunnerRuntime` configuration. An Executable runtime
accepts a bare `PATH` name, manifest-relative path, or absolute path plus
arguments, optional working directory, and environment overrides. A
RemoteGrpc runtime contains an endpoint and is connected rather than spawned.
Unknown provider JSON fields are accepted for forward evolution; registration
does not preflight the executable, endpoint, filesystem ownership, descriptor
digest, service list, or lifecycle.

The current custom-provider slice starts or connects the selected runtime, then
requires Health, `RunnerRuntimeService` metadata/status, and Reflection before
the Runner becomes ready. Reflected business services and methods become the
Runner's live capabilities. External gRPC calls carry a
typed Runner lease in binary metadata; the daemon removes that routing
metadata and external credentials, authorizes the Device call, atomically
admits an operation permit, and forwards the opaque gRPC body and trailers to
the private child. The permit remains held until response completion or
cancellation. Streaming bodies are forwarded without a separate allowlist;
their protobuf RPC defines ordering, backpressure, cancellation, and terminal
status. The developer-tool catalog includes only reflected methods annotated
`discoverable = true`; `effect` is auxiliary metadata and may be unspecified.
Private Runner reflection preserves custom method options. The daemon exposes
typed service discovery rather than requiring clients to know a static
provider manifest.

External gRPC requests terminate at the daemon. The daemon authenticates,
authorizes the concrete method according to the target Device policy, selects a
Runner, and forwards the call over private IPC. Streaming calls must preserve
cancellation, ordering, backpressure, and terminal gRPC status. There is no
generic base stream event; each streaming RPC defines concrete packet/event
messages and `oneof` variants when needed.

## REST resources and discovery

Implementation status (updated 2026-08-01): the daemon exposes the paths in this
section as protobuf-over-HTTP routes backed directly by generated daemon protobuf
messages. Discovery, the persistent local Device, caller-scoped in-memory
Runs, and the daemon Device/Run/Runner/RunnerClass gRPC services are implemented.
Typed service discovery is available through gRPC
`DiscoveryService.ListServices` and `GET /apis/auv/daemon/v1/services`. Both
project only methods visible to the authenticated Caller; a Caller with
control-plane inspection but without operation execution sees an empty service
catalog. The Rust `auv-api-client` exposes the same operation as
`Client::list_services()`, and generated JavaScript clients receive it from the
shared protobuf schema.
The CLI exposes `devices`, `run`, `runner`, and `runner classes` management
commands against those typed services. Root `--device`, `--device-id`, and
`--run` selectors are resolved through the daemon and inherited by plugins as
inline `AUV_CONTEXT`; `auv-api-client` provides `from_context` and `from_env`
constructors. A selector that names a Device without a Run creates an implicit
Run. For ordinary child exit, the root frontend maps the plugin exit code to a
terminal Run outcome and stops that implicitly owned Run; an explicitly named
Run remains open for later calls. Abrupt signal forwarding and bounded cleanup
are still marked as a follow-up at the process call site. Daemon-free plugin
invocation with no selector deliberately remains
independent until the implicit one-shot Run policy is approved. The selected
core-invoke adapters currently implemented are:

- `app.probePermissions`;
- `display.list` and `display.capture`;
- `screen.captureRegion`, `screen.findText`, `screen.waitForText`, and
  `screen.clickText`;
- `window.list`, `window.capture`, `window.findText`, `window.waitForText`, and
  `window.clickText`;
- `input.typeText`, `input.pasteText`, `input.key`, and
  `input.clickWindowPoint`;
- `input.focusText` and its compatibility alias `input.axFocusText`;
- `app.activate`;
- `mediaControl.nowPlaying`, `mediaControl.play`, `mediaControl.pause`,
  `mediaControl.togglePlayPause`, `mediaControl.next`, and
  `mediaControl.previous`;
- `overlay.outline`, `overlay.cursor`, `overlay.status`,
  `overlay.captureFrame`, and `overlay.clickTarget`.

`auv --run <id>
invoke <operation>` (or a root Device selector)
claims a typed local Driver Runner, calls the operation's concrete Driver RPC,
releases its lease, and renders the same direct-result schema as local invoke.
Pure input mapper tests reject malformed click counts/durations, non-finite
Window/Screen points, empty paste text, unknown paste submit values, and
negative paste settle durations. The real daemon regression sends a non-finite
`ClickScreenPoint`, an empty key, and an empty `PasteText` through admitted
leases and observes `INVALID_ARGUMENT` before native delivery or clipboard
mutation. It never sends a valid click, key, type, or paste event. Window-point
coverage claims both
`WindowService/ResolveWindow` and `InputService/ClickWindowPoint`, retains the
resolved Window context, and rejects an out-of-bounds point before delivery.
Screen text-click coverage claims
`TextRecognitionService/FindDisplayText` and
`InputService/ClickScreenPoint`, projects the OCR best match into a typed
screen point, and reuses the transport-independent result/artifact builder.
Its daemon/Runner regression sends a non-finite point and proves rejection
before native delivery rather than injecting a real click.
Remote capture reconstructs the
canonical RGBA frame and records the same PNG artifact purpose as local
capture. A 2026-07-31 macOS black-box run transferred a 6016x3384 RGBA8 display
capture through CLI -> daemon -> Runner, returned capture metadata,
and persisted a verified PNG artifact. Capture and OCR transports remove
tonic's small default unary ceiling and add no AUV-specific message-size policy;
live video still requires a separate streaming
protocol. `FindDisplayText` and `FindWindowText` execute capture and OCR inside
one Runner and return that exact source capture as evidence, avoiding an extra
full-frame client round trip. A live macOS `screen.findText` black-box call also
completed through the daemon and persisted its source PNG; a 320x240 logical
`screen.captureRegion` call returned the expected 640x480 Retina frame and a
verified PNG artifact. Every currently registered non-scan invoke command has
an exact typed Runner adapter. `scan.frame` and `scan.coverage` are local
fixture/tooling operations and are intentionally not represented as remote
Driver capabilities.
Overlay dry-run and `--no-overlay` paths validate and render before resolving a
daemon or claiming a Runner. General side-effect-free selected dry-run policy
for other commands remains deferred. On Unix, the
trusted `auv.core.local` RunnerClass self-spawns the current `auv` executable as
an independent local-driver child
over an inherited socketpair, requires health and reflection before `READY`,
and currently exposes typed `auv.api.driver.v1.DisplayService/ListDisplays`,
`auv.api.driver.v1.WindowService/{ListWindows,ResolveWindow}`,
`auv.api.driver.v1.CaptureService/{CaptureWindow,CaptureDisplay,CaptureRegion}`,
`auv.api.driver.v1.TextRecognitionService/{RecognizeText,FindWindowText,FindDisplayText}`, and
`auv.api.driver.v1.InputService/{ClickWindowPoint,ClickScreenPoint,TypeText,PasteText,PressKey}`, and
`auv.api.driver.v1.OverlayService/{ShowOverlay,RemoveOverlay}` through
the daemon. Input methods require `operations_execute` authorization and return
the canonical driver `InputActionResult` projection rather than a parallel
success schema. `ClickScreenPoint` uses a dedicated `ScreenClickOptions`
message, so callers cannot request Window-only policy or routing options that
the global pointer API would have to ignore.

On macOS the same local Runner additionally publishes
`auv.api.driver.macos.v1.PermissionService/ProbePermissions` and
`auv.api.driver.macos.v1.MediaControlService/{GetNowPlaying,Play,Pause,TogglePlayPause,NextTrack,PreviousTrack}`,
`auv.api.driver.macos.v1.ApplicationService/ActivateBundleId`, and
`auv.api.driver.macos.v1.AccessibilityService/FocusText`. Permission status
is an exact four-field projection with explicit granted, missing, and unknown
states; unspecified or unknown wire enum values fail closed. The permission
slice is covered by descriptor and pure mapper tests, runtime reflection,
the `operations_execute` gate, bearer-scope denial for an inspect-only
Caller, exact selected capability matching, and irrelevant `--target`
rejection. It is not live-probed by automated tests because the existing
Automation probe can trigger macOS consent UI. The media slice preserves the
owning `auv-media-macos::NowPlayingState` and `MediaControlOutcome`, including
optional boolean presence, floating-point seconds, before/after evidence, and
the owner's command-specific `verified` semantics. Each mutation is a separate
typed RPC rather than a generic enum invoke. A successful RPC means delivery
and both observations completed; `verified = false` means the fixed observation
window did not prove the postcondition, not that delivery failed. Because a
native failure after delivery has an uncertain outcome, mutation failures map
conservatively to gRPC `UNKNOWN` instead of retryable `UNAVAILABLE`. Pure
injected tests cover read/send/settle ordering and verification without calling
MediaRemote. The slice rejects non-finite backend/wire numbers and reuses the
existing invoke output schema. It is system-wide and rejects `--target` before
local platform access or selected daemon resolution;
the app-filtered NetEase API remains app-owned. Other platforms omit these
macOS-only services from the runtime's reflected capabilities. A 2026-08-01 macOS
black-box run built the actual binaries, started `auv serve` on a temporary
Unix socket, created a Run, selected `mediaControl.nowPlaying` through the root
`--run` context, spawned the local Driver Runner, and returned the live
MediaRemote state through `GetNowPlaying`. The observed Runner advertised the
exact MediaControl, Permission, and portable Driver capabilities; stopping the
Run and daemon completed without sending input or invoking the permission
probe.

Application activation returns typed before/after verification evidence;
accessibility focus accepts the owning `auv-driver` selector/options types and
preserves `input.axFocusText` only as a compatibility alias to the same RPC.
Overlay commands expand their CLI composites into typed portable overlay
messages and use one `ShowOverlay` RPC; malformed geometry and oversized SVG
data are rejected before AppKit access. The macOS local Runner uses a
current-thread runtime on the process main thread so the existing thread-local
AppKit controller remains on its required thread. Automated daemon tests send
only invalid application, accessibility, and overlay requests and therefore do
not activate applications, change focus, or draw UI.

These Driver and macOS platform capabilities are typed gRPC routes and appear
in trusted service discovery; they are not handwritten REST operation
endpoints. Capture preserves its native RGBA8 pixels in
`auv.api.image.v1.RgbaFrame`; object-detection input is explicitly RGB8 rather
than overloading one byte layout with two meanings. Shared image
messages live under `auv.api.image.v1`, while coordinate-space-aware geometry
stays under `auv.api.driver.v1`. Runner reflection publishes a minimal
descriptor closure. Readiness derives available business services and methods
from reflection instead of requiring a daemon-side manifest or descriptor
digest. Typed Runner claims match Device, RunnerClass,
labels, exact service/method capabilities, reuse policy, lifecycle, and idle
policy. Run-owned leases and cancellation-safe operation
permits now drive all three lifecycle policies: `ephemeral`, `unless-idle`, and
`unless-shutdown`; stopping a Run releases all of its leases. Lease TTL/renewal
remains deferred until distributed coordinator clock and ownership semantics
are approved. WATCH and generated
`google.api.http`, Protovalidate, and OpenAPI integration also remain deferred.

A safe end-to-end regression starts the real single-binary `auv serve`
subprocess without sibling Runner binaries, creates
one Run, claims distinct local-driver and ultralytics Runners into that Run,
routes malformed capture and RGB-frame requests without native capture or model
loading, and verifies both leases and child processes are cleaned up. This is
evidence for same-daemon multi-Runner orchestration, not for the deferred
cross-daemon authority model.

The first app-owned custom Runner proof is implemented in
`supported/apps/auv-netease-music`: that package owns
`auv.netease_music.v1.NeteaseMusicService/GetNowPlaying`, its generated client,
and a daemon-supervised Runner binary. `auv --device-id <id> netease-music
now-playing` creates an implicit Run, while `auv --run <id> netease-music
now-playing` attaches to an existing Run; both inherit inline `AUV_CONTEXT`,
claim/reuse the custom Runner, call through the daemon aggregation path, and
release the lease. A macOS black-box run on 2026-07-31 returned the live NetEase
now-playing record, observed the custom child in `RUNNER_PHASE_READY`, and
observed its `unless-idle` process exit after the configured deadline. An
automated integration test uses a deliberately non-matching media owner to
exercise the same daemon spawn, reflection, lease, aggregation, and typed RPC
path without depending on user playback state. This is one concrete custom API
slice, not a claim that every NetEase subcommand has been ported to remote
services.

REST is generated/mapped only after the protobuf service/resource boundaries
are accepted. Handwritten routes do not define a second domain model.

Control resources use an AUV-owned namespace followed by the API group,
version, and collection:

```text
GET    /apis/auv/daemon/v1/devices
GET    /apis/auv/daemon/v1/devices/{device}

GET    /apis/auv/runtime/v1/runners
POST   /apis/auv/runtime/v1/runners
GET    /apis/auv/runtime/v1/runners/{runner}
DELETE /apis/auv/runtime/v1/runners/{runner}

GET    /apis/auv/runtime/v1/runnerclasses
GET    /apis/auv/runtime/v1/runnerclasses/{runner_class}

GET    /apis/auv/runtime/v1/runs
POST   /apis/auv/runtime/v1/runs
GET    /apis/auv/runtime/v1/runs/{run}
POST   /apis/auv/runtime/v1/runs/{run}/stop
POST   /apis/auv/runtime/v1/runs/{run}/runnerleases
DELETE /apis/auv/runtime/v1/runs/{run}/runnerleases/{lease}
```

`auv` is the fixed API namespace; `core` and `runtime` are groups within that
namespace. AUV does not use DNS-style API group names.

Collections may support label/field selectors. LIST responses need a collection
resource version before WATCH is promised. WATCH must define reconnect,
retention, stale-version, and initial-state semantics; copying `?watch=1`
without these guarantees is insufficient.

Each resource family defines its own typed watch stream, for example:

```proto
rpc WatchRunners(WatchRunnersRequest)
    returns (stream WatchRunnersResponse);

message WatchRunnersResponse {
  oneof event {
    RunnerAdded added = 1;
    RunnerModified modified = 2;
    RunnerDeleted deleted = 3;
  }
}
```

This design does not require a bookmark event. Kubernetes bookmarks advance a
client's resumable resource version without sending a full object, but AUV has
not established the event retention/compaction behavior that would justify
that protocol. A resource-specific bookmark may be added compatibly if a later
WATCH implementation demonstrates the need.

The daemon should expose API discovery comparable to:

```text
GET /apis
GET /apis/auv
GET /apis/auv/{group}/{version}
```

Discovery lists resources/services currently registered on the target Device
and visible to the authenticated Caller. It is built from validated Runner
descriptors, RunnerClass policy, and Device authorization. The developer-tool
method catalog further filters this set to methods annotated as `discoverable`.

Custom Runner routing resembles Kubernetes API aggregation more than CRD
storage. CRDs let the primary server generically store a declared resource
schema. AUV custom Runners execute custom service logic; the daemon claims the
public route and proxies calls to a private implementation. Unlike Kubernetes
extension API servers, the Runner stays on daemon-owned private IPC.

## End-to-end call flows

### Plugin call with inherited Device and Run

```text
auv --device mac-mini --run 5c67cfa51dd1 \
  netease-music playlist play "Daily Mix"

root auv
  -> resolve mac-mini to canonical DeviceRef
  -> validate/attach RunRef
  -> serialize the resolved AuvContext into AUV_CONTEXT
  -> exec auv-netease-music with plugin argv

auv-netease-music frontend
  -> Client::from_env()
  -> construct generated PlaylistService client
  -> call PlayPlaylist

daemon
  -> authenticate Caller
  -> authorize Caller + Device + Run + method
  -> find/create a NetEase Runner
  -> lease Runner to Run
  -> proxy typed RPC through private IPC

NetEase Runner
  -> decode its owned protobuf request
  -> execute app-owned operation
  -> return typed response
```

No core generic Invoke RPC participates in this flow.

### Distributed Balatro observation

One Run may schedule:

```text
window.capture
  -> local-driver Runner on mac-mini

text.recognize
  -> vision Runner on gpu-linux-1

object-detection.detect
  -> ultralytics Runner on gpu-linux-2

input operation
  -> local-driver Runner on mac-mini
```

Each operation names typed requirements. The Run correlates the work and holds
leases; each Runner remains owned by exactly one Device.

#### Cross-daemon Run authority is not implemented

This flow is blocked pending an owner-approved distributed authority contract.
The current `RunRef` contains only a daemon-local `run_id`; `CreateRun` accepts
only Devices owned by the serving daemon; and `ClaimRunner` requires a running
Run owned by the authenticated Caller in that daemon's in-memory control
plane. Paired Caller IDs are also local pairing-store identities and are not
a stable cross-daemon subject. Consequently, creating one local Run on each
daemon with the same label, caller-chosen string, or client correlation value
would create several unrelated Runs. It is not an implementation of one Run
using several Devices.

The minimum distributed authority decision must define all of the following
before this flow can be implemented:

- a globally unambiguous Run identity and exactly one authoritative owner,
  provisionally an owner daemon/Device rather than whichever client currently
  holds a connection;
- a typed participant attach/join contract so another Device can retain a
  local projection of the canonical Run without creating another Run;
- owner-issued delegation evidence bound to the canonical Run, participant
  Device, authenticated caller subject, validity window, and revocation or
  generation, plus a cross-daemon Caller mapping that survives local
  pairing IDs and credential rotation;
- idempotent partial-failure semantics for participant attachment and Runner
  claims, including compensation that releases successful leases and reports
  cleanup failures instead of hiding them;
- stop ownership and state transitions while participant leases are being
  released, including the retry/recovery rule when a participant is
  unreachable and whether a non-terminal stopping phase is required;
- discovery and routing rules that locate the owner and participant daemons
  without treating a caller-supplied endpoint or Run reference as authority.

The recommended direction is an owner-daemon canonical Run plus authenticated
participant projections admitted with a narrowly scoped owner delegation. A
client may coordinate the RPC sequence, but it must not become the only durable
authority or turn the Run ID/Runner lease into an authentication credential.
An external coordinator service is a viable alternative if it becomes the
explicit Run owner. A client-only collection of daemon-local Runs is rejected
because it has no single authority, terminal state, or reliable cleanup owner.

Until this contract is approved, the implemented high-level placement client
owns one daemon transport per Run. Configured profiles can select that daemon,
but they do not make its daemon-local Run distributed.

### Cross-application music availability probe

A small Rust control-plane program is the intended architecture acceptance
scenario:

1. create one Run on a selected Device;
2. request/reuse an Apple Music Runner and a NetEase Music Runner;
3. call typed search services for the same song;
4. aggregate `Apple only`, `NetEase only`, `both`, or `neither`;
5. preserve partial failure, deadlines, cancellation, Device/Runner refs, and
   direct typed results.

The current repository cannot complete this live macOS scenario yet: NetEase
Music has substantial macOS operations, while Apple Music search/playback is
primarily implemented for Windows and macOS remains closer to a probe. This is
an implementation prerequisite, not a reason to weaken the control-plane API.

## Implementation status

The migration baseline landed on 2026-07-31:

- Device, Run, RunnerClass, Runner, claim, lease, and discovery contracts own
  the public control model.
- `auv invoke` remains the typed local/core registry; no generic network Invoke
  RPC exists.
- Root Device/Run selection and inline `AUV_CONTEXT` feed the reusable Rust
  placement client.
- Paired Device profiles are manageable through the
  provisional `auv devices profiles` CLI; Device LIST retains configured
  offline entries without exposing credential material.
- `auv devices pair create-token` uses the owner-checked local daemon RPC;
  `pair connect` consumes it remotely, verifies the authenticated Device view,
  and persists the opaque bearer in a local Device profile without printing it.
- The daemon supervises local and configured custom Runner children over
  private IPC, discovers runtime metadata and business APIs through
  Health/Reflection, and routes typed capability calls.
- The daemon projects annotation-driven typed service discovery over
  gRPC and versioned REST, filtered by caller-based authorization.
- `auv serve` atomically binds repeated local and paired-bearer listeners over one
  daemon handler, publishes only a preferred caller-local discovery endpoint,
  and preserves per-listener authentication plus Caller-scoped Runs.
- The currently admitted Display, Window, Capture, TextRecognition, Input,
  Overlay, and selected macOS Permission, MediaControl, Application, and
  Accessibility slices use dedicated protobuf services and hierarchical
  local/remote clients; custom app schemas remain app-owned.
- Every registered non-scan core invoke operation has an exact typed selected
  Runner adapter; no generic network Invoke RPC exists.
- One real-daemon regression proves two distinct Runner classes can be leased
  by the same Run and cleaned up without native capture or model execution.
- The public SessionService, session manager/resources/scopes, legacy
  VisionService, session CLI, and Session/Vision JavaScript SDK surfaces are
  removed.

The current REST discovery remains a typed protobuf-over-HTTP implementation.
Generated HTTP/OpenAPI bindings and LIST/WATCH version semantics remain future
slices; they must not be inferred from the retired `/v1/*:verb` prototype.

Mechanisms worth preserving include transport caller authentication, one-time
pairing tokens, live bearer revocation, Unix peer-owner checks, lazy resource
initialization, active-operation tracking, draining, idle cleanup, Buf
generation, and generated SDK validation.

## Intentional deferrals

- Per-operation Inspect storage, query indexes, and artifact read APIs remain a
  later Inspect responsibility. This design only assigns Run correlation.
- Video and real-time media/frame streaming require separate backpressure,
  encoding, clock, drop, and transport decisions.
- Unimplemented future package names and lifecycle defaults remain
  provisional. Landed schemas remain marked `EXPERIMENTAL / UNSTABLE`, but
  their generated consumers and descriptor tests require any incompatible
  change to use an explicit versioned migration rather than silently changing
  the wire contract.
- Automated daemon tests intentionally do not send valid native input events;
  they use malformed requests that prove routing, authorization, validation,
  and lease cleanup without moving the pointer, typing, or mutating the
  clipboard.
- Live `PermissionService` result assertions remain manual because the current
  Automation probe can trigger macOS consent UI.
- Automated tests for macOS media mutations use injected owner operations and
  constructed protobuf evidence; they intentionally do not send MediaRemote
  commands. Live mutation behavior remains a manual validation boundary.
- Automated application activation, accessibility focus, and overlay tests
  validate mapping, authorization, routing, and malformed-request rejection
  without causing those native UI side effects. Live behavior remains a manual
  validation boundary.
- Automatic Runner restart, backoff, and crash-loop suppression remain
  deferred. The current daemon marks an exited child `FAILED`, retains that
  record for observation, and permits a later claim to create a replacement.
- Device unpair/enable/disable are currently offline owner operations. Live
  trust mutation while the daemon is running requires a dedicated
  administration contract.
- Automatic launchd/systemd/brew-service installation is separate from the
  foreground `auv serve` contract.
- Dynamic HTTP/OpenAPI route installation must not precede descriptor trust,
  method authorization, validation, and route-conflict policy.
- Merged public gRPC reflection and dynamic external streaming remain separate
  slices; private readiness reflection and typed service discovery do not imply
  either capability.

## References

### Repository evidence

- [Kubectl plugin context research](../invoke-cli/2026-07-31-kubectl-plugin-context-research.md)
- [Current session/API implementation snapshot](2026-07-31-daemon-session-api-architecture.md)
- [`auv-cli` plugin execution](../../../../crates/auv-cli/src/plugin.rs)
- [`auv-driver` local facade](../../../../crates/auv-driver/src/lib.rs)
- [Shared AUV vocabulary](../../../TERMS_AND_CONCEPTS.md)

### Kubernetes plugin and client sources

- [kubectl plugin discovery, argument forwarding, environment, and process execution](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/plugin.go)
- [kubectl command/plugin dispatch](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/kubectl/pkg/cmd/cmd.go)
- [cli-runtime `ConfigFlags`](https://github.com/kubernetes/cli-runtime/blob/c6b14e7f9cb18d23d75accaa9b0cfed0dfe3d355/pkg/genericclioptions/config_flags.go)
- [client-go kubeconfig loading rules](https://github.com/kubernetes/kubernetes/blob/8e608f6b90eaae92055474eeddbd92784c5830df/staging/src/k8s.io/client-go/tools/clientcmd/loader.go)
- [Krew plugin manifest](https://github.com/kubernetes-sigs/krew/blob/299f8e0d1e917eec36fdd665b7435d4830001e60/site/content/docs/developer-guide/plugin-manifest.md)
- [Krew plugin best practices](https://github.com/kubernetes-sigs/krew/blob/299f8e0d1e917eec36fdd665b7435d4830001e60/site/content/docs/developer-guide/develop/best-practices.md)
- [Official sample kubectl plugin](https://github.com/kubernetes/sample-cli-plugin/blob/91817e142ac230c0212d77c22a6b0a03b373719e/pkg/cmd/ns.go)
- [kubectl-tree client construction](https://github.com/ahmetb/kubectl-tree/blob/552f01639c77680fa21f907554fe9aefc23fc6bd/cmd/kubectl-tree/rootcmd.go)
- [kubens typed-client construction](https://github.com/ahmetb/kubectx/blob/12ad6fb22e8c546ee2b54e7de38aa51c906832f7/cmd/kubens/list.go)

### Kubernetes resource and extension APIs

- [Kubernetes API concepts: resources, collections, LIST, WATCH, and resource versions](https://kubernetes.io/docs/reference/using-api/api-concepts/)
- [Kubernetes API aggregation layer](https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/apiserver-aggregation/)
- [CRD and aggregated API comparison](https://kubernetes.io/docs/concepts/extend-kubernetes/api-extension/custom-resources/)
- [Kubernetes APIService definition](https://kubernetes.io/docs/reference/kubernetes-api/apiregistration/api-service-v1/)

### Protobuf, gRPC, and HTTP

- [Protocol Buffers style guide](https://protobuf.dev/programming-guides/style/)
- [Proto3 language guide and compatibility rules](https://protobuf.dev/programming-guides/proto3/)
- [gRPC core concepts](https://grpc.io/docs/what-is-grpc/core-concepts/)
- [gRPC server reflection protocol](https://github.com/grpc/grpc-proto/blob/master/grpc/reflection/v1/reflection.proto)
- [gRPC health checking protocol](https://github.com/grpc/grpc-proto/blob/master/grpc/health/v1/health.proto)
- [Google HTTP annotations](https://github.com/googleapis/googleapis/blob/master/google/api/http.proto)
- [Protovalidate schemas](https://github.com/bufbuild/protovalidate/tree/main/proto/protovalidate/buf/validate)
- [grpc-gateway OpenAPI customization](https://grpc-ecosystem.github.io/grpc-gateway/docs/mapping/customizing_openapi_output/)
- [tonic Unix-domain-socket example](https://github.com/hyperium/tonic/tree/v0.14.5/examples/src/uds)
