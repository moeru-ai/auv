# 2026-06-18 Core Realtime Session Substrate Slice Design

Date: 2026-06-18

## Scope classification

`docs-only`

This note cuts a minimal implementable realtime / warm-model slice out of
`2026-06-10-stateful-session-daemon-js-repl-v0.md`. It is the design note that
must land before more core code is written in the realtime-session substrate
worktree. It is not approval to implement the full daemon, JS package ecosystem,
browser REPL, MC-6 / MC-7, or a new action-result schema.

## Decision

Build the next core-lane slice as a **minimal realtime substrate**:

- one local session runtime can keep expensive observation providers warm across
  repeated `observe` calls
- session resources retain observations and node handles across calls
- `observe` / `act` / `verify` reuse `ObservationSnapshot`, `SurfaceNode`,
  `InputActionResult`, and `VerificationResult`
- the first process / transport integration should reuse the already-proven
  inspect-server local HTTP/WebSocket host, loopback/token security, user-private
  descriptor, and broadcast patterns instead of creating a separate daemon
  process

The process model decision is deliberately conservative: **reuse the
`inspect_server` host/security/broadcasting substrate, but keep execution-session
semantics distinct from run inspection semantics**. Do not overload `/runs/*`,
`InspectServerSession`, or run viewer payloads as the automation session API.

Warm model ownership starts **session-owned**. A model/provider instance belongs
to the session that opened it and is released with that session. A daemon-global
provider pool is deferred until two or more sessions contend for the same
provider and the sharing policy, eviction, and action-lock interactions are
measured.

## Existing anchors

- `docs/TERMS_AND_CONCEPTS.md` defines `Device` and `Session`. `Session` already
  groups target defaults, observation cache, run recording state, and
  permission/capability profile.
- `auv-tracing-driver::trace` defines `DeviceId` and `SessionId`, plus default
  ids for local/default execution.
- `RunSpec` records `device_id` and `session_id` on runs; existing runtime tests
  prove custom ids are persisted.
- `src/session.rs` now has an in-process `SessionRuntime` with provider reuse,
  observation/node resources, invalidation after action, and verification
  resource recording.
- `src/osu.rs` projects osu fixture detections into session observations.
- `src/minecraft_session.rs` projects Minecraft spatial telemetry frames into
  the same `ObservationSnapshot` contract without adding Minecraft nouns to
  `src/session.rs`.
- `src/inspect_server` proves the local HTTP/WebSocket serving model, loopback
  and token write security, owner-only session descriptor file, write conflict
  checks, and broadcaster-to-WebSocket delivery for run updates.

## Architecture fork

### Chosen: inspect-server-hosted session surface

Use the inspect server as the first host for realtime session endpoints:

```text
inspect/local server process
  /runs/*                 existing run inspection and live run update stream
  /session/*              new automation session resource API, same host/security
  session broadcaster     emits session resource events, not RunUpdate
```

This keeps local serving, descriptor discovery, token handling, and WebSocket
plumbing in one already-tested process. It also lets the browser viewer link run
artifacts and session resources later without a second local server to discover.

### Rejected for this slice: separate daemon process

A separate `auv-session-daemon` process is deferred. It adds process lifecycle,
port discovery, cross-process ownership, and conflict behavior before the core
resource semantics are fully proven. The separate process can be reopened after
the inspect-hosted substrate proves warm providers, invalidation, and session
resource handles.

### Boundary guard

Reusing the inspect server does not mean the inspect server becomes the runtime
semantics owner. The split remains:

```text
session substrate
  owns live resources: sessions, providers, observations, nodes, action events
  calls runtime/driver/domain APIs on request
  records runs through RunRecordingBackend when an operation needs evidence

inspect run viewer
  reads stored run data and artifacts
  streams run updates
  may render links to session resources later
```

## Minimal slice

The first code slice after this note should prove these surfaces only:

1. `SessionRuntime` can register one warm provider that reports its load count
   and observe count.
2. N repeated `observe` calls reuse the same provider instance without reload.
3. The session keeps observed nodes addressable by handle/label lookup across
   calls.
4. `act` invalidates existing observation resources and emits a stale reason
   while preserving the existing `InputActionResult` shape.
5. `verify` stores an existing `VerificationResult` shape.
6. At least two vertical consumers project into the same substrate. The current
   candidates are osu fixture detections and Minecraft spatial telemetry frames.

No transport endpoint is required before the in-process proof is green. If a
transport endpoint is added in the same PR, it must wrap the same
`SessionRuntime` API and use the inspect-server host/security decision above;
it must not introduce a parallel resource table.

## Red-line contracts

### Core, not Minecraft

The realtime substrate is a core runtime capability. Minecraft, osu, NetEase,
Balatro, and future domains are consumers. Do not put Minecraft-specific types,
refusal reasons, targets, or sidecar assumptions into `src/session.rs` or the
generic transport DTOs. Domain adapters may live beside their domain crates or
in root adapter modules that convert into `ObservationSnapshot`.

### Substrate, not agent

The session stores state, not goals. It must not run its own
perceive-decide-act loop. It exposes request-driven operations:

```text
observe(request) -> ObservationResource
act(request, InputActionResult) -> InputActionResult + invalidation events
verify(VerificationResult) -> VerificationResource
```

Callers decide when to observe, when to act, and what a successful goal means.
The substrate can stream state changes and invalidation events, but it cannot
choose targets, schedule autonomous actions, or retry on its own.

## Cheap spatial floor

Before opening MC-6 / 2.5D / 3DGS representation work, the session should prove
the cheapest useful spatial memory:

```text
posed detection = observation node + capture/frame id + app/window scope
                 + optional camera pose / projection detail in provider detail
```

The session should be able to answer:

- "Have we seen this label/block/object before?"
- "Which observation/frame/node last saw it?"
- "Is that observation still fresh or stale after an action?"

This is below 2.5D: no depth reconstruction, no dense photometric comparison,
and no splat. If this floor is enough for a task, do not climb the representation
ladder. If it fails, the MC-6 texture-sweep measurement decides whether 2.5D is
needed; 3DGS remains behind the three-gate decision in the Minecraft closure
plan.

## Acceptance gate

The implementation slice is acceptable only when current-state evidence proves:

- a warm provider/model is initialized once and reused across N `observe` calls
  without reload
- the session persists observations and can answer one lookup from retained
  state
- action invalidation uses existing `InputActionResult` and stale-resource
  events, with no third action-result schema
- verification uses existing `VerificationResult`
- at least one non-fixture vertical consumes the same substrate as a second
  consumer; Minecraft spatial telemetry already qualifies if its adapter stays
  outside generic session code
- the design stays core-resident: no MC nouns in generic session/transport code
- no autonomous loop exists in code or protocol

Suggested focused verification:

```text
cargo fmt --check
cargo check
cargo test session::
cargo test osu_detection_provider_projects_into_session_observation
cargo test minecraft_spatial_frame_session_provider
git diff --check
```

If transport endpoints land in the same slice, also add focused
`inspect_server` tests that prove loopback/token behavior is reused and that
session resource events are not serialized as `RunUpdate`.

## Deferrals

- TODO(session-transport-v0): HTTP/WS endpoints under the inspect-server host
  are deferred until the in-process warm-provider proof is complete.
- TODO(session-js-client-v0): JS resource classes and REPL previews are deferred
  until transport DTOs exist.
- TODO(session-daemon-process): a standalone daemon process is deferred until
  inspect-hosted semantics prove insufficient.
- TODO(session-global-provider-pool): daemon-global warm model pooling is
  deferred until multiple sessions need shared provider ownership.
- TODO(session-action-lock): device-level mutating action locks are deferred
  until concurrent sessions contend for one device.
- TODO(session-recording-link): session-originated operations recording through
  `RunRecordingBackend` is deferred until the first transport or domain command
  needs durable evidence from a session operation.

## Exploration ledger

- Read `2026-06-16-minecraft-live-mc2-mc4-closure-plan.md`: MC-2/3/4 are closed;
  the next slice is runtime lifecycle, not MC-6/MC-7 representation work.
- Read `2026-06-10-stateful-session-daemon-js-repl-v0.md`: the full direction is
  daemon + typed JS/REPL, but its suggested first slice is in-process resource
  semantics, provider reuse, stale events, and no full transport.
- Read `docs/TERMS_AND_CONCEPTS.md`: `Device`, `Session`, run scoping, and
  inspect-server terms already exist and should be reused.
- Read `auv-tracing-driver::{trace,run_builder}`: `DeviceId` / `SessionId` are
  already typed and persisted on run records.
- Read `src/session.rs`, `src/osu.rs`, and `src/minecraft_session.rs`: the first
  in-process substrate and two observation consumers already exist.
- Read `src/inspect_server`: local HTTP/WebSocket, loopback/token write security,
  session descriptor, conflict rejection, and broadcasting already have tests.
- Collabi pre-edit coordination was accounted for, but no callable Collabi writer
  API is available in this harness; the human writer entry remains
  `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`.
