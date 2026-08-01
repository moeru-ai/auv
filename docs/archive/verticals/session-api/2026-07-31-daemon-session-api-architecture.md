# Daemon and Session API Architecture

Date: 2026-07-31

Status: archived implementation snapshot. Its SessionService, generic Invoke, and
handwritten REST resource shape are superseded as target architecture by
[`2026-07-31-device-run-runner-aggregated-api-design.md`](2026-07-31-device-run-runner-aggregated-api-design.md).
Transport, authority, resource-lifecycle, and SDK evidence in this note remain
useful during migration.

Implemented snapshot: local and paired-mTLS transport, native gRPC plus
protobuf-over-HTTP REST, lifecycle control plane, local discovery,
Rust/TypeScript clients, typed window capture/text recognition, unary object
detection with lazy session-owned resources, and offline paired-certificate
provisioning. Live administration and further typed operation expansion remain
incomplete in this prototype.

## Current implemented shape

The session API no longer lives inside the product CLI. The dependency
direction is:

```text
auv-api-proto
    ↑                 ↑
auv-api-server    auv-api-client
    ↑                 ↑
    └──────── auv-cli process frontend
```

- `auv-api-proto` owns the experimental protobuf schema, generated prost/tonic
  messages and service traits, and descriptor set.
- `auv-api-server` owns the session lifecycle aggregate, protobuf-to-command
  mapper, invoke adapter, gRPC status mapping, and listener adapters.
- `auv-api-client` hides tonic channel construction and exposes `Client` before
  session selection and a session-scoped `Connection` afterwards.
- `auv-cli` parses configuration, binds the server, publishes readiness and a
  locked discovery descriptor, provides session management commands, and turns
  process interruption or an optional no-session idle timeout into graceful
  tonic shutdown.
- `js/packages/api-client` contains Buf-generated Protobuf-ES messages/service
  descriptors, a type-checked Node gRPC client constructor, and a typed REST
  client that encodes the same generated messages.

The same generated `SessionService` runs over loopback TCP, Unix domain sockets,
and remote mutual TLS. These are transports, not separate business APIs or
schemas. The foreground entrypoints are:

```text
auv api-server serve
auv api-server serve --unix-socket <path>
auv api-server serve --remote-listen <ip> --tls-certificate <path> \
  --tls-private-key <path> --client-ca-certificate <path> \
  --pairing-store <path> --no-discovery
```

The plaintext server refuses non-loopback TCP. Remote mode requires an explicit
IP, mutual-TLS material, and a pairing store. The Unix listener refuses to
overwrite an existing socket path, sets that socket to owner-only access, and
rejects peers whose OS credential UID differs from the socket owner. On
graceful shutdown it removes only the same socket inode it created, avoiding
deletion of a replacement created by another process.

Every listener also exposes these protobuf-over-HTTP resource routes:

```text
POST   /v1/sessions
POST   /v1/sessions:acquire
GET    /v1/sessions
GET    /v1/sessions/{session_id}
DELETE /v1/sessions/{session_id}
POST   /v1/session-leases:renew
POST   /v1/session-leases:release
POST   /v1/operations:invoke
POST   /v1/object-detections:detect
POST   /v1/windows:capture
POST   /v1/text:recognize
```

Requests with bodies and all successful responses use
`application/protobuf`; the bodies are the corresponding generated request and
response messages. Errors use RFC 9457-style `application/problem+json` and
stable HTTP categories. This is deliberately not grpc-gateway/OpenAPI JSON
transcoding: protobuf remains the sole operation schema, including bytes,
timestamps, enums, and oneofs. Tonic still owns accepting and TLS/UDS connection
metadata, so REST and gRPC derive principals from the same transport evidence
and call the same `SessionApiHandler` instance.

The CLI publishes a versioned descriptor in the current user's platform state
directory, falling back to platform-local data (or `AUV_DISCOVERY_FILE` /
`--discovery-file`). An exclusive sibling
lock rejects a competing publisher. The descriptor is written atomically with
owner-only Unix permissions and removed only if its instance identity still
matches. Endpoint precedence is `--endpoint`, `AUV_ENDPOINT`, then discovery.
A missing descriptor makes `session list` an empty successful query; a stale,
malformed, or explicitly selected endpoint remains an error.

## Current session truth

The current `Session` is a daemon-owned lifecycle aggregate with a UUIDv7-based
opaque ID. `CreateSession` always creates; `AcquireSession` alone may select a
compatible ready reusable session. Leases have bounded TTLs and renew/release
operations. Operations acquire RAII capacity permits. Close revokes leases and
either removes an idle session or moves an active one to draining. A background
reaper expires leases and sessions.

The aggregate now retains lazy Ultralytics object detectors and one lazily
opened local driver session in its typed resource container. A normalized
detector specification is the per-session model cache key, concurrent
initialization is coordinated through `OnceLock`, and a failed initialization
is cached rather than repeated on every RPC. Operation permits on a reused
session see the same container, and draining/expiry releases it only after the
last operation. App-specific handles and device-level action locks remain
outside this implemented slice.

`ObjectDetectionService.DetectObjects` is a typed unary RGB8 contract. It
requires a live `SessionLeaseRef`, validates exact frame byte length before
model loading, moves request bytes into the image buffer, and executes blocking
model work away from the async reactor. Remote paired principals cannot submit
daemon-host model paths; that filesystem capability remains local-owner-only
until a server-owned model registry exists.

`VisionService.CaptureWindow` resolves a typed application/window selector and
returns RGB8 pixels with logical screen bounds, scale, backend, and explicit
fallback reason. `VisionService.RecognizeText` consumes that typed capture and
returns recognized regions in the same logical coordinate space. Both use the
session-owned driver and the same lease/capacity rules as inference. The Rust
`Connection`, generated TypeScript descriptors, and typed REST client expose
these services.

Local descriptor parsing and endpoint precedence live in `auv-api-client`, so
non-CLI frontends do not duplicate daemon discovery. Balatro's state command
uses the API when an explicit, environment, or discovered endpoint is
available. Image observation sends both detector calls through one reused
session. Live observation additionally captures the window and performs OCR on
that same `Connection`, maps logical OCR regions into detector pixel space,
and retains the local path only when no daemon exists. Hover reads, action
delivery, and action verification remain local pending typed contracts; this
slice makes no video claim.

The current generic invoke payload remains:

```text
command_id + bytes(JSON request) -> bytes(JSON direct result)
```

It is a useful migration escape hatch, not the final typed cross-language API.
Further stable driver and app operations still need dedicated typed
request/response messages; the generic JSON path does not provide that safety.

## Lifecycle invariants

Keep these concepts separate:

- `Session`: daemon-owned reusable runtime and its resources.
- `Connection`: client-side handle attached to a `SessionRef`.
- `SessionLease`: server-issued, expiring attachment that keeps a session alive;
  it is not an authentication credential.
- daemon lifetime: process lifetime, independent from each session's idle
  deadline.

A reusable session must not be destroyed until it has no active lease, no
active operation, and its idle deadline has elapsed. Resource pressure should
evict an unleased idle session first. In-flight work should enter a draining
phase rather than being killed. Capacity and provider concurrency must be
explicit so a client can acquire a second session instead of silently
overloading one model instance.

The wire contract covers acquire/create, get/list, close, lease renew/release,
phase, active operation count, idle deadline, and protobuf
`Timestamp`/`Duration`, plus typed unary object detection, window capture, and
text recognition. Streaming events still wait for a concrete cursor/gap
recovery consumer rather than introducing a generic base event.

## Local and remote authority

Local Unix access skips pairing, but it is not anonymous: filesystem mode and
tonic Unix peer credentials constrain calls to the server owner. Both
owner-checked Unix requests and loopback-only TCP requests now project to a
transport-independent local-owner principal before entering handlers. Session
creation/reuse, get/list, leases, operation admission, and close are all scoped
to that principal; foreign identities receive unknown-session/lease results
rather than an ownership oracle.

Remote control uses a separate explicit `RemoteTls` listen mode. Tonic requires
a client certificate signed by the configured client CA. The transport hashes
the validated leaf certificate's DER with SHA-256, resolves it through the
durable pairing store, checks a typed scope, and only then projects the stable
pair ID into handlers. Pair credentials do not belong in ordinary session
request bodies, and neither a mutable certificate display name nor a
`SessionLease` is identity.

The pairing store is a versioned owner-private JSON record protected by a
process-lifetime file lock. Updates use a same-directory temporary file,
file/directory synchronization, and atomic rename; authorization reads use the
current immutable in-memory snapshot. A stable pair can hold multiple leaf
fingerprints for certificate rotation. Pair enable/disable, scope changes, and
individual credential revocation affect the next RPC. Session inspect,
management, and operation execution are separate scopes. Missing pairing maps
to `UNAUTHENTICATED`; a missing scope maps to `PERMISSION_DENIED` without
exposing store details.

The owner provisions the store while the daemon is stopped:

```text
auv pairing list [--json]
auv pairing add --label <label> --certificate <leaf.pem> --scope <scope>...
auv pairing rotate <pair-id> --certificate <replacement.pem>
auv pairing set-scopes <pair-id> --scope <scope>...
auv pairing enable|disable <pair-id>
auv pairing revoke --certificate <leaf.pem>
```

The tool hashes the certificate DER rather than trusting a supplied
fingerprint. `add` creates a stable UUIDv7 pair ID when omitted, and `rotate`
adds a credential without changing that identity. It takes the same exclusive
store lock as the daemon, so an attempted offline mutation while serving fails
closed. Certificate issuance and CA policy remain external to AUV.

The Rust client accepts an explicit server name, server CA, client certificate,
and client key through `Client::connect_paired`. The ordinary endpoint parser
continues to reject remote plaintext. The local discovery descriptor remains
credential-free, so remote server CLI mode requires `--no-discovery` until a
credential-profile contract exists.

## Protobuf and SDK generation ownership

Rust generation remains Cargo-native through `tonic-prost-build` and vendored
`protoc`; this keeps `cargo build` self-contained and versions aligned through
`Cargo.lock`. `proto/buf.gen.yaml` is not a second Rust owner: it pins the
Buf-hosted Protobuf-ES plugin at `v2.13.0`, revision 1, and generates the
checked-in TypeScript SDK. Its runtime packages are pinned to compatible
versions and `pnpm --filter @auv/api-client typecheck` validates the result.

The Nix shell now includes Buf without pulling Linux-only Wayland dependencies
on macOS. Schema format/lint/generate therefore use the repository shell, while
Cargo continues to own Rust generation.

## External implementation references

- tonic 0.14 Unix transport examples and `UdsConnectInfo` establish that UDS
  is an incoming/channel adapter over the same service:
  <https://github.com/hyperium/tonic/tree/v0.14.5/examples/src/uds>
- tonic's current TLS client-auth example configures `client_ca_root` and reads
  the validated peer certificate from the request:
  <https://github.com/grpc/grpc-rust/blob/master/examples/src/tls_client_auth/server.rs>
- containerd Rust extensions show a generated protocol crate plus a high-level
  clone-cheap client facade:
  <https://github.com/containerd/rust-extensions/tree/main/crates/client>
- Buck2 documents daemon startup locking, discovery metadata, compatibility,
  authentication metadata, and idle shutdown:
  <https://github.com/facebook/buck2/blob/main/app/buck2_daemon/daemon_lifecycle.md>
- sccache separates TCP/UDS accepting, idle shutdown, and active-request drain:
  <https://github.com/mozilla/sccache/blob/main/src/server.rs>
- containerd leases separate a managed resource from the references that keep
  it alive:
  <https://github.com/containerd/containerd/blob/main/api/services/leases/v1/leases.proto>
- runwasi/containerd-shimkit provides explicit lifecycle transitions and
  grouping-key examples:
  <https://github.com/containerd/runwasi/tree/main/crates/containerd-shimkit>

## Intentional deferrals

- `TODO(inference-resource-pressure)`: add daemon-wide model-memory budgets and
  idle-session eviction policy once provider memory measurements can define a
  safe limit; current capacity bounds operations, not total model bytes.
- `TODO(daemon-supervision)`: discovery and opt-in no-session idle shutdown
  exist, but automatic startup, service-manager installation, and crash restart
  need a process supervisor contract.
- `TODO(paired-discovery-profile)`: remote mTLS works with explicit credentials,
  but discovery must not publish a remote endpoint until it can safely name a
  client credential profile and trust roots.
- `TODO(typed-operation-expansion)`: object detection, window capture, and text
  recognition are typed, but generic JSON invoke remains for other commands
  until their owning driver/app contracts are selected; the generated SDK
  cannot make those JSON payloads operation-safe.
- `TODO(balatro-daemon-actions)`: live observation now reuses one daemon
  connection for capture, OCR, and detection. Hover reads, action delivery,
  and action verification remain local until owner-approved typed contracts
  exist; this does not claim video or streaming reuse.
- `TODO(live-pairing-admin)`: offline owner provisioning is implemented; live
  mutations require an owner-authorized local admin RPC and audit evidence
  before the CLI may update a serving daemon.
- `TODO(js-unix-transport)`: the Node SDK currently dials loopback gRPC/TCP;
  Unix dialing needs a tested Node HTTP/2 connector and browsers need an
  explicit gRPC-Web/Connect gateway.
- `TODO(websocket-events)`: do not add a generic WebSocket event envelope until
  a concrete non-video consumer defines ordering, cursor, gap recovery, and
  cancellation semantics. Video/frame streaming remains a separate future
  project as requested.
