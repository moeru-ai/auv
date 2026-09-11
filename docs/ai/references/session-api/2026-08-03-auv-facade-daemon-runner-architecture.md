# AUV facade, daemon, and Runner architecture

Date: 2026-08-03

Status: accepted target architecture; the facade, opaque routed gRPC, internal
Run affinity, public claim/lease removal, route-independent capability schemas,
and standard Health/Reflection-only Runner host have landed. Capability calls
now use one route-bound transport and do not add typed forwarding methods to the
daemon. Daemon crate extraction, a true local facade backend, `daemon.v1`, and
first-party in-process composition remain follow-up implementation slices.
Paired HTTP endpoints may use loopback, LAN, VPN, or another
operator-selected network; transport encryption is a deployment concern rather
than a client-side or server-side endpoint admission rule. A daemon configured
with only a paired public listener creates a private local listener for
executable Runner parent delegation; it never injects the bearer endpoint as
child context.

## Background

The implementation before this migration combined several responsibilities:

- `auv-api-client` contains gRPC clients together with `AuvContext`, profile
  resolution, daemon discovery, placement, and local Driver dependencies.
- `auv-api-server::control_plane` has typed forwarding methods for individual
  Driver, inference, and platform RPCs.
- `auv-api-server` also owns daemon stores, Runner providers, supervision, and
  long-lived control state.
- capability requests carry `RunnerLeaseRef`, so extension-owned protobuf
  messages must know daemon lifecycle details.
- `RunnerRuntimeService` duplicates standard gRPC Health/Reflection and state
  the daemon can observe at its own process and proxy boundaries.

Those choices made each new capability a coordinated change across the daemon,
server, client, and protobuf request shapes. They also prevent the Rust API from
using one domain-facing interface for local and remote execution.

## Accepted model

### Rust domain contracts and the AUV facade

`crates/auv` will be a thin SDK crate exposing `auv::Client`. Application and
extension business code uses the same typed operations for local and remote
execution.

```text
auv::Client
├── local backend
│   └── Driver / inference / media domain providers
└── remote backend
    └── auv-api-client
```

Rust request, result, and semantic error types are owned by their domain crates.
Protobuf is their cross-process and cross-device wire projection. Local calls
pass domain values directly; remote adapters convert between domain and
protobuf values. `tonic::Status` remains inside the protocol adapter.

`auv::Client` also owns `AuvContext`, profile and environment resolution,
client-side daemon discovery, backend selection, and routed extension
transport. Once a context selects a daemon or remote Device, failures are
reported and never trigger implicit local fallback.

`auv-api-client` becomes a pure wire client. It accepts an explicit endpoint and
routing parameters, establishes transport, exposes daemon control clients, and
provides an opaque routed gRPC transport. It does not read environment variables
or user files, choose Devices, or depend on local capability implementations.

Community extensions keep their generated Rust clients and any
extension-specific local/remote facade. AUV supplies a transport bound to the
selected daemon, Device, optional Run, and RunnerClass; it does not aggregate
unknown community service types into `auv::Client`.

### Daemon and API server ownership

`crates/auv-daemon` owns the long-lived process role:

- Device and Run state;
- local RunnerClass registration;
- Runner creation, supervision, affinity, drain, and termination;
- capability route resolution;
- first-party capability composition;
- persistent daemon state.

`auv-api-server` owns protocol serving:

- listeners and gRPC transport;
- authentication/routing middleware;
- typed daemon control adapters;
- wire/domain error mapping;
- scoped standard Health/Reflection forwarding;
- streaming opaque gRPC proxying.

`auv-cli` parses `auv serve`, supplies frontend configuration, handles frontend
lifecycle, and calls `auv-daemon`. It is not the reusable daemon implementation.

### Control plane and capability data plane

Daemon control services are typed and implemented by `auv-daemon`:

```text
DeviceService
PairingService
DiscoveryService
RunService
RunnerClassService
RunnerService
```

Their protobuf package is `auv.api.daemon.v1`. It replaced the experimental
`auv.api.core.v1` package before stabilization. No `meta.v1` package is introduced without
a shared wire concept that lacks a domain owner.

All capability services use one opaque route, including first-party Driver,
inference, and media services:

```text
public gRPC request
  -> transport authentication and routing metadata
  -> auv-daemon resolves RunnerClass registration and live Runner
  -> auv-api-server forwards the original gRPC stream
  -> Runner service implementation
```

Adding a capability RPC must not add a typed forwarding method to
`auv-daemon` or `auv-api-server`.

The daemon reads the complete gRPC method path and AUV routing metadata. It does
not decode extension request/response messages, parse descriptors, merge
reflection schemas, compare schema digests, translate versions, or wrap calls
in an AUV-owned JSON/protobuf envelope.

Capability routing metadata carries the selected Device, optional Run, and
RunnerClass registration. Business protobuf messages do not contain
`RunnerLeaseRef` or another daemon routing resource.

### Extensions, CLI plugins, and Runner registration

An AUV extension is a distributable project that may provide either or both of:

- a CLI plugin discovered as `auv-<name>` on the invoking process's `PATH`;
- one or more registered RunnerClasses serving gRPC.

PATH discovery grants only the CLI role. It does not register a RunnerClass or
approve remote execution. RunnerClass registration is local operator
configuration and cannot be changed through the remote API. Remote callers may
list registrations and manage live instances of an already admitted class.

Registering a RunnerClass approves its entire gRPC endpoint. The daemon does not
maintain a per-service or per-method allowlist. Its own control namespace cannot
be shadowed by a routed extension.

A RunnerClass registration has a stable Device-local key, for example
`netease-music.personal`. The same executable may have several registrations
with different arguments or profiles. Business calls select this stable key;
live Runner IDs are limited to listing, health, prewarming, drain, stop, and
debugging.

The RunnerClass is the deployment, configuration, isolation, and shared-resource
lifecycle unit for a cohesive service bundle. It is not one class per protobuf
service. A NetEase Music class may expose playlist, song, playback, and search
services while sharing one application connection and login profile.

### Runner runtime forms

The target runtime forms are:

- `InProcess`: trusted first-party service composition inside `auv-daemon`;
- `Executable`: an approved child binary over daemon-created local IPC;
- `RemoteGrpc`: an existing gRPC endpoint whose process the daemon does not own.

First-party Driver, inference, and media bundles default to `InProcess` while
using the same logical Runner routing model. Their domain behavior remains in
the owning provider crates. The Ultralytics gRPC adapter is currently hosted by
the `auv-cli` child-Runner runtime and should move into `auv-daemon` composition
when that owner lands; there is no dedicated inference Runner crate.

An executable Runner adopts a connected IPC handle created before spawn by the
daemon. It does not bind or advertise its own socket path. Standard output and
error remain log streams. `AUV_CONTEXT` supplies the parent daemon endpoint for
a distinct outbound client connection when the hosted extension needs Driver,
OCR, inference, or another ordinary AUV API.

There is no shared Runner helper crate. Inherited platform-local transport is
owned by `auv-api-server::transport`; executable hosts directly assemble their
standard gRPC Health and Reflection services, injected `AuvContext`, and process
shutdown integration.

There is no AUV-specific `RunnerRuntimeService`. Registration supplies identity,
standard Health supplies readiness, the daemon observes owned process state and
proxied active requests, and the daemon's route gate implements drain.

### Discovery and reflection

There is no separate capability catalog. Remote discovery composes:

```text
ListRunnerClasses
  -> select one registration
  -> scoped grpc.health.v1.Health
  -> scoped grpc.reflection.v1.ServerReflection
```

Health and Reflection calls carry RunnerClass routing metadata and are forwarded
to that Runner without descriptor parsing. Each registration therefore reports
its own actual schema, including ordinary protobuf version skew.

Future JavaScript and other dynamic clients are use cases for this contract,
not an approved SDK implementation in this slice. Generated extension SDKs
remain optional.

### Run affinity and Runner lifecycle

Frontend or application operation roots create and terminate Runs.
`auv::Client` propagates an optional Run association but does not create a Run
per client or per RPC.

When routing metadata includes a Run, the daemon keeps non-exclusive affinity
for:

```text
(Run, Device, RunnerClass registration) -> live Runner
```

The attachment lasts until the Run ends or the Runner stops/fails. Several Runs
may share one Runner. Calls without a Run have no affinity beyond their active
request.

Runner claims, public leases, lease deadlines, caller-owned reuse policy, and
required-capability scheduling are removed. Normal calls resolve or start a
Runner from the selected registration. Explicit `CreateRunner` remains only for
prewarming and operations.

### Request context and authentication

Public transport middleware authenticates once, parses routing metadata, and
injects a request context containing the Caller. Handlers do not establish the
Caller again. A typed daemon domain operation may still make a
resource-specific authorization decision using that established identity.

Daemon-created private IPC does not repeat external authentication inside the
Runner.

## Dependency direction

```text
domain contracts and providers
  auv-driver*
  inference crates
  media crates
          ^
          |
auv -----------------> auv-api-client ------> auv-api-proto
 ^                           ^
 |                           |
extension CLI/SDK            |
                             |
auv-daemon ------> auv-api-server ----------+
    |
    +-----------> auv-runner
    +-----------> first-party domain providers

auv-cli ----------> auv
auv-cli ----------> auv-daemon
```

Domain crates do not depend on protobuf, API clients, the daemon, or the CLI.
`auv-api-server` does not depend on concrete Driver/inference/media behavior.
`auv-daemon` is the composition root for the serving process but is not a
general application runtime crate.

## Removed target concepts

- public `RunnerLeaseRef` and release RPCs;
- Runner claim and required-capability scheduling;
- per-capability typed forwarding in the daemon control plane;
- daemon-side descriptor parsing, merging, and compatibility validation;
- daemon-owned capability manifest and catalog;
- per-service/method Runner allowlists;
- custom `RunnerRuntimeService`;
- dedicated first-party executable runner crates by default;
- environment, profile, discovery, and placement policy in `auv-api-client`;
- protobuf-generated types and `tonic::Status` in the `auv::Client` domain API.

## Deferred work

- JavaScript SDK and dynamic invocation ergonomics;
- HTTP transcoding and WebSocket/video streaming;
- the concrete gRPC error-details schema;
- Windows inherited-handle implementation;
- optional richer Runner observability after a concrete consumer exists.

These are intentional deferrals, not requirements for the first migration
slice.

## Migration progress

Each step should leave the workspace compiling and should not combine unrelated
behavior changes.

1. **Landed:** Introduce the `auv` facade with domain-facing types and move context,
   profile, discovery, and backend-selection policy out of `auv-api-client`.
2. **Landed:** Reduce `auv-api-client` to explicit transport, daemon control clients, and a
   route-bound opaque gRPC transport.
3. **Pending:** Introduce `auv-daemon`; move stores, Runner registration/providers,
   supervision, and first-party composition out of `auv-api-server` and
   `auv-cli`.
4. **Landed:** Add transport routing metadata and remove `RunnerLeaseRef` from capability
   requests and clients.
5. **Landed:** Replace typed capability forwarding with a streaming opaque proxy for both
   first-party and extension services.
6. **Landed:** Rename the experimental `auv.api.core.v1` package and REST group
   to `auv.api.daemon.v1` / `daemon/v1` before stabilization.
7. **Landed:** Replace public claim/lease lifecycle with daemon-internal Run attachments and
   RunnerClass resolution.
8. **Landed:** Remove `RunnerRuntimeService` and place inherited local transport
   under `auv-api-server::transport`; there is no Runner protocol/helper crate.
9. **Partly landed:** Remove the dedicated inference Runner crate and host its
   adapter in the `auv-cli` child-Runner runtime. Composing first-party services
   as InProcess RunnerClasses remains pending on the `auv-daemon` owner.
10. **Landed:** Focused tests cover a routed local Driver operation, separate
    Driver/inference RunnerClasses in one Run, and a supervised NetEase
    extension service reached through the same opaque proxy.

This sequence is a handoff, not approval to implement every step in one change.
Each implementation slice still requires owner scope and focused tests.
