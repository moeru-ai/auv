# 2026-06-30 AUV API-P1: Session proto boundary review and minimal surface design

Date: 2026-06-30

Status: **docs-only boundary review** — defines the first real direction for
`crates/auv-api-proto` beyond `DoExample`, but does **not** approve server
implementation, transport runtime, or internal schema replacement.

## One-line summary

AUV now has a real `auv-api-proto` crate and build chain, but the current
`session.proto` is still example-only. API-P1 defines the **minimal external
session API surface** that may replace that placeholder in a later proto-only
slice, while explicitly freezing what proto must **not** absorb from AUV core.

## Current repo state

### What already exists

- Workspace crate: `crates/auv-api-proto`
- Proto file: `proto/auv/api/v1/session.proto`
- Cargo build path compiles the proto with vendored `protoc`
- Generated Rust types are re-exported from `crates/auv-api-proto/src/lib.rs`

### What is still placeholder-only

The current service is:

```proto
service SessionService {
  rpc DoExample(DoExampleRequest) returns (DoExampleResponse);
  rpc DoExampleStream(stream DoExampleStreamRequest)
      returns (stream DoExampleStreamResponse);
}
```

The current messages are simple `input` / `output` placeholders and do not
express AUV session, invoke, run, artifact, or event semantics.

### Evidence pointers

- Proto package today: `auv.api.session.v1`
- Placeholder methods: `DoExample`, `DoExampleStream`
- Build chain: vendored `protoc` via `tonic_prost_build`
- Generated-type smoke test: `DoExampleRequest`

See:

- `proto/auv/api/v1/session.proto`
- `crates/auv-api-proto/build.rs`
- `crates/auv-api-proto/src/lib.rs`

## Boundary decision

### Proto is an external API boundary

API-P1 treats protobuf as:

- a **cross-process / cross-language API contract**
- a **narrow external surface** for sessions, invoke, operation summary, and
  event streaming
- a packaging and interoperability boundary

API-P1 does **not** treat protobuf as:

- AUV's internal truth schema
- the replacement for run-store JSON
- the replacement for `OperationResult`
- the replacement for inspect-server wire records
- a reason to fork existing Rust domain types into a second internal runtime

### Source of truth stays where it already lives

The following remain source-of-truth surfaces after API-P1:

- run store and trace records
- `OperationResult` and related persisted artifacts
- artifact JSON payloads and roles
- existing Rust runtime and invoke model types

Proto may reference, summarize, or point at those surfaces. It should not
silently redefine them.

## Why this lane is the right next move

Recent Minecraft / osu / Balatro work closed multiple donor-side proof lanes.
Recent Core-A / Core-C / Core-D work also repeatedly paused:

- controller / planner growth
- action lease implementation
- generic runtime widening from donor evidence alone

That means the highest-value next convergence is **not another donor proof**
and **not orchestration runtime**. The next clean seam is the **external API
surface**: what a client would call, what it receives back, and how it refers
to runs and artifacts.

## API-P1 scope

API-P1 defines the smallest non-example `SessionService` surface that still
looks like AUV instead of a tutorial stub.

### Required concepts

The first real proto surface should cover only:

- `SessionRef`
- `OperationRef`
- `ArtifactRef`
- `InvokeRequest`
- `InvokeResponse`
- `GetOperationRequest`
- `GetOperationResponse`
- `SessionEvent`

### Service shape

API-P1 recommends replacing the example service with a minimal real surface of
this form:

```proto
service SessionService {
  rpc CreateSession(CreateSessionRequest) returns (CreateSessionResponse);
  rpc Invoke(InvokeRequest) returns (InvokeResponse);
  rpc GetOperation(GetOperationRequest) returns (GetOperationResponse);
  rpc StreamSessionEvents(StreamSessionEventsRequest)
      returns (stream SessionEvent);
}
```

This is intentionally small:

- one session-creation entry
- one invoke entry
- one read-back summary entry
- one event stream

No planner, no controller loop, no donor-specific query vocabulary.

## Minimal message design

The following is the intended **design direction**, not approved final schema.

### SessionRef

```proto
message SessionRef {
  string session_id = 1;
}
```

Maps to the existing AUV concept that every run carries a `session_id`, with
`default` remaining a runtime concern rather than a proto guarantee.

### OperationRef

```proto
message OperationRef {
  string run_id = 1;
  string operation_id = 2;
}
```

Rationale:

- `run_id` is already the stable user-visible handle in AUV
- `operation_id` keeps the proto contract aligned with command/invoke identity
  without forcing one giant result envelope into a single opaque string

### ArtifactRef

```proto
message ArtifactRef {
  string run_id = 1;
  string artifact_id = 2;
  string role = 3;
}
```

Proto should point at artifacts by stable reference. It should not inline the
full artifact JSON surface in v1.

### InvokeRequest

```proto
message InvokeRequest {
  SessionRef session = 1;
  string command_id = 2;
  bytes json_payload = 3;
}
```

API-P1 explicitly prefers a narrow payload seam here:

- `command_id` matches the current AUV invoke model better than inventing
  donor-specific proto methods
- `json_payload` is intentionally coarse for v1

This avoids re-encoding the entire command registry as protobuf on day one.

### InvokeResponse

```proto
message InvokeResponse {
  OperationRef operation = 1;
  string status = 2;
  repeated ArtifactRef artifacts = 3;
  repeated string known_limits = 4;
}
```

This should expose:

- what operation/run was created
- whether the invoke completed or failed
- which artifacts are worth reading next
- which known-limit caveats matter to the caller

### GetOperation

`GetOperationRequest` / `GetOperationResponse` should return a **summary view**
over an operation, not a full replacement for internal persisted records.

The response may include:

- operation ref
- status
- output summary text
- signal key/value pairs
- artifact refs
- optional failure message

This matches today's `InvokeResult` shape more honestly than inventing a new
proto-only outcome model.

### SessionEvent stream

`StreamSessionEvents` should be the one place where the proto lane admits
ongoing delivery.

But v1 should stay minimal:

- session lifecycle events
- invoke started / completed / failed
- artifact recorded

Not:

- donor-specific progress events
- controller state machines
- lease renewals
- high-frequency observation streams

## Versioning rules

### Package name

API-P1 keeps:

```proto
package auv.api.session.v1;
```

Do not rename the package in this slice.

### Stability language

This surface should be documented as:

- **experimental**
- **unstable**
- not yet a public long-term compatibility promise

The point of v1 here is naming and surface convergence, not immediate SDK
stability guarantees.

### Field discipline

API-P1 recommends:

- conservative first-field allocation
- no speculative donor-specific oneof explosion
- no giant “future everything” wrapper messages

Reserve room by staying small, not by pre-modeling every future lane.

## Mapping to existing AUV concepts

| Proto concept | Existing AUV concept | Boundary rule |
| --- | --- | --- |
| `SessionRef.session_id` | session id in run/runtime vocabulary | reference only; runtime still owns defaults |
| `OperationRef.run_id` | stable run handle | direct external handle is fine |
| `OperationRef.operation_id` | invoke/command operation identity | summary-level API field, not store rewrite |
| `ArtifactRef` | artifact id + role + run association | reference only; artifact JSON stays separate |
| `InvokeRequest.command_id` | current `InvokeRequest.command_id` | reuse existing invoke naming |
| `InvokeResponse.status` | current `RunStatus` / invoke outcome | summarize; do not replace internal result model |

## Explicit non-goals

API-P1 intentionally does **not** approve:

- gRPC server implementation
- transport runtime, daemon, or connection lifecycle
- controller / planner APIs
- action lease APIs
- Minecraft / osu / Balatro-specific proto messages
- replacing `OperationResult` as the internal truth schema
- rewriting artifact JSON or inspect-server payloads into proto
- generated SDK support guarantees
- streaming raw observations or model outputs

If a later slice wants any of the above, it needs a new owner-named scope.

## Recommended follow-on split

API-P1 should be followed, if approved, by **one** narrow proto-only slice:

- **API-P2:** replace `DoExample` / `DoExampleStream` with the minimal real
  session surface defined here (landed, commit `758fa21`)
- **API-P3:** record the proto-to-internal mapper boundary and server handoff;
  see
  [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-session-api-closeout.md)

API-P1 does **not** itself approve:

- server runtime work
- MCP transport work
- JS SDK work
- inspect-server migration

## Validation for this slice

Docs-only validation:

- `git diff --check`

Future proto-only validation target (not run in API-P1):

- `cargo fmt --check`
- `cargo check -p auv-api-proto`
- `git diff --check`

If `buf` linting is later standardized in-repo, it may be added in API-P2 or a
later proto hygiene slice, not assumed here.

## Final decision

API-P1 verdict:

- **do not** continue MC-20 / controller / lease implementation from current
  donor evidence
- **do** treat `auv-api-proto` as the next external-surface convergence seam
- **do** keep proto narrow, external, and summary-oriented
- **defer** all runtime/server/controller growth until a later named slice

## Related

- Terms: `docs/TERMS_AND_CONCEPTS.md`
- Proto crate: `crates/auv-api-proto`
- Proto file: `proto/auv/api/v1/session.proto`
- Re-export shim: `src/model.rs`
- Invoke host types: `crates/auv-cli-invoke/src/model.rs`
- MC-20 final pause:
  [`2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`](../apps/minecraft/2026-06-30-minecraft-probe-20-reference.md)
- Core-D1 boundary:
  [`2026-06-30-auv-core-d1-action-lease-ownership-boundary-review.md`](../runtime/2026-06-30-core-action-lease-ownership-boundary-review.md)
