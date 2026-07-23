# AUV Tracing Contract V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. In this completed plan, checkbox (`- [ ]`) steps preserve the approved procedure and are not a historical progress log.

**Status:** Implementation complete. Completion is recorded by this status and the Inspect index; all unchecked steps below preserve the original execution procedure consistently.

**Goal:** Replace the rejected `auv-run` execution layer and the legacy `auv-tracing-driver` recorder stack with the approved opt-in `auv-tracing` context, run-fact, artifact, store, Inspect, and telemetry contracts.

**Architecture:** `auv-tracing` is a lightweight typed instrumentation library, not a runner, command catalog, or operation framework. Applications, CLI frontends, MCP frontends, REPLs, and app crates keep their own control flow, explicitly create `RunId` and root `Context` values, and optionally route emitted facts to one authority `RunStore` plus bounded telemetry projectors. Inspect Server can be that one authority and stores binary artifacts; Rust `tracing` and OpenTelemetry are lossy projections that never receive artifact bytes.

**Tech Stack:** Rust 2024, `serde`/`serde_json`, `uuid` V7, `mime`, `url`, `sha2`, `bytes`, `futures-*`, `pin-project`, standard-library threads and synchronization in the default crate, optional `fs2`, optional Rust `tracing`, Axum/Tokio/Reqwest for Inspect integration, OpenTelemetry 0.32 for the separate OTEL projector, Vue/TypeScript for the existing viewer.

---

## Execution Rules

- Execute in the current branch and working tree, as approved by the owner. Do not create another worktree.
- Treat [`2026-07-20-auv-run-recording-contract-v1-spec.md`](2026-07-20-auv-run-recording-contract-v1-spec.md) as the normative contract. Existing code is migration evidence only.
- Keep direct application return values independent from tracing. No task may add `Operation`, `OperationId`, `ExecutionId`, `RunSession`, a central runner, or a shared CLI/MCP result type to `auv-tracing`.
- Keep exactly one authority store per dispatch. Do not add a composite store, mirror store, recorder fan-out, or local-to-Inspect replication path.
- Use `git commit --only <task paths>` for every task because the working tree already contains owner-approved documentation edits. Before committing, run `git diff --cached --name-only` and verify that unrelated staged files are excluded.
- A task is complete only after its focused tests pass. Do not batch later tasks into an earlier commit.

## File Structure

### Canonical Core

- Create `crates/auv-tracing/` as the canonical package and remove the unconsumed `crates/auv-run/` experiment in the same task.
- Rewrite `crates/auv-tracing/src/value.rs` for validated IDs, names, timestamps, content metadata, and exact JSON bounds.
- Create `crates/auv-tracing/src/event.rs` for `Attributes`, `EventSchema`, `JsonPayload`, and `EventPayload`.
- Rewrite `crates/auv-tracing/src/artifact.rs` for canonical artifact URI, committed metadata, upload requests, byte readers, and artifact errors.
- Rewrite `crates/auv-tracing/src/history.rs` for span/event facts, mutations, commits, snapshots, and the deterministic reducer.
- Rewrite `crates/auv-tracing/src/store.rs` for the object-safe authority port and read/subscription types.
- Create `crates/auv-tracing/src/store/memory.rs` and `crates/auv-tracing/src/store/file.rs` for the two concrete authority stores.
- Create `crates/auv-tracing/src/dispatch.rs` for configuration, routing workers, barriers, and error reporting.
- Create `crates/auv-tracing/src/context.rs` for current-context propagation, span lifetime, scoped guards, and future wrappers.
- Create `crates/auv-tracing/src/propagation.rs` for the four-field cross-process carrier.
- Create `crates/auv-tracing/src/telemetry.rs` for the sealed bounded projector port.
- Create `crates/auv-tracing/src/rust_tracing.rs` behind the `rust-tracing` feature.
- Create `crates/auv-tracing/src/macros.rs` for the three action-oriented macro frontends.

### Inspect And OTEL Integrations

- Create `crates/auv-tracing-inspect/` for versioned Inspect protocol DTOs and `InspectRunStore`.
- Rewrite `crates/auv-inspect-server/src/server.rs` around `Arc<dyn RunStore>` and split V1 run and artifact routes into `run_api.rs` and `artifact_api.rs`.
- Rewrite `crates/auv-inspect-server/src/read_projection.rs` and `viewer/src/viewer.ts` against `RunSnapshot`, `RunCommit`, and `ArtifactUri`.
- Create `crates/auv-tracing-otel/` for `OtelProjector`; applications continue to own SDK provider and OTLP exporter configuration.

### Producer And Frontend Migration

- Add optional `tracing` features to selected driver/app crates; enabling a feature adds call sites but installs no dispatch.
- Delete `crates/auv-cli-invoke/src/recorded.rs` after CLI, MCP, and product call sites independently own run/context setup and call direct typed command functions.
- Change CLI and MCP composition roots to create their own `RunId` and root `Context` and then call the same app/command function.
- Convert old path-based produced artifacts to owned readers plus validated artifact metadata.
- Migrate read-side consumers from `CanonicalRun` to `RunSnapshot` and from artifact paths/roles to `ArtifactUri`/`ArtifactPurpose`.
- Remove `crates/auv-tracing-driver/` only after the final import search is empty.

## Existing `auv-run` Disposition

| Existing file | Disposition | Reason |
|---|---|---|
| `src/value.rs` and `tests/value_contract.rs` | Reimplement selectively in the new crate | Existing validation patterns are useful evidence, but limits and names are wrong and `ExecutionId`, operation names, seal reasons, and encoded operation payloads are out of contract. |
| `src/history.rs` and `tests/reducer_contract.rs` | Reimplement only applicable reducer invariants | Existing reducer technique is useful evidence; run-open, execution, verification, and run-seal entities are rejected. |
| `src/artifact.rs` | Reimplement from the V1 contract | Remove execution/verification scope, indirect `ArtifactRef`, and any caller-controlled locator; use run/span correlation and canonical `ArtifactUri`. |
| `src/operation.rs`, `src/execution.rs`, and `tests/operation_contract.rs` | Delete | These files implement the rejected central operation/execution model. |
| `src/runtime.rs` and `src/handler.rs` | Delete | `auv-tracing` does not schedule application work or expose a post-commit handler abstraction. |
| `src/store.rs` | Replace | The current file is a stub; V1 needs the object-safe `RunStore` authority port. |
| `src/otel.rs` | Delete and implement a fresh `crates/auv-tracing-otel/` package | OTEL is a bounded projector, not a core feature module or store. |

## Milestones

1. Tasks 1-5 produce a complete canonical model and in-memory authority.
2. Tasks 6-9 produce the opt-in typed API, context propagation, telemetry barriers, and artifact pipeline.
3. Tasks 10-14 produce durable local storage, Inspect authority/client, and external telemetry projections.
4. Tasks 15-23 migrate real producers/frontends/readers and retire both rejected recording stacks.

### Task 1: Replace `auv-run` With A Clean `auv-tracing` Crate

**Files:**
- Create: `crates/auv-tracing/tests/crate_identity.rs`
- Create: `crates/auv-tracing/Cargo.toml`
- Create: `crates/auv-tracing/src/lib.rs`
- Remove: `crates/auv-run/`
- Modify: `Cargo.toml`
- Modify: `docs/TERMS_AND_CONCEPTS.md`

- [ ] **Step 1: Add a failing crate-boundary test**

```rust
use std::fs;

#[test]
fn core_crate_is_lightweight_auv_tracing() {
  let manifest = fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
  assert!(manifest.contains("name = \"auv-tracing\""));
  for forbidden in ["tokio", "reqwest", "opentelemetry", "RunSession", "OperationCatalog"] {
    assert!(!manifest.contains(forbidden), "core manifest contains {forbidden}");
  }
}
```

- [ ] **Step 2: Run the test and confirm the rejected identity is visible**

Run: `cargo test -p auv-tracing --test crate_identity`

Expected: FAIL because the workspace has no package named `auv-tracing`.

- [ ] **Step 3: Rename the crate, remove the rejected execution modules, and define the exact feature boundary**

Create the new package from the approved surface and remove `crates/auv-run`
instead of renaming or preserving its module graph. Use this manifest
feature/dependency surface:

```toml
[features]
default = []
memory-store = []
file-store = ["dep:fs2"]
rust-tracing = ["dep:tracing"]

[dependencies]
bytes.workspace = true
futures-core.workspace = true
futures-io.workspace = true
futures-util = { workspace = true, features = ["io", "sink"] }
futures-channel = "0.3"
futures-executor = { version = "0.3", features = ["thread-pool"] }
fs2 = { workspace = true, optional = true }
hex.workspace = true
mime.workspace = true
pin-project = "1"
serde.workspace = true
serde_json = { workspace = true, features = ["arbitrary_precision", "raw_value"] }
sha2.workspace = true
thiserror.workspace = true
time.workspace = true
tracing = { version = "0.1", optional = true }
url = { version = "2", features = ["serde"] }
uuid.workspace = true
```

Add `futures-io = "0.3"` and `tempfile = "3"` to
`[workspace.dependencies]`, change the workspace
member to `crates/auv-tracing`, and keep the first crate root deliberately
minimal so every declared feature compiles before later modules land:

```rust
#![forbid(unsafe_code)]

//! Typed, opt-in AUV instrumentation and run-data contracts.
```

- [ ] **Step 4: Replace the top-level vocabulary with the approved terms**

Make `Run`, `AuthorityId`, operation scope, span, event, artifact, `Dispatch`, `Context`, `RunStore`, `RunCommit`, and projection normative. State explicitly:

```markdown
An operation scope is an ordinary caller-named AUV span around app or driver
work. It is not a persisted operation entity and does not require an AUV-owned
operation trait, runner, execution id, or session object.

A run is an explicitly created correlation, persistence, inspection, and replay
scope. It has no start, finish, status, or seal fact in V1 and is not an
OpenTelemetry trace.
```

Move the old operation-execution and recorder definitions under historical terms. Remove claims that every span/event belongs to an operation execution or that every run has device/session/status fields.

- [ ] **Step 5: Run the boundary test**

Run: `cargo test -p auv-tracing --test crate_identity`

Expected: PASS.

- [ ] **Step 6: Commit the crate identity change**

```bash
git add -A -- Cargo.toml Cargo.lock crates/auv-run crates/auv-tracing docs/TERMS_AND_CONCEPTS.md
git commit --only Cargo.toml Cargo.lock crates/auv-run crates/auv-tracing docs/TERMS_AND_CONCEPTS.md -m "refactor(auv-tracing): replace rejected run execution crate"
```

### Task 2: Implement Validated Core Values, Attributes, Events, And Artifact URI

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/value.rs`
- Create: `crates/auv-tracing/src/event.rs`
- Create: `crates/auv-tracing/src/artifact.rs`
- Create: `crates/auv-tracing/tests/value_contract.rs`

- [ ] **Step 1: Replace the value tests with failing V1 invariant tests**

```rust
use auv_tracing::{
  ArtifactId, ArtifactUri, AttributeKey, AttributeValue, Attributes, EventPayload,
  ContentType, EventName, EventSchema, JsonPayload, PageLimit, RunId, RunRevision,
  Sha256Digest,
};
use serde::Serialize;

#[derive(Serialize)]
struct SampleEvent {
  count: u64,
}

impl EventPayload for SampleEvent {
  const NAME: &'static str = "auv.test.sample";
  const VERSION: u32 = 1;
}

#[test]
fn artifact_uri_has_one_canonical_form() {
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let uri = ArtifactUri::from_ids(run_id, artifact_id);
  assert_eq!(uri.run_id(), run_id);
  assert_eq!(uri.artifact_id(), artifact_id);
  assert!("auv://runs/not-a-uuid/artifacts/nope".parse::<ArtifactUri>().is_err());
  assert!(format!("{uri}?download=1").parse::<ArtifactUri>().is_err());
  assert!(format!("{uri}#fragment").parse::<ArtifactUri>().is_err());
}

#[test]
fn attributes_enforce_v1_shape_and_size() {
  let key = AttributeKey::parse("auv.test.label").unwrap();
  let attrs = Attributes::try_from_iter([(key, AttributeValue::string("ok").unwrap())]).unwrap();
  assert_eq!(attrs.len(), 1);
  assert!(AttributeKey::parse("Label").is_err());
  assert!(AttributeValue::integer(9_007_199_254_740_992).is_err());
}

#[test]
fn event_schema_and_payload_are_bounded() {
  let schema = EventSchema::for_payload::<SampleEvent>().unwrap();
  let payload = JsonPayload::encode(&SampleEvent { count: 4 }).unwrap();
  assert_eq!(schema.version().get(), 1);
  assert_eq!(payload.get(), r#"{"count":4}"#);
}

#[test]
fn revisions_stop_at_javascript_exact_integer_limit() {
  assert!(RunRevision::new(9_007_199_254_740_991).is_ok());
  assert!(RunRevision::new(9_007_199_254_740_992).is_err());
}

#[test]
fn page_and_content_type_values_have_concrete_bounds() {
  assert!(PageLimit::new(1024).is_ok());
  assert!(PageLimit::new(1025).is_err());
  assert!(ContentType::parse("image/png").is_ok());
  assert!(ContentType::parse("image/*").is_err());
  assert!(ContentType::parse(&format!("text/plain; label={}", "a".repeat(256))).is_err());
}

#[test]
fn digest_requires_lowercase_sha256_hex() {
  assert!("A123".parse::<Sha256Digest>().is_err());
  assert!("00".repeat(32).parse::<Sha256Digest>().is_ok());
}

#[test]
fn namespaced_names_are_bounded_by_encoded_bytes() {
  let accepted = format!("auv.test.{}", "a".repeat(119));
  let rejected = format!("auv.test.{}", "a".repeat(120));
  assert_eq!(accepted.len(), 128);
  assert!(EventName::parse(&accepted).is_ok());
  assert!(EventName::parse(&rejected).is_err());
}

#[test]
fn json_payload_rejects_duplicate_object_keys() {
  assert!(JsonPayload::from_str(r#"{"outer":{"value":1,"value":2}}"#).is_err());
}
```

- [ ] **Step 2: Run the focused test and confirm the old model fails**

Run: `cargo test -p auv-tracing --test value_contract`

Expected: FAIL with unresolved `AuthorityId`, `ArtifactUri`, `EventPayload`, or the new constructor names.

- [ ] **Step 3: Implement the exact public value surface and bounds**

Implement private-field newtypes for:

```rust
pub struct RunId(uuid::Uuid);
pub struct AuthorityId(uuid::Uuid);
pub struct SpanId(uuid::Uuid);
pub struct EventId(uuid::Uuid);
pub struct ArtifactId(uuid::Uuid);
pub struct RunRevision(u64);
pub struct IdempotencyKey(uuid::Uuid);
pub struct PageLimit(std::num::NonZeroU32);
pub struct BoundedString(String);
pub struct FiniteF64(f64);
pub struct NonEmptyVec<T>(Vec<T>);
pub struct NamespacedName(String);
pub struct AttributeKey(NamespacedName);
pub struct ErrorCode(NamespacedName);
pub struct SpanName(NamespacedName);
pub struct EventName(NamespacedName);
pub struct ArtifactPurpose(NamespacedName);
pub struct ContentType(mime::Mime);
pub struct ByteLength(u64);
pub struct Sha256Digest([u8; 32]);
pub struct Timestamp {
  unix_seconds: i64,
  nanoseconds: u32,
}
```

Use these fixed bounds: 128 UTF-8 bytes for the complete namespaced name, 32
attributes, 1,024 UTF-8 bytes per attribute string, 16,384 compact-JSON bytes
per `Attributes`, 64 KiB per event JSON payload, 512 MiB per V1 artifact, 256
resolver URIs, 1,024 commits per page, 256 UTF-8 bytes for canonical concrete
MIME text, and `9_007_199_254_740_991` as the largest revision/integer. Reject
nil UUIDs, non-finite floats, invalid names, wildcard MIME types, duplicate
attribute keys, null/object/array/bytes attribute values, and unknown fields
during deserialization.

- [ ] **Step 4: Implement the typed event adapter and canonical artifact URI parser**

```rust
pub enum AttributeValue {
  Bool(bool),
  I64(i64),
  F64(FiniteF64),
  String(BoundedString),
}

pub struct Attributes(std::collections::BTreeMap<AttributeKey, AttributeValue>);

pub struct EventSchema {
  name: EventName,
  version: std::num::NonZeroU32,
}

pub struct JsonPayload(Box<serde_json::value::RawValue>);

pub trait EventPayload: serde::Serialize {
  const NAME: &'static str;
  const VERSION: u32;
}

pub struct ArtifactUri(url::Url);

impl ArtifactUri {
  pub fn from_ids(run_id: RunId, artifact_id: ArtifactId) -> Self;
  pub fn run_id(&self) -> RunId;
  pub fn artifact_id(&self) -> ArtifactId;
}
```

Canonicalize event object key order before storing `RawValue`, preserve exact JSON integers, and reject values outside the exact integer range. Parse wire JSON with a recursive map visitor before constructing `serde_json::Value`; parsing to `Value` first is forbidden because duplicate object keys would already have been overwritten. Add `#[serde(deny_unknown_fields)]` to every wire struct, and use externally tagged enums whose inner structs also deny unknown fields. `ArtifactUri::from_str` must require `auv://runs/{run_id}/artifacts/{artifact_id}` with no user info, port, query, fragment, extra segment, traversal, or alternate escaping.

Expose only the modules implemented in this task:

```rust
mod artifact;
mod event;
mod value;

pub use artifact::*;
pub use event::*;
pub use value::*;
```

- [ ] **Step 5: Run the value tests**

Run: `cargo test -p auv-tracing --test value_contract`

Expected: PASS.

- [ ] **Step 6: Commit the values**

```bash
git add crates/auv-tracing/src/lib.rs crates/auv-tracing/src/value.rs crates/auv-tracing/src/event.rs crates/auv-tracing/src/artifact.rs crates/auv-tracing/tests/value_contract.rs
git commit --only crates/auv-tracing/src/lib.rs crates/auv-tracing/src/value.rs crates/auv-tracing/src/event.rs crates/auv-tracing/src/artifact.rs crates/auv-tracing/tests/value_contract.rs -m "feat(auv-tracing): add validated run values"
```

### Task 3: Implement Canonical Facts, Commits, And The Deterministic Reducer

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/history.rs`
- Create: `crates/auv-tracing/tests/reducer_contract.rs`

- [ ] **Step 1: Add failing tests for the two-axis-free span/event model**

```rust
use auv_tracing::*;

fn timestamp(seconds: i64) -> Timestamp {
  Timestamp::new(seconds, 0).unwrap()
}

fn span_started(span_id: SpanId) -> SpanStarted {
  SpanStarted::new(
    span_id,
    None,
    None,
    SpanName::parse("auv.test.operation").unwrap(),
    timestamp(10),
    Attributes::empty(),
  )
}

#[test]
fn span_end_contains_no_outcome_or_status() {
  let json = serde_json::to_value(SpanEnded::new(SpanId::new(), timestamp(11))).unwrap();
  assert_eq!(json.as_object().unwrap().len(), 2);
  assert!(json.get("span_id").is_some());
  assert!(json.get("ended_at").is_some());
}

#[test]
fn missing_span_end_remains_open_without_inferred_reason() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let commit = RunCommit::new(
    authority,
    run,
    RunRevision::new(1).unwrap(),
    IdempotencyKey::new(),
    timestamp(10),
    vec![RunFact::SpanStarted(span_started(span))],
  ).unwrap();
  let snapshot = reduce_commits(&[commit]).unwrap();
  assert!(snapshot.spans().get(&span).unwrap().ended().is_none());
}

#[test]
fn reducer_rejects_event_after_span_end() {
  let authority = AuthorityId::new();
  let run = RunId::new();
  let span = SpanId::new();
  let event = EventOccurred::new(
    EventId::new(),
    Some(span),
    timestamp(12),
    EventSchema::new(EventName::parse("auv.test.event").unwrap(), 1).unwrap(),
    JsonPayload::from_str(r#"{"value":1}"#).unwrap(),
  );
  let commits = vec![
    RunCommit::new(
      authority,
      run,
      RunRevision::new(1).unwrap(),
      IdempotencyKey::new(),
      timestamp(10),
      vec![RunFact::SpanStarted(span_started(span))],
    ).unwrap(),
    RunCommit::new(
      authority,
      run,
      RunRevision::new(2).unwrap(),
      IdempotencyKey::new(),
      timestamp(11),
      vec![RunFact::SpanEnded(SpanEnded::new(span, timestamp(11)))],
    ).unwrap(),
    RunCommit::new(
      authority,
      run,
      RunRevision::new(3).unwrap(),
      IdempotencyKey::new(),
      timestamp(12),
      vec![RunFact::EventOccurred(event)],
    ).unwrap(),
  ];
  assert_eq!(reduce_commits(&commits).unwrap_err(), ReduceError::EventAfterSpanEnd);
}

#[test]
fn typed_wire_records_reject_unknown_fields() {
  let event = format!(
    r#"{{"event_id":"{}","span_id":null,"occurred_at":{{"unix_seconds":1,"nanoseconds":0}},"schema":{{"name":"auv.test.event","version":1}},"payload":{{"value":1}},"surprise":true}}"#,
    EventId::new(),
  );
  assert!(serde_json::from_str::<EventOccurred>(&event).is_err());
}

#[test]
fn ordinary_commit_batches_are_bounded() {
  let mutations = (0..257)
    .map(|_| RunMutation::EndSpan(SpanEnded::new(SpanId::new(), timestamp(11))))
    .collect::<Vec<_>>();
  assert!(RunCommitRequest::new(
    AuthorityId::new(),
    RunId::new(),
    IdempotencyKey::new(),
    mutations,
  ).is_err());
}
```

The test must use the same validated public constructors as HTTP and store
adapters; do not add test-only record constructors.

- [ ] **Step 2: Run the reducer tests**

Run: `cargo test -p auv-tracing --test reducer_contract`

Expected: FAIL because the old history still requires run-open/execution/seal changes.

- [ ] **Step 3: Replace the history types with the V1 unions**

```rust
pub struct SpanLink {
  span_id: SpanId,
}

pub struct SpanStarted {
  span_id: SpanId,
  parent_span_id: Option<SpanId>,
  remote_link: Option<SpanLink>,
  name: SpanName,
  started_at: Timestamp,
  attributes: Attributes,
}

pub struct SpanEnded {
  span_id: SpanId,
  ended_at: Timestamp,
}

pub struct EventOccurred {
  event_id: EventId,
  span_id: Option<SpanId>,
  occurred_at: Timestamp,
  schema: EventSchema,
  payload: JsonPayload,
}

pub struct ArtifactMetadata {
  uri: ArtifactUri,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  byte_length: ByteLength,
  sha256: Sha256Digest,
  attributes: Attributes,
}

pub struct ArtifactPublished {
  span_id: Option<SpanId>,
  metadata: ArtifactMetadata,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunMutation {
  StartSpan(SpanStarted),
  EndSpan(SpanEnded),
  EmitEvent(EventOccurred),
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunFact {
  SpanStarted(SpanStarted),
  SpanEnded(SpanEnded),
  EventOccurred(EventOccurred),
  ArtifactPublished(ArtifactPublished),
}
```

Serialize each union as one externally tagged key. Do not add a `kind`, `status`, `summary`, result, verification, operation, or run-lifecycle field.
Add `#[serde(deny_unknown_fields)]` to `SpanLink`, `SpanStarted`, `SpanEnded`,
`EventOccurred`, `ArtifactMetadata`, and `ArtifactPublished` as well as the two
enum containers; strictness must hold inside the selected variant payload.

Add `mod history;` and `pub use history::*;` to `lib.rs` in this task.

- [ ] **Step 4: Implement commit and snapshot invariants**

```rust
pub struct RunCommitRequest {
  authority_id: AuthorityId,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  mutations: NonEmptyVec<RunMutation>,
}

pub struct RunCommit {
  authority_id: AuthorityId,
  run_id: RunId,
  revision: RunRevision,
  idempotency_key: IdempotencyKey,
  committed_at: Timestamp,
  facts: NonEmptyVec<RunFact>,
}

pub struct RunSnapshot {
  authority_id: AuthorityId,
  run_id: RunId,
  through_revision: RunRevision,
  spans: std::collections::BTreeMap<SpanId, SpanSnapshot>,
  events: Vec<EventOccurred>,
  artifacts: std::collections::BTreeMap<ArtifactUri, ArtifactPublished>,
}

pub struct SpanSnapshot {
  started: SpanStarted,
  ended: Option<SpanEnded>,
}
```

The reducer must reject non-contiguous revisions, mixed authority/run IDs,
duplicate span starts/ends, missing or cyclic local parents, parent plus
duplicate remote link, end-before-start, event outside an open span, duplicate
event IDs, duplicate artifact URIs, and unknown artifact span IDs. Constructors
and deserializers reject ordinary mutation or fact batches outside `1..=256`
before reduction. Artifact publication after its span end remains valid.

- [ ] **Step 5: Run reducer and serialization tests**

Run: `cargo test -p auv-tracing --test reducer_contract`

Expected: PASS.

- [ ] **Step 6: Commit the canonical history**

```bash
git add crates/auv-tracing/src/lib.rs crates/auv-tracing/src/history.rs crates/auv-tracing/tests/reducer_contract.rs
git commit --only crates/auv-tracing/src/lib.rs crates/auv-tracing/src/history.rs crates/auv-tracing/tests/reducer_contract.rs -m "feat(auv-tracing): define canonical run facts"
```

### Task 4: Define The Object-Safe `RunStore` Port And Shared Conformance Harness

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/store.rs`
- Create: `crates/auv-tracing-conformance/Cargo.toml`
- Create: `crates/auv-tracing-conformance/src/lib.rs`
- Modify: `Cargo.toml`
- Create: `crates/auv-tracing/tests/store_port_contract.rs`

- [ ] **Step 1: Add a compile-time dyn-compatibility test**

```rust
use std::sync::Arc;
use auv_tracing::RunStore;

fn accepts_dyn_store(_store: Arc<dyn RunStore>) {}

#[test]
fn run_store_is_dyn_compatible() {
  let _: fn(Arc<dyn RunStore>) = accepts_dyn_store;
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p auv-tracing --test store_port_contract`

Expected: FAIL because `RunStore` is not defined.

- [ ] **Step 3: Define the exact object-safe port and error unions**

```rust
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
pub type ArtifactBody = std::pin::Pin<Box<dyn futures_io::AsyncRead + Send>>;
pub type ArtifactReader = std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<bytes::Bytes, ArtifactReadError>> + Send>>;
pub type RunSubscription = std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<RunCommit, SubscriptionError>> + Send>>;

pub trait RunStore: Send + Sync {
  fn authority_id(&self) -> AuthorityId;
  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>>;
  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>>;
  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>>;
  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>>;
  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>>;
  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>>;
  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>>;
}

pub struct StoreArtifactRequest {
  authority_id: AuthorityId,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  artifact_id: ArtifactId,
  span_id: Option<SpanId>,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: Attributes,
}

pub struct RunCommitPage {
  commits: Vec<RunCommit>,
  last_revision: RunRevision,
  has_more: bool,
}

pub enum CommitError {
  AuthorityMismatch { expected: AuthorityId, received: AuthorityId },
  IdempotencyMismatch,
  Rejected(ErrorCode),
  Unavailable(ErrorCode),
  CommitUnknown(ErrorCode),
}

pub enum ArtifactWriteError {
  AuthorityMismatch { expected: AuthorityId, received: AuthorityId },
  IdempotencyMismatch,
  Rejected(ErrorCode),
  Integrity(ErrorCode),
  Unavailable(ErrorCode),
  PublicationUnknown(ErrorCode),
}

pub enum ReadError {
  NotFound,
  Forbidden,
  InvalidReference(ErrorCode),
  HistoryGap { requested_after: RunRevision, earliest_available: RunRevision },
  CursorAhead { requested_after: RunRevision, latest: RunRevision },
  Unavailable(ErrorCode),
  Integrity(ErrorCode),
}

pub enum ArtifactReadError {
  Unavailable(ErrorCode),
  Integrity(ErrorCode),
}

pub enum SubscriptionError {
  Gap { requested_after: RunRevision, earliest_available: RunRevision },
  Store(ReadError),
}
```

Generic typed decoding, `async fn`, associated `impl Stream`, paths, retry advice, and second-store routing are forbidden on this trait.
Add `mod store;` and `pub use store::*;` to `lib.rs` in this task.

- [ ] **Step 4: Implement the complete reusable conformance harness**

```rust
pub async fn assert_store_contract(make: impl Fn() -> std::sync::Arc<dyn auv_tracing::RunStore>) {
  authority_is_stable_and_non_nil(make()).await;
  authority_mismatch_precedes_invalid_mutation(make()).await;
  authority_mismatch_does_not_poll_artifact_body(make()).await;
  revisions_start_at_one_and_remain_contiguous(make()).await;
  equal_commit_replay_returns_the_original_commit(make()).await;
  mismatched_commit_replay_is_rejected(make()).await;
  lookup_resolves_an_already_committed_request(make()).await;
  event_ids_are_unique_across_idempotency_keys(make()).await;
  snapshot_reduction_is_deterministic(make()).await;
  page_cursors_cover_pagination_ahead_and_empty(make()).await;
  pages_stop_at_the_canonical_byte_budget(make()).await;
  equal_artifact_replay_does_not_poll_replacement_body(make()).await;
  artifact_id_conflict_cannot_replace_committed_bytes(make()).await;
  artifact_length_and_digest_are_verified_before_publication(make()).await;
  interrupted_artifact_body_publishes_no_fact(make()).await;
  open_artifact_returns_exact_committed_bytes(make()).await;
  subscription_resumes_after_cursor_without_a_snapshot_race(make()).await;
}

pub async fn assert_gap_contract(
  store: std::sync::Arc<dyn auv_tracing::RunStore>,
  induce_retention_gap: impl FnOnce(),
) {
  page_and_subscription_report_the_same_typed_gap(store, induce_retention_gap).await;
}
```

Keep this harness in the non-published `auv-tracing-conformance` package so
memory, file, and remote Inspect authorities execute the same public-behavior
suite without exposing test-only APIs from `auv-tracing`. Add the package to the
workspace and depend on `auv-tracing` with `memory-store` and `file-store`.

Implement every named function in `auv-tracing-conformance/src/lib.rs` in this
task. Each function creates a fresh run and uses only public constructors and
`RunStore` methods. Use one shared `sample_event_request(run_id, key, event_id,
value)` builder, a `ProbeArtifactBody` that records its first poll, and a
`collect_artifact(ArtifactReader)` helper. The required concrete assertions are:

| Function | Required assertion |
|---|---|
| `authority_is_stable_and_non_nil` | two `authority_id()` calls are equal and non-nil |
| `authority_mismatch_precedes_invalid_mutation` | a wrong-authority, well-formed mutation that would fail reducer validation returns `AuthorityMismatch`, proving precedence |
| `authority_mismatch_does_not_poll_artifact_body` | returns `AuthorityMismatch`; probe remains unpolled |
| `revisions_start_at_one_and_remain_contiguous` | two distinct commits return revisions 1 and 2 |
| `equal_commit_replay_returns_the_original_commit` | equal key/request returns byte-equivalent first commit and does not append |
| `mismatched_commit_replay_is_rejected` | same key/different canonical request returns `IdempotencyMismatch` |
| `lookup_resolves_an_already_committed_request` | lookup returns the exact commit after caller discards the first response |
| `event_ids_are_unique_across_idempotency_keys` | duplicate `EventId` under a new key returns `Rejected` |
| `snapshot_reduction_is_deterministic` | snapshot equals `reduce_commits(commits_after(0))` |
| `page_cursors_cover_pagination_ahead_and_empty` | page limits advance `last_revision`; latest is empty; a future cursor returns `CursorAhead` |
| `pages_stop_at_the_canonical_byte_budget` | commits remain in order, a page stops before exceeding 32 MiB of compact canonical JSON, `has_more` is true, and the next cursor makes progress |
| `equal_artifact_replay_does_not_poll_replacement_body` | exact replay returns original commit and replacement probe remains unpolled |
| `artifact_id_conflict_cannot_replace_committed_bytes` | same ID/different metadata or key fails; original bytes remain readable |
| `artifact_length_and_digest_are_verified_before_publication` | mismatch returns typed error and snapshot has no artifact fact |
| `interrupted_artifact_body_publishes_no_fact` | reader error returns a pre-publication write error and leaves no metadata/blob reference |
| `open_artifact_returns_exact_committed_bytes` | collected bytes, content length, and SHA-256 equal committed metadata |
| `subscription_resumes_after_cursor_without_a_snapshot_race` | subscription after snapshot revision receives the next commit exactly once |
| `page_and_subscription_report_the_same_typed_gap` | after induced retention, page returns `ReadError::HistoryGap` and subscription returns the matching `SubscriptionError::Gap` |

Every backend runs `assert_store_contract`. Backends with bounded retention run
`assert_gap_contract` with a test fixture action that advances the earliest
available revision; Memory and Inspect use this in Tasks 5 and 13. The
append-only V1 `FileRunStore` has no retention action, so its base conformance
test does not claim to exercise an unreachable gap.

- [ ] **Step 5: Run the port and conformance-library checks**

Run: `cargo test -p auv-tracing --test store_port_contract && cargo check -p auv-tracing-conformance`

Expected: PASS.

- [ ] **Step 6: Commit the port**

```bash
git add Cargo.toml Cargo.lock crates/auv-tracing/src/lib.rs crates/auv-tracing/src/store.rs crates/auv-tracing/tests/store_port_contract.rs crates/auv-tracing-conformance
git commit --only Cargo.toml Cargo.lock crates/auv-tracing/src/lib.rs crates/auv-tracing/src/store.rs crates/auv-tracing/tests/store_port_contract.rs crates/auv-tracing-conformance -m "feat(auv-tracing): define run store authority port"
```

### Task 5: Implement `MemoryRunStore`, Artifacts, Paging, And Subscriptions

**Files:**
- Create: `crates/auv-tracing/src/store/memory.rs`
- Modify: `crates/auv-tracing/src/store.rs`
- Modify: `crates/auv-tracing-conformance/src/lib.rs`
- Create: `crates/auv-tracing-conformance/tests/memory.rs`

- [ ] **Step 1: Wire the complete conformance harness to `MemoryRunStore`**

```rust
#[test]
fn memory_store_satisfies_authority_contract() {
  futures_executor::block_on(assert_store_contract(|| {
    std::sync::Arc::new(auv_tracing::MemoryRunStore::new(auv_tracing::AuthorityId::new()))
  }));
}

#[test]
fn memory_store_reports_retention_gaps() {
  let fixture = RetainedMemoryFixture::new(2);
  futures_executor::block_on(assert_gap_contract(fixture.store(), || fixture.evict_oldest()));
}
```

The harness already contains every case from Task 4. `RetainedMemoryFixture`
constructs `MemoryRunStore::with_history_limit(authority_id,
NonZeroUsize::new(2).unwrap())`; the ordinary constructor remains unbounded.
No memory-specific assertion is added here.

- [ ] **Step 2: Run the memory conformance test**

Run: `cargo test -p auv-tracing-conformance --test memory`

Expected: FAIL because `MemoryRunStore` is not defined.

- [ ] **Step 3: Implement the memory authority state**

```rust
pub struct MemoryRunStore {
  authority_id: AuthorityId,
  state: std::sync::Mutex<MemoryState>,
  changed: std::sync::Condvar,
}

struct MemoryState {
  runs: std::collections::HashMap<RunId, MemoryRun>,
  blobs: std::collections::HashMap<ArtifactUri, std::sync::Arc<[u8]>>,
}

struct MemoryRun {
  commits: Vec<RunCommit>,
  idempotency: std::collections::HashMap<IdempotencyKey, StoredRequest>,
  event_ids: std::collections::HashSet<EventId>,
  artifact_ids: std::collections::HashSet<ArtifactId>,
}

struct StoredRequest {
  fingerprint: Sha256Digest,
  commit: RunCommit,
}
```

Add the feature-gated `store::memory` module and `MemoryRunStore` export from
`store.rs` in this task; the default feature set still compiles without the
implementation.

Validate a request completely before locking in a revision. Build candidate facts, reduce the prior snapshot plus candidate commit, then append atomically. Hash the canonical validated request without incoming JSON spelling. Keep memory history untrimmed by default and expose the explicit `with_history_limit` constructor for short sessions and gap tests; retention never activates implicitly.

- [ ] **Step 4: Implement streaming artifact writes and cursor subscriptions**

Read `ArtifactBody` incrementally into bounded chunks, stop immediately above expected length or 512 MiB, hash while reading, and publish bytes plus `ArtifactPublished` under one store state transition. Implement subscription as a cursor-driven stream that rereads committed history and registers a waker; do not rely on lossy broadcast delivery.

The repeated equal artifact request must return the stored commit before polling its replacement body. `open_artifact` returns chunks from the immutable committed `Arc<[u8]>` and verifies length/digest before the first item.

- [ ] **Step 5: Run memory-store and reducer tests**

Run: `cargo test -p auv-tracing-conformance --test memory && cargo test -p auv-tracing --features memory-store --test reducer_contract`

Expected: PASS.

- [ ] **Step 6: Commit the memory store**

```bash
git add crates/auv-tracing/src/store.rs crates/auv-tracing/src/store/memory.rs crates/auv-tracing-conformance/src/lib.rs crates/auv-tracing-conformance/tests/memory.rs
git commit --only crates/auv-tracing/src/store.rs crates/auv-tracing/src/store/memory.rs crates/auv-tracing-conformance/src/lib.rs crates/auv-tracing-conformance/tests/memory.rs -m "feat(auv-tracing): add memory run store"
```

### Task 6: Implement Dispatch Selection, Current Context, And Disabled Behavior

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/dispatch.rs`
- Create: `crates/auv-tracing/src/context.rs`
- Create: `crates/auv-tracing/src/macros.rs`
- Create: `crates/auv-tracing/tests/context_contract.rs`
- Create: `crates/auv-tracing/tests/support/mod.rs`

- [ ] **Step 1: Add failing disabled/current-context tests**

```rust
use auv_tracing::{Context, RunId, SpanSpec, Attributes};

struct TestSpan;
impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.operation";
  fn attributes(&self) -> Attributes { Attributes::empty() }
}

#[test]
fn root_creation_does_not_install_current_context() {
  let before = Context::current();
  let root = Context::root(RunId::new());
  assert!(!root.is_enabled());
  assert_eq!(Context::current().run_id(), before.run_id());
}

#[test]
fn disabled_calls_do_not_create_a_run() {
  let root = Context::root(RunId::new());
  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    assert!(!span.is_enabled());
  });
}

#[test]
fn root_context_without_emissions_creates_no_stored_run() {
  let fixture = TestDispatch::memory();
  let run_id = RunId::new();
  let _root = fixture.context(run_id);
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert!(futures_executor::block_on(fixture.store.load_snapshot(run_id)).unwrap().is_none());
}

#[test]
fn authority_commits_follow_submission_order() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let root = fixture.root();
  root.in_scope(|| {
    auv_tracing::emit_event!(TestEvent { value: 1 });
    auv_tracing::emit_event!(TestEvent { value: 2 });
  });
  store.release_first_commit();
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(store.committed_event_values(), [1, 2]);
  assert_eq!(store.committed_revisions(), [1, 2]);
}

#[test]
fn blocked_run_does_not_block_an_independent_run() {
  let store = ControlledStore::new();
  let fixture = TestDispatch::with_store(store.clone());
  let run_a = fixture.context(RunId::new());
  let run_b = fixture.context(RunId::new());
  store.block_run(run_a.run_id().copied().unwrap());
  run_a.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  run_b.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  store.wait_until_committed(run_b.run_id().copied().unwrap());
  assert_eq!(store.event_values(run_b.run_id().copied().unwrap()), [2]);
  store.release_run(run_a.run_id().copied().unwrap());
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
}
```

- [ ] **Step 2: Run the context test**

Run: `cargo test -p auv-tracing --features memory-store --test context_contract`

Expected: FAIL because `Dispatch`, `Context`, `SpanSpec`, and macros are absent.

- [ ] **Step 3: Implement dispatch configuration and scoped defaults**

```rust
#[derive(Clone)]
pub struct Dispatch {
  inner: std::sync::Arc<DispatchInner>,
}

pub fn configure() -> DispatchBuilder;

pub type DispatchTask = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;

pub trait TaskSpawner: Send + Sync {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError>;
}

pub struct TaskSpawnError {
  code: ErrorCode,
}

impl TaskSpawnError {
  pub fn new(code: ErrorCode) -> Self;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchStage {
  Encode,
  Spawn,
  AuthorityCommit,
  AuthorityRead,
  Project,
  ProjectorFlush,
  ArtifactWrite,
}

pub struct DispatchFailure {
  stage: DispatchStage,
  code: ErrorCode,
}

pub struct FlushError {
  failures: NonEmptyVec<DispatchFailure>,
}

impl FlushError {
  pub fn failure_count(&self) -> std::num::NonZeroUsize;
  pub fn first(&self) -> &DispatchFailure;
}

impl DispatchFailure {
  pub fn stage(&self) -> DispatchStage;
  pub fn code(&self) -> &ErrorCode;
}

pub struct ThreadTaskSpawner {
  pool: futures_executor::ThreadPool,
}

impl DispatchBuilder {
  pub fn run_store(self, store: std::sync::Arc<dyn RunStore>) -> Self;
  pub fn task_spawner(self, spawner: std::sync::Arc<dyn TaskSpawner>) -> Self;
  pub fn build(self) -> Result<Dispatch, BuildError>;
}

impl Dispatch {
  pub fn flush(&self) -> BoxFuture<'_, Result<(), FlushError>>;
}

pub mod dispatcher {
  pub fn set_global_default(dispatch: Dispatch) -> Result<(), SetGlobalDefaultError>;
  pub fn with_default<T>(dispatch: &Dispatch, f: impl FnOnce() -> T) -> T;
}
```

Use a thread-local scoped-dispatch stack and a one-time global default. Restore
the previous scoped dispatch on normal return and unwind. Building with one
store captures its stable `AuthorityId`; there is no API for a second store.
The builder defaults to `ThreadTaskSpawner`; later runtime-specific adapters can
override it without adding Tokio to core. The builder lazily starts one
serialized authority fact lane per `RunId`. Enabled span/event submission reserves a dispatch ticket
before validation; each valid request waits for the previous authority request
to become terminal before it calls `RunStore::commit`. This is the mechanism
that preserves parent/start/event/end causality within a run without allowing a
blocked run to stall unrelated runs. Reordering completed futures after
concurrent same-run store calls is explicitly forbidden.

Implement the base `flush()` in this task. Calling it synchronously captures the
current ticket as its barrier; awaiting the returned future waits until every
authority fact ticket through that barrier is terminal. It does not end spans,
seal runs, or prevent later emissions. `FlushError` contains only authority,
encoding, or task-spawn failures at this stage; Task 8 extends the same barrier
with projector failures rather than replacing it.

- [ ] **Step 4: Implement `Context`, `SpanSpec`, and macro delegation**

```rust
#[derive(Clone)]
pub struct Context {
  state: ContextState,
}

pub trait SpanSpec {
  const NAME: &'static str;
  fn attributes(&self) -> Attributes;
}

pub fn start_span(spec: impl SpanSpec) -> Span;
pub fn emit_event(event: impl EventPayload);

#[macro_export]
macro_rules! start_span {
  ($spec:expr) => { $crate::start_span($spec) };
}

#[macro_export]
macro_rules! emit_event {
  ($event:expr) => { $crate::emit_event($event) };
}
```

`Context::root` captures the current dispatch and run ID but emits nothing and does not make itself current. `Context::current` returns a disabled context if no scope is active. `enter` is thread-bound; `in_scope` uses it and restores on unwind. A disabled span has no ID. `emit_event` returns `()` in every mode.

Create the shared integration-test fixture with this concrete base:

```rust
pub struct TestDispatch {
  pub dispatch: auv_tracing::Dispatch,
  pub store: std::sync::Arc<auv_tracing::MemoryRunStore>,
}

impl TestDispatch {
  pub fn memory() -> Self {
    let store = std::sync::Arc::new(auv_tracing::MemoryRunStore::new(auv_tracing::AuthorityId::new()));
    let dispatch = auv_tracing::configure().run_store(store.clone()).build().unwrap();
    Self { dispatch, store }
  }

  pub fn context(&self, run_id: auv_tracing::RunId) -> auv_tracing::Context {
    auv_tracing::dispatcher::with_default(&self.dispatch, || auv_tracing::Context::root(run_id))
  }

  pub fn root(&self) -> auv_tracing::Context {
    self.context(auv_tracing::RunId::new())
  }
}
```

Tasks 7-9 extend this test module with projector and reader probes; those probes
remain test-only.
Add `context`, `dispatch`, and `macros` modules and re-export their public
surfaces from `lib.rs` in this task.

- [ ] **Step 5: Run the context tests**

Run: `cargo test -p auv-tracing --features memory-store --test context_contract`

Expected: PASS.

- [ ] **Step 6: Commit dispatch selection and disabled behavior**

```bash
git add crates/auv-tracing/src/lib.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/context.rs crates/auv-tracing/src/macros.rs crates/auv-tracing/tests/context_contract.rs crates/auv-tracing/tests/support/mod.rs
git commit --only crates/auv-tracing/src/lib.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/context.rs crates/auv-tracing/src/macros.rs crates/auv-tracing/tests/context_contract.rs crates/auv-tracing/tests/support/mod.rs -m "feat(auv-tracing): add dispatch and context surface"
```

### Task 7: Implement Span Lifetime, Async Wrappers, And Cross-Process Propagation

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Modify: `crates/auv-tracing/src/context.rs`
- Modify: `crates/auv-tracing/tests/support/mod.rs`
- Create: `crates/auv-tracing/src/propagation.rs`
- Create: `crates/auv-tracing/tests/span_lifecycle_contract.rs`
- Create: `crates/auv-tracing/tests/propagation_contract.rs`

- [ ] **Step 1: Add failing lifecycle tests**

```rust
#[test]
fn last_span_or_context_clone_ends_exactly_once() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let child_context = span.context();
  drop(span);
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(), 0);
  drop(child_context);
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(), 1);
}

#[test]
fn dropping_enter_guard_does_not_end_span() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  { let _guard = span.enter(); }
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(), 0);
}
```

Add a future probe whose `poll` and `Drop` record `Context::current().run_id()`. Test both `Context::instrument` and `Span::instrument` on ready completion and cancellation before ready.

Add one concurrent test that polls two instrumented futures with distinct run
roots and asserts their events retain distinct run IDs. Add one spawn test that
shows a plain spawned task sees no current run while
`root.instrument(spawned_future)` carries the explicit run.

- [ ] **Step 2: Run lifecycle tests**

Run: `cargo test -p auv-tracing --features memory-store --test span_lifecycle_contract`

Expected: FAIL because span shared-close state and pinned wrappers are incomplete.

- [ ] **Step 3: Implement shared span close state and pinned wrappers**

```rust
struct SpanState {
  dispatch: Dispatch,
  run_id: RunId,
  span_id: SpanId,
  started_at: Timestamp,
  started_tick: std::time::Duration,
  clock: std::sync::Arc<dyn Clock>,
}

trait Clock: Send + Sync {
  fn wall_now(&self) -> Timestamp;
  fn monotonic_now(&self) -> std::time::Duration;
}

#[derive(Clone)]
pub struct Span {
  state: Option<std::sync::Arc<SpanState>>,
}

#[pin_project::pin_project(PinnedDrop)]
pub struct WithContext<F> {
  context: Context,
  #[pin]
  future: Option<F>,
}

#[pin_project::pin_project(PinnedDrop)]
pub struct Instrumented<F> {
  context: Context,
  span: Option<Span>,
  #[pin]
  future: Option<F>,
}
```

Implement `Drop for SpanState` to submit exactly one `SpanEnded`; derive
`ended_at` from wall-clock start plus monotonic elapsed. Keep the `Clock`
injection private and add a unit test in `context.rs` whose wall time moves
backward while monotonic time advances. The pinned-drop implementations enter
the captured context and call `projection.future.set(None)` before leaving that
scope, then release the span handle. The same `set(None)` sequence runs after
`Poll::Ready`. A direct `F` field is forbidden because `PinnedDrop` runs before
automatic field destruction and would drop the future outside the captured
context. The implementation remains safe Rust under `#![forbid(unsafe_code)]`.

- [ ] **Step 4: Add and run propagation failures first**

Use a map carrier to test all-absent, complete, duplicate, partial, invalid UUID, unknown version, authority mismatch, and remote-link cases. Run:

`cargo test -p auv-tracing --features memory-store --test propagation_contract`

Expected before implementation: FAIL because `extract`, `inject`, and `RemoteContext` are absent.

- [ ] **Step 5: Implement the four-field carrier**

```rust
pub struct RemoteContext {
  authority_id: Option<AuthorityId>,
  run_id: RunId,
  remote_span_id: Option<SpanId>,
}

pub trait TextMapWriter {
  fn set(&mut self, name: &'static str, value: &str);
  fn remove(&mut self, name: &'static str);
}

pub trait TextMapReader {
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a>;
}
```

Use only `auv-context-version`, `auv-run-id`, `auv-authority-id`, and `auv-span-id`. `Context::from_remote` binds the current local dispatch, rejects conflicting authorities before submission, and makes the remote span a `SpanLink` on the first local span rather than a local parent.
Add `mod propagation;` and `pub use propagation::*;` to `lib.rs`.

- [ ] **Step 6: Run both test files**

Run: `cargo test -p auv-tracing --features memory-store --test span_lifecycle_contract && cargo test -p auv-tracing --features memory-store --test propagation_contract`

Expected: PASS.

- [ ] **Step 7: Commit lifecycle and propagation**

```bash
git add crates/auv-tracing/src/lib.rs crates/auv-tracing/src/context.rs crates/auv-tracing/src/propagation.rs crates/auv-tracing/tests/span_lifecycle_contract.rs crates/auv-tracing/tests/propagation_contract.rs crates/auv-tracing/tests/support/mod.rs
git commit --only crates/auv-tracing/src/lib.rs crates/auv-tracing/src/context.rs crates/auv-tracing/src/propagation.rs crates/auv-tracing/tests/span_lifecycle_contract.rs crates/auv-tracing/tests/propagation_contract.rs crates/auv-tracing/tests/support/mod.rs -m "feat(auv-tracing): propagate scoped span context"
```

### Task 8: Implement Ordered Telemetry Projection, Error Reporting, And Flush Barriers

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/telemetry.rs`
- Modify: `crates/auv-tracing/src/dispatch.rs`
- Modify: `crates/auv-tracing/src/context.rs`
- Modify: `crates/auv-tracing/tests/support/mod.rs`
- Create: `crates/auv-tracing/tests/dispatch_contract.rs`

- [ ] **Step 1: Add failing ordered-projector and barrier tests**

```rust
#[test]
fn flush_waits_for_pre_barrier_items_and_projector_flush() {
  let projector = BlockingProjector::new();
  let fixture = TestDispatch::with_projector(projector.clone());
  fixture.root().in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let flush = fixture.dispatch.flush();
  assert_eq!(projector.projected_count(), 0);
  projector.release_project();
  projector.release_flush();
  futures_executor::block_on(flush).unwrap();
  assert_eq!(projector.calls(), ["project", "flush"]);
}

#[test]
fn failed_flush_advances_its_reported_interval() {
  let fixture = TestDispatch::with_failing_projector();
  fixture.root().in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  assert_eq!(futures_executor::block_on(fixture.dispatch.flush()).unwrap_err().failure_count().get(), 1);
  assert!(futures_executor::block_on(fixture.dispatch.flush()).is_ok());
}

#[test]
fn oversized_event_reports_encode_failure_and_reaches_no_sink() {
  let fixture = TestDispatch::memory_with_reporter();
  fixture.root().in_scope(|| auv_tracing::emit_event!(OversizedEvent::new(70 * 1024)));
  let error = futures_executor::block_on(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.first().stage(), DispatchStage::Encode);
  assert_eq!(fixture.store.commit_count(), 0);
  assert_eq!(fixture.projector.item_count(), 0);
}

#[test]
fn flush_does_not_end_spans_and_later_events_are_accepted() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.span_end_count(), 0);
  span.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.event_count(), 1);
  drop(span);
}

#[test]
fn authority_cursor_projects_local_commits_in_revision_order_and_skips_other_writers() {
  let fixture = TestDispatch::memory_with_projector();
  let root = fixture.root();
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  fixture.wait_for_authority_revision(1);
  fixture.commit_external_event_to_same_run(2);
  root.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.projected_event_values(), [1, 3]);
  assert_eq!(fixture.projected_revisions(), [1, 3]);
}

#[test]
fn unknown_commit_is_looked_up_but_never_resubmitted() {
  let store = CommitUnknownStore::with_committed_lookup();
  let fixture = TestDispatch::with_store(store.clone());
  fixture.root().in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(store.commit_calls(), 1);
  assert_eq!(store.lookup_calls(), 1);
  assert_eq!(fixture.event_count(), 1);
}

#[test]
fn unresolved_unknown_commit_quarantines_only_that_run_lane() {
  let store = CommitUnknownStore::without_lookup_result();
  let fixture = TestDispatch::with_store(store.clone());
  let affected = fixture.root();
  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 1 }));
  let error = futures_executor::block_on(fixture.dispatch.flush()).unwrap_err();
  assert_eq!(error.first().stage(), DispatchStage::AuthorityCommit);
  affected.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 2 }));
  assert!(futures_executor::block_on(fixture.dispatch.flush()).is_err());
  assert_eq!(store.commit_calls(), 1);
  assert_eq!(store.lookup_calls(), 1);
  fixture.root().in_scope(|| auv_tracing::emit_event!(TestEvent { value: 3 }));
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(store.commit_calls(), 2);
}
```

- [ ] **Step 2: Run the dispatch tests**

Run: `cargo test -p auv-tracing --features memory-store --test dispatch_contract`

Expected: FAIL because the projector lane and captured barriers are absent.

- [ ] **Step 3: Implement the object-safe bounded projector port**

```rust
pub enum TelemetryItem {
  SpanStart { authority_id: Option<AuthorityId>, run_id: RunId, span_id: SpanId, parent_span_id: Option<SpanId>, remote_span_id: Option<SpanId>, name: SpanName, started_at: Timestamp, start_revision: Option<RunRevision>, attributes: Attributes },
  SpanEnd { authority_id: Option<AuthorityId>, run_id: RunId, span_id: SpanId, ended_at: Timestamp, end_revision: Option<RunRevision> },
  Event { authority_id: Option<AuthorityId>, run_id: RunId, span_id: Option<SpanId>, event_id: EventId, schema: EventSchema, occurred_at: Timestamp, revision: Option<RunRevision> },
  Artifact { authority_id: AuthorityId, run_id: RunId, span_id: Option<SpanId>, uri: ArtifactUri, purpose: ArtifactPurpose, content_type: ContentType, byte_length: ByteLength, sha256: Sha256Digest, attributes: Attributes, revision: RunRevision },
}

pub struct TelemetryRoutePolicy {
  span_attribute_keys: std::collections::BTreeSet<AttributeKey>,
  artifact_attribute_keys: std::collections::BTreeSet<AttributeKey>,
}

impl TelemetryRoutePolicy {
  pub fn fixed_fields_only() -> Self;
  pub fn allow_span_attribute(mut self, key: AttributeKey) -> Self;
  pub fn allow_artifact_attribute(mut self, key: AttributeKey) -> Self;
}

pub trait TelemetryProjector: Send + Sync {
  fn project(&self, item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>>;
  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>>;
}

pub trait DispatchErrorReporter: Send + Sync {
  fn report(&self, failure: &DispatchFailure);
}

impl DispatchBuilder {
  pub fn on_error(self, reporter: std::sync::Arc<dyn DispatchErrorReporter>) -> Self;
}
```

Do not expose event payload JSON or artifact bodies through `TelemetryItem`.
The default policy forwards no producer attributes. The dispatch router applies
the allowlist while constructing `TelemetryItem`; a projector never receives
unfiltered attributes and therefore cannot accidentally reinterpret policy.
Add this builder method after the trait exists:

```rust
pub fn project_telemetry(
  self,
  projector: std::sync::Arc<dyn TelemetryProjector>,
  policy: TelemetryRoutePolicy,
) -> Self;
```

The fixed telemetry vocabulary is exactly:

```text
auv.authority.id
auv.run.id
auv.run.revision
auv.span.id
auv.span.name
auv.span.parent_id
auv.span.remote_id
auv.span.start_revision
auv.span.end_revision
auv.event.id
auv.event.schema.name
auv.event.schema.version
auv.artifact.uri
auv.artifact.purpose
auv.artifact.content_type
auv.artifact.byte_length
auv.artifact.sha256
```

No adapter may rename these keys to underscore variants.
Add `mod telemetry;` and `pub use telemetry::*;` to `lib.rs`.

- [ ] **Step 4: Implement dispatch sequence tracking and serialized projector workers**

Retain Task 6's serialized authority fact lane. Assign a monotonically
increasing dispatch sequence before validating every enabled emission attempt.
A validation or encoding rejection marks that ticket terminal with an error,
so the next flush observes it. Track out-of-order terminal jobs in a
`BTreeSet<u64>` and advance a contiguous completion watermark. `flush()` must
synchronously capture the current sequence before returning its future, wait
for authority completion and every projector's own `flush`, report all errors
since the previous completed barrier, then advance the reporting interval even
when returning `FlushError`. Spawn all routing workers through the selected
`TaskSpawner`; a spawn rejection records a terminal submit error for that
ticket instead of leaving the barrier pending forever.

For a dispatch with an authority, lazily establish one ordered committed-fact
cursor per run before releasing that run's first authority mutation: load the
snapshot, subscribe after `through_revision`, then submit queued mutations.
Projectors consume that cursor, not pre-commit mutations and not independently
completed store futures. Register each dispatch-owned idempotency key before
its store call, skip commits owned by other writers, and project this dispatch's
commits strictly in the authority order observed by the cursor; on
`SubscriptionError::Gap`, recover with `commits_after` and resume from the last
accepted revision. A flush waits until the committed cursor has projected
through every revision returned for its pre-barrier local submissions. This
also orders artifact-lane commits introduced in Task 9 without blocking the
fact lane. Telemetry-only dispatches have no revisions and project span/events
in submission order. Definitive authority rejections and unresolved unknown
outcomes never produce synthetic telemetry items. Projection failures call the non-blocking
`DispatchErrorReporter` and cannot roll back a commit.

When `RunStore::commit` returns `CommitUnknown`, call `lookup_commit` once with
the original run/idempotency key. An equal stored commit resolves the ticket;
`None` or a read failure terminates the barrier with an authority error and
quarantines that dispatch's per-run fact lane. Later emissions for the same run
become terminal local `auv.dispatch.run_lane_indeterminate` failures without a
store call; independent run lanes continue. The caller must use a new `RunId`;
constructing another dispatch is not permission to continue the indeterminate
run. Never resubmit the mutation, synthesize a revision, or invoke application
code. The authority cursor may still project a commit that the store proves was
accepted.

- [ ] **Step 5: Run dispatch, context, and reducer tests**

Run: `cargo test -p auv-tracing --features memory-store --test dispatch_contract && cargo test -p auv-tracing --features memory-store --test context_contract && cargo test -p auv-tracing --features memory-store --test reducer_contract`

Expected: PASS.

- [ ] **Step 6: Commit routing and flush**

```bash
git add crates/auv-tracing/src/lib.rs crates/auv-tracing/src/telemetry.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/context.rs crates/auv-tracing/tests/dispatch_contract.rs crates/auv-tracing/tests/support/mod.rs
git commit --only crates/auv-tracing/src/lib.rs crates/auv-tracing/src/telemetry.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/context.rs crates/auv-tracing/tests/dispatch_contract.rs crates/auv-tracing/tests/support/mod.rs -m "feat(auv-tracing): route facts through flush barriers"
```

### Task 9: Implement Detached Artifact Emission

**Files:**
- Modify: `crates/auv-tracing/src/artifact.rs`
- Modify: `crates/auv-tracing/src/dispatch.rs`
- Modify: `crates/auv-tracing/src/macros.rs`
- Modify: `crates/auv-tracing/tests/support/mod.rs`
- Create: `crates/auv-tracing/tests/artifact_emission_contract.rs`

- [ ] **Step 1: Add failing body-polling, receipt-drop, and ordering tests**

```rust
#[test]
fn disabled_artifact_does_not_poll_body() {
  let polled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let artifact = test_artifact(ProbeReader::new(polled.clone()));
  let result = futures_executor::block_on(auv_tracing::emit_artifact(artifact)).unwrap();
  assert!(result.is_none());
  assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn telemetry_only_artifact_does_not_poll_body() {
  let fixture = TestDispatch::telemetry_only();
  let polled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let result = fixture.root().in_scope(|| {
    futures_executor::block_on(auv_tracing::emit_artifact(
      test_artifact(ProbeReader::new(polled.clone())),
    ))
  }).unwrap();
  assert!(result.is_none());
  assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn slow_artifact_does_not_block_span_end() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let gate = ReadGate::new();
  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    let receipt = span.in_scope(|| auv_tracing::emit_artifact(test_artifact(gate.reader())));
    drop(span);
    drop(receipt);
  });
  gate.wait_until_polled();
  fixture.wait_for_span_end();
  assert_eq!(fixture.artifact_count(), 0);
  gate.release();
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.artifact_count(), 1);
}

#[test]
fn slow_artifact_and_later_facts_project_only_in_authority_revision_order() {
  let fixture = TestDispatch::memory_with_projector();
  let gate = ReadGate::new();
  let root = fixture.root();
  root.in_scope(|| {
    let span = auv_tracing::start_span!(TestSpan);
    let receipt = span.in_scope(|| auv_tracing::emit_artifact(test_artifact(gate.reader())));
    span.in_scope(|| auv_tracing::emit_event!(TestEvent { value: 7 }));
    drop(span);
    drop(receipt);
  });
  fixture.wait_for_authority_revision(3);
  assert_eq!(fixture.projected_fact_kinds(), ["span_started", "event_occurred", "span_ended"]);
  gate.release();
  futures_executor::block_on(fixture.dispatch.flush()).unwrap();
  assert_eq!(fixture.projected_revisions(), [1, 2, 3, 4]);
  assert_eq!(fixture.projected_fact_kinds(), ["span_started", "event_occurred", "span_ended", "artifact_published"]);
}
```

- [ ] **Step 2: Run the artifact emission tests**

Run: `cargo test -p auv-tracing --features memory-store --test artifact_emission_contract`

Expected: FAIL because `NewArtifact` and `ArtifactEmission` are incomplete.

- [ ] **Step 3: Implement the caller-owned request and receipt future**

```rust
pub struct NewArtifact<R> {
  artifact_id: ArtifactId,
  idempotency_key: IdempotencyKey,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: Attributes,
  body: R,
}

pub struct ArtifactEmission {
  receipt: ArtifactReceipt,
}

enum ArtifactReceipt {
  Disabled,
  Pending(futures_channel::oneshot::Receiver<Result<ArtifactMetadata, ArtifactWriteError>>),
}

pub fn emit_artifact<R>(artifact: NewArtifact<R>) -> ArtifactEmission
where
  R: futures_io::AsyncRead + Unpin + Send + 'static;

impl std::future::Future for ArtifactEmission {
  type Output = Result<Option<ArtifactMetadata>, ArtifactWriteError>;
  fn poll(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Self::Output>;
}
```

`NewArtifact::new` generates the artifact ID and idempotency key. `emit_artifact` synchronously captures detached authority/run/span identity plus the preceding fact fence and submits the job before returning. The detached token must not retain the span lifecycle `Arc`.

- [ ] **Step 4: Implement the artifact worker lane**

When no authority is available, return a ready `Ok(None)` receipt without polling the body. With an authority, wait for the prerequisite fact fence, start `RunStore::write_artifact` on the artifact lane, and mark the job terminal only after commit, confirmed pre-publication failure, or idempotency lookup resolves an unknown publication. The artifact transfer must not occupy the synchronous fact FIFO. Dropping the receipt does not cancel the job; an unobserved failure is sent to `DispatchErrorReporter`.

Add the macro:

```rust
#[macro_export]
macro_rules! emit_artifact {
  ($artifact:expr) => { $crate::emit_artifact($artifact) };
}
```

- [ ] **Step 5: Run artifact and flush tests**

Run: `cargo test -p auv-tracing --features memory-store --test artifact_emission_contract && cargo test -p auv-tracing --features memory-store --test dispatch_contract`

Expected: PASS.

- [ ] **Step 6: Commit artifact emission**

```bash
git add crates/auv-tracing/src/artifact.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/macros.rs crates/auv-tracing/tests/artifact_emission_contract.rs crates/auv-tracing/tests/support/mod.rs
git commit --only crates/auv-tracing/src/artifact.rs crates/auv-tracing/src/dispatch.rs crates/auv-tracing/src/macros.rs crates/auv-tracing/tests/artifact_emission_contract.rs crates/auv-tracing/tests/support/mod.rs -m "feat(auv-tracing): emit detached artifacts"
```

### Task 10: Implement Crash-Durable `FileRunStore`

**Files:**
- Modify: `Cargo.lock`
- Modify: `crates/auv-tracing/Cargo.toml`
- Create: `crates/auv-tracing/src/store/file.rs`
- Modify: `crates/auv-tracing/src/store.rs`
- Modify: `crates/auv-tracing-conformance/Cargo.toml`
- Modify: `crates/auv-tracing-conformance/src/lib.rs`
- Create: `crates/auv-tracing-conformance/src/bin/file_store_child.rs`
- Create: `crates/auv-tracing-conformance/tests/file.rs`
- Create: `crates/auv-tracing/tests/file_store_recovery.rs`

- [ ] **Step 1: Run the shared conformance harness against a missing file store**

```rust
#[test]
fn file_store_satisfies_authority_contract() {
  futures_executor::block_on(assert_store_contract(|| {
    let root = tempfile::tempdir().unwrap().keep();
    std::sync::Arc::new(auv_tracing::FileRunStore::open(root).unwrap())
  }));
}
```

Add `tempfile.workspace = true` under `[dev-dependencies]` in both
`auv-tracing` and `auv-tracing-conformance`. The latter package also declares
the `file_store_child` binary used below; it is test infrastructure and is not
exported by `auv-tracing`.

```toml
[[bin]]
name = "file_store_child"
path = "src/bin/file_store_child.rs"
```

Run: `cargo test -p auv-tracing-conformance --test file`

Expected: FAIL because `FileRunStore` is not defined.

- [ ] **Step 2: Implement the private versioned layout and stable authority**

Use only store-derived paths:

```text
{root}/authority.json
{root}/authority.lock
{root}/runs/{run_id}/commits.log
{root}/runs/{run_id}/commit.lock
{root}/blobs/sha256/{first_two_hex}/{sha256}
{root}/tmp/{store_generated_name}
```

Each log frame contains a fixed magic/version, encoded payload length, SHA-256 of the payload, and one `RunCommit` JSON payload. Persist `authority.json` by write/fdatasync/rename/directory-fsync and reuse it after restart. Serialize each run under an advisory lock. Reject any symlink encountered below the configured root.
Create the stable authority under `authority.lock`; after taking that lock,
reread `authority.json` before deciding to generate one. This is required for
two processes racing on the first `open(root)`.
Add the feature-gated `store::file` module and `FileRunStore` export from
`store.rs` in this task.

- [ ] **Step 3: Add failing recovery and path-hardening tests**

```rust
#[test]
fn truncated_final_frame_is_discarded_but_prior_corruption_fails() {
  let fixture = FileFixture::with_two_commits();
  fixture.truncate_last_frame();
  assert_eq!(fixture.reopen().unwrap().load().unwrap().through_revision().get(), 1);
  fixture.corrupt_first_frame();
  assert!(matches!(fixture.reopen().unwrap().load(), Err(ReadError::Integrity(_))));
}

#[test]
fn artifact_publication_never_uses_caller_paths() {
  let fixture = FileFixture::new();
  fixture.install_symlink_escape();
  assert!(matches!(fixture.write_artifact(), Err(ArtifactWriteError::Rejected(_))));
  assert!(!fixture.outside_file_exists());
}

#[test]
fn two_open_store_instances_refresh_indexes_under_the_run_lock() {
  let root = tempfile::tempdir().unwrap();
  let first = FileRunStore::open(root.path()).unwrap();
  let second = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let one = futures_executor::block_on(first.commit(event_request(run_id, 1))).unwrap();
  let two = futures_executor::block_on(second.commit(event_request(run_id, 2))).unwrap();
  assert_eq!(one.revision().get(), 1);
  assert_eq!(two.revision().get(), 2);
  let snapshot = futures_executor::block_on(first.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.events().len(), 2);
}
```

In `crates/auv-tracing-conformance/tests/file.rs`, add process-level tests that
run `env!("CARGO_BIN_EXE_file_store_child")`. The helper accepts only these
test protocols and prints one strict JSON result to stdout:

```text
authority <root> <ready-file> <go-file>
commit-event <root> <run-id> <event-id> <idempotency-key> <value> <ready-file> <go-file>
write-artifact <root> <run-id> <span-id-or-none> <artifact-id> <idempotency-key> <body-file> <ready-file> <go-file>
```

The parent waits for both ready files before atomically creating the go file.
Required assertions are:

| Test | Required assertion |
|---|---|
| `concurrent_first_open_chooses_one_authority` | two fresh child processes report the same non-nil `AuthorityId`, and the persisted file decodes to that ID |
| `concurrent_process_commits_allocate_contiguous_revisions` | two distinct event requests to one run return revisions `{1, 2}` and a reopened snapshot contains both events |
| `concurrent_equal_idempotency_replay_appends_once` | the same request/key in two processes returns the same revision and the log contains one commit |
| `concurrent_artifact_id_conflict_keeps_one_blob_and_one_fact` | different bodies for one `ArtifactId` produce one publication and one typed conflict; the winner's digest, bytes, and single fact agree |
| `point_reads_refresh_after_another_process_writes` | an already-open instance observes the other process through `lookup_commit`, `load_snapshot`, `commits_after`, and `open_artifact` without reopening |
| `subscription_observes_another_instance_commit` | a subscription opened from one store instance receives the next revision committed by another process without reopening |

Use child processes, not only threads, because the contract relies on OS file
locks and durable on-disk refresh. Give every wait a bounded timeout and kill
children on timeout so a broken lock implementation cannot hang the suite.

- [ ] **Step 4: Implement durable artifact publication and restart indexes**

Stream into a private temp file while counting and hashing. After validation, fsync the temp file, atomically rename to the content-addressed blob path, fsync the blob directory, and only then append/fsync the artifact commit. A crash may leave an unreferenced blob eligible for later garbage collection, but never metadata that points to absent bytes.

Treat indexes loaded by `FileRunStore::open` as caches, not authority. For every
commit and artifact publication, acquire the per-run file lock, reread and
verify the log tail from the last locally known offset, rebuild the run-local
revision/idempotency/event-ID/artifact-ID/snapshot state, then validate and
append. This lock-and-refresh sequence is required even when two
`FileRunStore` values in one process share the same root; neither instance may
allocate a revision from stale memory.
If refresh finds a recoverable partial final frame, truncate the log to the
last verified frame boundary and `fdatasync` it while still holding the run
lock before appending. Never append after ignored trailing bytes. Corruption in
or before a complete frame remains `ReadError::Integrity` and is not truncated.

Point reads also treat the log as authority. `lookup_commit`, `load_snapshot`,
and `commits_after` take the run lock, refresh and verify all newly complete
frames, update the local cache, and only then answer. `open_artifact` parses the
URI, refreshes its owning run the same way, resolves committed metadata, and
then opens the derived blob. A read ignores a partial final frame but does not
truncate it under a shared/read path; only a writer performs the locked repair
before append. No point-read result may depend solely on indexes captured at
`FileRunStore::open`.

File subscriptions cannot rely only on process-local commit notifications.
After the first subscription, lazily start one standard-library watcher thread
per `FileRunStore`; it tracks subscribed commit-log lengths/metadata and wakes
their `futures_util::task::AtomicWaker` when an external writer changes a log.
Each awakened stream reacquires the run lock and parses verified frames from
its cursor before yielding. Use a private 25 ms check interval with a
`NOTICE(file-store-subscription-poll)` explaining that stable Rust has no
portable filesystem notification API; stop the watcher when the store and all
subscriptions are gone. The integration test uses a five-second timeout, so
the interval is observable only as delivery latency and never as a correctness
or retention boundary.

- [ ] **Step 5: Run file conformance and recovery tests**

Run: `cargo test -p auv-tracing-conformance --test file && cargo test -p auv-tracing --features file-store --test file_store_recovery`

Expected: PASS.

- [ ] **Step 6: Commit the file store**

```bash
git add Cargo.lock crates/auv-tracing/Cargo.toml crates/auv-tracing/src/store.rs crates/auv-tracing/src/store/file.rs crates/auv-tracing/tests/file_store_recovery.rs crates/auv-tracing-conformance/Cargo.toml crates/auv-tracing-conformance/src/lib.rs crates/auv-tracing-conformance/src/bin/file_store_child.rs crates/auv-tracing-conformance/tests/file.rs
git commit --only Cargo.lock crates/auv-tracing/Cargo.toml crates/auv-tracing/src/store.rs crates/auv-tracing/src/store/file.rs crates/auv-tracing/tests/file_store_recovery.rs crates/auv-tracing-conformance/Cargo.toml crates/auv-tracing-conformance/src/lib.rs crates/auv-tracing-conformance/src/bin/file_store_child.rs crates/auv-tracing-conformance/tests/file.rs -m "feat(auv-tracing): add durable file run store"
```

### Task 11: Atomically Migrate Inspect Run Authority, Read Projection, And Viewer

**Files:**
- Create: `crates/auv-tracing-inspect/Cargo.toml`
- Create: `crates/auv-tracing-inspect/src/lib.rs`
- Create: `crates/auv-tracing-inspect/src/protocol.rs`
- Modify: `Cargo.toml`
- Modify: `crates/auv-inspect-server/Cargo.toml`
- Create: `crates/auv-inspect-server/src/run_api.rs`
- Modify: `crates/auv-inspect-server/src/server.rs`
- Modify: `crates/auv-inspect-server/src/lib.rs`
- Modify: `crates/auv-inspect-model/Cargo.toml`
- Rewrite: `crates/auv-inspect-model/src/lib.rs`
- Rewrite: `crates/auv-inspect-server/src/read_projection.rs`
- Modify: `crates/auv-inspect-server/src/session.rs`
- Rewrite: `crates/auv-inspect-server/viewer/src/viewer.ts`
- Modify: `crates/auv-inspect-server/viewer/src/App.vue`
- Create: `crates/auv-inspect-server/tests/run_api_contract.rs`
- Create: `crates/auv-inspect-server/tests/viewer_projection_contract.rs`

- [ ] **Step 1: Add failing HTTP contract tests**

Test `GET /v1/authority`, new/equal/conflicting commits, idempotency lookup, missing/present snapshot, paging, authority mismatch, history gap, cursor ahead, and SSE reconnect. One exact error assertion is:

```rust
assert_eq!(response.status(), axum::http::StatusCode::GONE);
assert_eq!(
  json_body(response).await,
  serde_json::json!({"history_gap":{"requested_after":4,"earliest_available":9}}),
);
```

Also POST one body with an unknown top-level field and one raw body containing
the same `authority_id` key twice. Both return 400 before `RunStore::commit` is
called.
POST a valid body one byte above 32 MiB and assert 413 before strict decoding or
store access. Install Axum's body limit on run JSON routes only; binary artifact
PUT remains streaming and uses the artifact length boundary.

Add this read-side race assertion in `viewer_projection_contract.rs` before
changing server state:

```rust
#[tokio::test]
async fn snapshot_then_subscription_does_not_drop_intervening_commit() {
  let authority = TestAuthority::new();
  authority.commit_span_start().await;
  let initial = authority.snapshot().await;
  authority.commit_event().await;
  let updates = authority.subscribe(initial.through_revision()).await;
  assert_eq!(updates.first().unwrap().revision().get(), initial.through_revision().get() + 1);
}
```

Run: `cargo test -p auv-inspect-server --test run_api_contract --test viewer_projection_contract`

Expected: FAIL with 404 for `/v1/authority`.

- [ ] **Step 2: Define shared protocol DTOs without HTTP dependencies in core**

```rust
pub const RUN_MEDIA_TYPE: &str = "application/vnd.auv.run+json; version=1";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommitBody {
  pub authority_id: auv_tracing::AuthorityId,
  pub mutations: auv_tracing::NonEmptyVec<auv_tracing::RunMutation>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunApiError {
  NotFound,
  Forbidden,
  InvalidReference { code: auv_tracing::ErrorCode },
  AuthorityMismatch { expected: auv_tracing::AuthorityId, received: auv_tracing::AuthorityId },
  IdempotencyMismatch,
  Rejected { code: auv_tracing::ErrorCode },
  HistoryGap { requested_after: auv_tracing::RunRevision, earliest_available: auv_tracing::RunRevision },
  CursorAhead { requested_after: auv_tracing::RunRevision, latest: auv_tracing::RunRevision },
  Integrity { code: auv_tracing::ErrorCode },
  Unavailable { code: auv_tracing::ErrorCode },
}
```

Put request/response path-body DTOs in `auv-tracing-inspect::protocol`; at this
commit `lib.rs` exports only `protocol`, not the Task 12 client modules. Do not
add HTTP concepts to `auv-tracing`. Every protocol struct and enum variant
payload denies unknown fields. Before Serde decoding, run the same
duplicate-key-preserving JSON validation used by `JsonPayload`; HTTP adapters
must not parse into `serde_json::Value` first.

- [ ] **Step 3: Replace legacy write routes with `Arc<dyn RunStore>` authority routes**

Implement:

```text
GET  /v1/authority
POST /v1/runs/{run_id}/commits
GET  /v1/runs/{run_id}/commits/by-idempotency-key/{key}
GET  /v1/runs/{run_id}/snapshot
GET  /v1/runs/{run_id}/commits?after_revision={revision}&limit={limit}
GET  /v1/runs/{run_id}/commits/stream?after_revision={revision}
```

Require `Idempotency-Key` for POST, take `run_id` only from the path, and accept only `authority_id` plus externally tagged mutations in the body. Return 201 for new commits and 200 for equal replay. Map all `RunApiError` variants to the status codes in the spec.

- [ ] **Step 4: Implement SSE cursor and gap behavior**

Use revision as SSE `id`, event name `commit`, and one serialized `RunCommit` as `data`. Select the greater valid cursor from query and `Last-Event-ID`. On `SubscriptionError::Gap`, emit a `gap` event with requested/earliest revisions and close. Snapshot/page reads remain the recovery authority.

- [ ] **Step 5: Replace the Inspect document and viewer in the same server change**

```rust
pub struct InspectDocument {
  pub authority_id: AuthorityId,
  pub run_id: RunId,
  pub through_revision: RunRevision,
  pub spans: Vec<InspectSpan>,
  pub events: Vec<InspectEvent>,
  pub artifacts: Vec<InspectArtifact>,
}
```

Build this disposable DTO only from `RunSnapshot`. Preserve typed event schema
and payload for inspection. Artifacts expose canonical URI and committed
metadata; they never expose filesystem paths, preferred filenames, role,
summary, status, result, verification aggregate, or a trace ID. The browser
fetches the snapshot, records `through_revision`, and then opens
`/v1/runs/{run_id}/commits/stream?after_revision={through_revision}`. It applies
commits in revision order and reloads the snapshot after a `gap` event or an
unrecoverable reconnect. An unclosed span renders as `open`; the viewer does not
infer running, success, failure, cancellation, or semantic verification.

- [ ] **Step 6: Run server and viewer protocol tests**

Run: `cargo test -p auv-inspect-model && cargo test -p auv-inspect-server --test run_api_contract --test viewer_projection_contract && npm --prefix crates/auv-inspect-server/viewer run smoke && npm --prefix crates/auv-inspect-server/viewer run build`

Expected: PASS.

- [ ] **Step 7: Commit the atomic Inspect migration**

```bash
git add Cargo.toml Cargo.lock crates/auv-tracing-inspect crates/auv-inspect-model crates/auv-inspect-server
git commit --only Cargo.toml Cargo.lock crates/auv-tracing-inspect crates/auv-inspect-model crates/auv-inspect-server -m "refactor(auv-inspect-server): adopt the run authority contract"
```

### Task 12: Implement Inspect Binary Upload/Read And The Complete `InspectRunStore`

**Files:**
- Create: `crates/auv-tracing-inspect/src/client.rs`
- Create: `crates/auv-tracing-inspect/src/task_spawner.rs`
- Modify: `crates/auv-tracing-inspect/src/protocol.rs`
- Modify: `crates/auv-tracing-inspect/src/lib.rs`
- Modify: `crates/auv-tracing-inspect/Cargo.toml`
- Create: `crates/auv-tracing-inspect/tests/client_contract.rs`
- Create: `crates/auv-inspect-server/src/artifact_api.rs`
- Modify: `crates/auv-inspect-server/src/server.rs`
- Modify: `crates/auv-inspect-server/Cargo.toml`
- Create: `crates/auv-inspect-server/tests/artifact_api_contract.rs`

- [ ] **Step 1: Add a failing connect/read/write/SSE client test**

```rust
#[tokio::test]
async fn connect_fetches_authority_before_store_installation() {
  let server = TestInspectAuthority::start().await;
  let store = auv_tracing_inspect::InspectRunStore::connect(server.base_url()).await.unwrap();
  assert_eq!(store.authority_id(), server.authority_id());
  let spawner = auv_tracing_inspect::TokioTaskSpawner::current().unwrap();
  let dispatch = auv_tracing::configure()
    .task_spawner(std::sync::Arc::new(spawner))
    .run_store(std::sync::Arc::new(store.clone()))
    .build()
    .unwrap();
  let commit = store.commit(server.sample_request()).await.unwrap();
  assert_eq!(commit.authority_id(), server.authority_id());
  dispatch.flush().await.unwrap();
}
```

Also assert typed reconstruction of 404, 409 cursor-ahead, 410 history-gap, 422 rejection, 500 integrity, and 503 unavailable responses.
Serve one valid snapshot larger than 32 MiB and assert
`InspectRunStore::load_snapshot` returns it instead of applying the bounded
commit-page limit.

In `artifact_api_contract.rs`, cover new/equal/conflicting draft creation,
24-hour expiry, unknown span, streamed body overflow, digest mismatch, length
mismatch, successful publication, repeated PUT without body polling,
response-loss lookup, original content headers, and authority mismatch before
body polling. Also cover available/not-found partial batch resolution,
duplicate URI positions, the 256-item limit, malformed URI rejection, and
authority mismatch before lookup.

- [ ] **Step 2: Run the client contract**

Run: `cargo test -p auv-tracing-inspect --test client_contract && cargo test -p auv-inspect-server --test artifact_api_contract`

Expected: FAIL because `InspectRunStore` is absent.

- [ ] **Step 3: Define upload DTOs and implement streaming server routes**

```rust
pub struct ArtifactUploadDraftRequest {
  pub authority_id: AuthorityId,
  pub artifact_id: ArtifactId,
  pub span_id: Option<SpanId>,
  pub purpose: ArtifactPurpose,
  pub content_type: ContentType,
  pub byte_length: ByteLength,
  pub sha256: Sha256Digest,
  pub attributes: Attributes,
}

pub struct ArtifactUploadId(uuid::Uuid);

pub struct ArtifactUploadDraft {
  pub upload_id: ArtifactUploadId,
  pub artifact_uri: ArtifactUri,
  pub expires_at: Timestamp,
}

pub struct ResolveArtifactsRequest {
  pub authority_id: AuthorityId,
  pub uris: Vec<ArtifactUri>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedArtifact {
  Available { uri: ArtifactUri, content_type: ContentType, byte_length: ByteLength, sha256: Sha256Digest, content_url: url::Url },
  NotFound { uri: ArtifactUri },
}
```

Implement `POST /v1/runs/{run_id}/artifact-uploads`, `PUT
/v1/runs/{run_id}/artifact-uploads/{upload_id}/content`, and `GET
/v1/runs/{run_id}/artifacts/{artifact_id}`, plus `POST
/v1/resources/artifacts/resolve`. Drafts are temporary server
resources. Require matching RFC 9530 `Content-Digest`, content type, length,
digest, authority, and existing optional span. Stream Axum body chunks through
`tokio_util::io::StreamReader` and `TokioAsyncReadCompatExt` into
`RunStore::write_artifact`; never collect the body. Return the complete
`RunCommit` after publication and exact content headers on read.

Guard the draft indexes by `(run_id, idempotency_key)` and derived
`ArtifactUri` in one server-side critical section. An equal replay returns the
same draft; a different request under either identity returns 409. Before
creating a draft, call `RunStore::lookup_commit`: an already published equal
artifact is resolved from its commit, while a key already used by an ordinary
commit or different artifact returns 409. Expiry removes only unpublished
draft state; publication identity remains authoritative in `RunStore`.

The resolver validates the full batch and authority before lookup, preserves
order and duplicates, and returns one externally tagged result per URI.
Available results contain an absolute credential-free HTTP(S) content URL;
that URL is never stored in canonical metadata.

Use these transport dependencies in `auv-tracing-inspect`:

```toml
auv-tracing = { path = "../auv-tracing" }
base64 = "0.22"
futures-util = { workspace = true, features = ["io"] }
reqwest = { workspace = true, features = ["stream"] }
tokio.workspace = true
tokio-util = { workspace = true, features = ["io", "compat"] }
url = { version = "2", features = ["serde"] }
uuid.workspace = true
```

Add `base64 = "0.22"` and
`tokio-util = { workspace = true, features = ["io", "compat"] }` to
`auv-inspect-server`. Keep 24-hour expiry behind a private server `Clock` used
by router tests; it is not a canonical field.

- [ ] **Step 4: Implement the complete authority client**

```rust
pub struct InspectRunStore {
  authority_id: auv_tracing::AuthorityId,
  base_url: url::Url,
  client: reqwest::Client,
}

impl InspectRunStore {
  pub async fn connect(base_url: url::Url) -> Result<Self, ConnectError>;
}

pub struct TokioTaskSpawner {
  handle: tokio::runtime::Handle,
}

impl TokioTaskSpawner {
  pub fn current() -> Result<Self, NoCurrentRuntime>;
}

impl auv_tracing::TaskSpawner for TokioTaskSpawner {
  fn spawn(&self, task: auv_tracing::DispatchTask) -> Result<(), auv_tracing::TaskSpawnError>;
}
```

`connect` fetches and caches `/v1/authority`. Implement every `RunStore` method
with the exact V1 media type and typed errors. `write_artifact` creates/replays a
draft, streams its one-shot futures-IO body through
`FuturesAsyncReadCompatExt` and `ReaderStream`, and returns the publication
commit. On an unknown transport outcome, call `lookup_commit`; return
`PublicationUnknown` only when lookup cannot resolve it. Never resend a
consumed body or repeat application work. For SSE, parse `id`, `event`, and
multi-line `data`; reject malformed revisions and reconnect only from the last
accepted commit. Do not infer gaps from empty pages or disconnected streams.
Bound commit-page, error, draft, resolver, and one assembled SSE-event JSON
response to 32 MiB before `decode_strict`. `load_snapshot` is the deliberate
full-materialization API and accepts a valid larger snapshot; do not reuse the
page limit there. Stream artifact GET bytes separately and enforce their
committed length/digest while yielding them.
Implement the resolver as an Inspect-specific client method; it is not added to
the generic `RunStore` trait.
Add `client` and `task_spawner` modules plus their public exports to
`auv-tracing-inspect/src/lib.rs` in this task.

- [ ] **Step 5: Run client and server tests**

Run: `cargo test -p auv-tracing-inspect --test client_contract && cargo test -p auv-inspect-server --test run_api_contract --test artifact_api_contract`

Expected: PASS.

- [ ] **Step 6: Commit upload/read and the complete Inspect store client**

```bash
git add crates/auv-tracing-inspect crates/auv-inspect-server/src/artifact_api.rs crates/auv-inspect-server/src/server.rs crates/auv-inspect-server/tests/artifact_api_contract.rs crates/auv-inspect-server/Cargo.toml Cargo.lock
git commit --only crates/auv-tracing-inspect crates/auv-inspect-server/src/artifact_api.rs crates/auv-inspect-server/src/server.rs crates/auv-inspect-server/tests/artifact_api_contract.rs crates/auv-inspect-server/Cargo.toml Cargo.lock -m "feat(auv-tracing-inspect): add binary run store client"
```

### Task 13: Validate Strict Inspect Wire Shapes And Remote Store Conformance

**Files:**
- Modify: `crates/auv-tracing-inspect/src/client.rs`
- Modify: `crates/auv-tracing-inspect/src/protocol.rs`
- Modify: `crates/auv-tracing-inspect/tests/client_contract.rs`
- Modify: `crates/auv-inspect-server/src/run_api.rs`
- Modify: `crates/auv-inspect-server/src/artifact_api.rs`
- Modify: `crates/auv-inspect-server/tests/run_api_contract.rs`
- Modify: `crates/auv-inspect-server/tests/artifact_api_contract.rs`
- Modify: `crates/auv-tracing-conformance/Cargo.toml`
- Create: `crates/auv-tracing-conformance/tests/inspect.rs`

- [ ] **Step 1: Add failing strict-decoder tests at every Inspect JSON boundary**

```rust
#[test]
fn strict_json_rejects_duplicate_and_unknown_fields_recursively() {
  assert!(decode_strict::<RunCommitBody>(&duplicate_top_level_authority()).is_err());
  assert!(decode_strict::<RunCommit>(&duplicate_nested_timestamp_seconds()).is_err());
  assert!(decode_strict::<RunCommitBody>(&unknown_commit_request_field()).is_err());
  assert!(decode_strict::<RunMutation>(&unknown_span_started_variant_field()).is_err());
  assert!(decode_strict::<ArtifactUploadDraftRequest>(&unknown_upload_draft_field()).is_err());
  assert!(decode_strict::<ResolvedArtifact>(&unknown_resolved_artifact_variant_field()).is_err());
}
```

Endpoint tests send each malformed shape to its run or artifact route, assert
HTTP 400, and assert the backing `RunStore` probe received no call. SSE/client
tests reject duplicate keys and unknown fields in received `RunCommit` and
`RunApiError` payloads instead of accepting a lenient subset.

Run: `cargo test -p auv-tracing-inspect --test client_contract && cargo test -p auv-inspect-server --test run_api_contract --test artifact_api_contract`

Expected: FAIL because protocol decoding is still performed separately by
route/client call sites.

- [ ] **Step 2: Implement one concrete strict protocol decoder**

```rust
pub fn decode_strict<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolDecodeError> {
  reject_duplicate_keys_recursively(bytes)?;
  serde_json::from_slice(bytes).map_err(ProtocolDecodeError::invalid_json)
}
```

`reject_duplicate_keys_recursively` is an Inspect-protocol-private structured
Serde visitor using the same recursive algorithm and exact-number policy as
Task 2. It is not a public helper in `auv-tracing`, because Inspect protocol
body limits differ from `JsonPayload` limits. Every Inspect JSON request,
response, error, and SSE
`data` payload uses `decode_strict`; no call site first converts to
`serde_json::Value`. All protocol structs and variant payload structs retain
`#[serde(deny_unknown_fields)]`.

- [ ] **Step 3: Run the complete store contract through the real HTTP boundary**

Add `auv-tracing-inspect` and the real Axum router as development dependencies
of `auv-tracing-conformance`. Run `assert_store_contract` against
`InspectRunStore`, then `assert_gap_contract` against a test server whose
explicit retention hook advances `earliest_available`. The suite exercises run
commits, binary write/read, interrupted bodies, idempotency lookup, snapshots,
pages, SSE, and gaps through HTTP rather than substituting an in-process store.

Run: `cargo test -p auv-tracing-conformance --test inspect`

Expected: PASS.

- [ ] **Step 4: Run all Inspect contract tests**

Run: `cargo test -p auv-inspect-server --test run_api_contract --test artifact_api_contract --test viewer_projection_contract && cargo test -p auv-tracing-inspect --test client_contract && cargo test -p auv-tracing-conformance --test inspect`

Expected: PASS.

- [ ] **Step 5: Commit strict decoding and remote conformance**

```bash
git add crates/auv-tracing-inspect/src/client.rs crates/auv-tracing-inspect/src/protocol.rs crates/auv-tracing-inspect/tests/client_contract.rs crates/auv-inspect-server/src/run_api.rs crates/auv-inspect-server/src/artifact_api.rs crates/auv-inspect-server/tests/run_api_contract.rs crates/auv-inspect-server/tests/artifact_api_contract.rs crates/auv-tracing-conformance/Cargo.toml crates/auv-tracing-conformance/tests/inspect.rs Cargo.lock
git commit --only crates/auv-tracing-inspect/src/client.rs crates/auv-tracing-inspect/src/protocol.rs crates/auv-tracing-inspect/tests/client_contract.rs crates/auv-inspect-server/src/run_api.rs crates/auv-inspect-server/src/artifact_api.rs crates/auv-inspect-server/tests/run_api_contract.rs crates/auv-inspect-server/tests/artifact_api_contract.rs crates/auv-tracing-conformance/Cargo.toml crates/auv-tracing-conformance/tests/inspect.rs Cargo.lock -m "test(auv-tracing-inspect): enforce remote store contract"
```

### Task 14: Implement Rust `tracing` And OpenTelemetry Projectors

**Files:**
- Modify: `crates/auv-tracing/src/lib.rs`
- Create: `crates/auv-tracing/src/rust_tracing.rs`
- Create: `crates/auv-tracing/tests/rust_tracing_contract.rs`
- Create: `crates/auv-tracing-otel/Cargo.toml`
- Create: `crates/auv-tracing-otel/src/lib.rs`
- Create: `crates/auv-tracing-otel/tests/projection_contract.rs`
- Create: `crates/auv-tracing-otel/tests/support/mod.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add failing Rust tracing vocabulary tests**

Capture emitted callsites with a test subscriber and assert that only fixed fields appear:

```rust
const ALLOWED_FIELDS: &[&str] = &[
  "auv.authority.id", "auv.run.id", "auv.run.revision", "auv.span.id",
  "auv.span.name", "auv.span.parent_id", "auv.span.remote_id",
  "auv.span.start_revision", "auv.span.end_revision", "auv.event.id",
  "auv.event.schema.name", "auv.event.schema.version",
  "auv.artifact.uri", "auv.artifact.purpose",
  "auv.artifact.content_type", "auv.artifact.byte_length",
  "auv.artifact.sha256",
];
```

Assert that event payload JSON and artifact bytes are absent.

- [ ] **Step 2: Implement `RustTracingProjector` behind the feature**

Use fixed static callsites for span start/end, event, and artifact metadata.
Register this projector with `TelemetryRoutePolicy::fixed_fields_only()`; the
dispatch router has already removed producer attributes before `project` is
called. Never create dynamic field names or forward canonical event JSON. Add
the feature-gated `rust_tracing` module and export to `lib.rs` in this task.

Run: `cargo test -p auv-tracing --features rust-tracing --test rust_tracing_contract`

Expected: PASS after implementation.

- [ ] **Step 3: Add failing in-memory OTEL exporter tests**

Assert independent OTEL trace/span IDs, `Status::Unset`, AUV correlation attributes, separate span start/end revisions, event/artifact single revisions, event-form logs for run events and artifacts, allowlisted attributes only, and no binary or full event JSON fields.

Use OpenTelemetry 0.32 with `trace` and `logs` enabled. The test support module
implements bounded in-memory `SpanExporter` and `LogExporter` collectors and
asserts exported SDK data, not `OtelProjector` private state.

Run: `cargo test -p auv-tracing-otel --test projection_contract`

Expected: FAIL because `auv-tracing-otel` is absent.

- [ ] **Step 4: Implement `OtelProjector` against an application-supplied provider**

```rust
pub struct OtelProjector {
  inner: std::sync::Arc<OtelProjectorInner>,
}

impl OtelProjector {
  pub fn new(
    tracer_provider: opentelemetry_sdk::trace::SdkTracerProvider,
    logger_provider: opentelemetry_sdk::logs::SdkLoggerProvider,
  ) -> Self;
}
```

Create independent OTEL root spans for AUV roots and child OTEL spans for local parentage. Remote AUV links remain AUV correlation attributes unless the application separately propagates W3C context. Map AUV span end without status. Emit run-scoped events and artifact metadata as OTEL logs. The application chooses `TelemetryRoutePolicy` when registering this projector; OTEL cannot expand that policy later. `flush` delegates to the supplied SDK providers; this crate never configures an OTLP exporter.

- [ ] **Step 5: Run both projector suites**

Run: `cargo test -p auv-tracing --features rust-tracing --test rust_tracing_contract && cargo test -p auv-tracing-otel --test projection_contract`

Expected: PASS.

- [ ] **Step 6: Commit projectors**

```bash
git add Cargo.toml Cargo.lock crates/auv-tracing/src/lib.rs crates/auv-tracing/src/rust_tracing.rs crates/auv-tracing/tests/rust_tracing_contract.rs crates/auv-tracing-otel
git commit --only Cargo.toml Cargo.lock crates/auv-tracing/src/lib.rs crates/auv-tracing/src/rust_tracing.rs crates/auv-tracing/tests/rust_tracing_contract.rs crates/auv-tracing-otel -m "feat(auv-tracing): add bounded telemetry projectors"
```

### Task 15: Add Opt-In Instrumentation To Real Driver And App Operations

**Files:**
- Modify: `crates/auv-media-macos/Cargo.toml`
- Modify: `crates/auv-media-macos/src/lib.rs`
- Modify: `crates/auv-netease-music/Cargo.toml`
- Modify: `crates/auv-netease-music/src/invoke/select_proof.rs`
- Create: `crates/auv-netease-music/tests/tracing_opt_in.rs`

- [ ] **Step 1: Add a failing feature-isolation test**

```rust
#[test]
fn direct_result_is_identical_with_and_without_active_dispatch() {
  let fixture = hermetic_select_proof_fixture_dir();
  let without = build_select_result_from_fixture_dir(&fixture).unwrap();
  let tracing = TestTracing::memory();
  let with = tracing.root.in_scope(|| build_select_result_from_fixture_dir(&fixture)).unwrap();
  assert_eq!(with, without);
  futures_executor::block_on(tracing.dispatch.flush()).unwrap();
  assert_eq!(tracing.span_names(), ["auv.netease.playlist.select_proof"]);
}
```

Run: `cargo test -p auv-netease-music --features tracing --test tracing_opt_in`

Expected: FAIL because the feature and callsite do not exist.

- [ ] **Step 2: Add optional dependencies without default activation**

Add to both manifests:

```toml
[features]
default = []
tracing = ["dep:auv-tracing"]

[dependencies.auv-tracing]
path = "../auv-tracing"
optional = true
```

Merge with existing feature tables rather than replacing unrelated feature flags. Verify `cargo tree -p auv-media-macos --no-default-features` and `cargo tree -p auv-netease-music --no-default-features` contain no `auv-tracing`.
For the NetEase contract test only, add a development dependency on
`auv-tracing` with `memory-store` and on `futures-executor`; normal dependency
resolution remains optional and feature-gated.

- [ ] **Step 3: Instrument operations without changing dispatch ownership**

Under `cfg(feature = "tracing")`, start ordinary typed spans around `now_playing`, `send_command`, and the hermetic NetEase select proof. Emit small typed events only for concrete delivery/read facts. Keep the original `Result<T, E>` as the function return and install no global dispatch.

```rust
#[cfg(feature = "tracing")]
struct SelectProofSpan;

#[cfg(feature = "tracing")]
impl auv_tracing::SpanSpec for SelectProofSpan {
  const NAME: &'static str = "auv.netease.playlist.select_proof";
  fn attributes(&self) -> auv_tracing::Attributes { auv_tracing::Attributes::empty() }
}
```

Use a disabled local helper only to compile out the callsite when the feature is absent; do not introduce a driver context parameter or tracing dependency bag.

Instrument `auv-media-macos::{now_playing, send_command, seek}` with ordinary
operation spans named `auv.media.now_playing`, `auv.media.send_command`, and
`auv.media.seek`. `send_command` attaches the bounded scalar attribute
`auv.media.command`; `seek` attaches `auv.media.position_millis` after checked
conversion to the exact integer range. The span end records no delivery or
playback-success claim. Add unit assertions that the three functions return the
same `Result` with no dispatch and with a memory dispatch; only the latter
records spans.

- [ ] **Step 4: Run feature-on and feature-off tests**

Run: `cargo test -p auv-netease-music --no-default-features && cargo test -p auv-netease-music --features tracing --test tracing_opt_in && cargo test -p auv-media-macos --no-default-features && cargo test -p auv-media-macos --features tracing`

Expected: PASS.

- [ ] **Step 5: Commit producer instrumentation**

```bash
git add crates/auv-media-macos/Cargo.toml crates/auv-media-macos/src/lib.rs crates/auv-netease-music/Cargo.toml crates/auv-netease-music/src/invoke/select_proof.rs crates/auv-netease-music/tests/tracing_opt_in.rs Cargo.lock
git commit --only crates/auv-media-macos/Cargo.toml crates/auv-media-macos/src/lib.rs crates/auv-netease-music/Cargo.toml crates/auv-netease-music/src/invoke/select_proof.rs crates/auv-netease-music/tests/tracing_opt_in.rs Cargo.lock -m "feat: add opt-in auv tracing callsites"
```

### Task 16: Move Run Creation And Dispatch Setup To CLI/MCP Composition Roots

**Files:**
- Modify: `crates/auv-cli-invoke/Cargo.toml`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/artifact.rs`
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke/src/registry.rs`
- Modify: `crates/auv-cli-invoke/src/commands/display.rs`
- Modify: `crates/auv-cli-invoke/src/commands/input.rs`
- Modify: `crates/auv-cli-invoke/src/commands/scan.rs`
- Modify: `crates/auv-cli-invoke/src/commands/screen.rs`
- Modify: `crates/auv-cli-invoke/src/commands/window.rs`
- Modify: `crates/auv-cli-invoke/src/models/invoke_result.rs`
- Modify: `crates/auv-cli-invoke/src/render.rs`
- Modify: `crates/auv-cli-invoke-macros/src/lib.rs`
- Modify: `crates/auv-cli/Cargo.toml`
- Modify: `crates/auv-cli/src/cli_frontend.rs`
- Modify: `src/mcp.rs`
- Create: `crates/auv-cli/tests/support/instrumented_call.rs`
- Create: `crates/auv-cli/tests/run_recording_frontends.rs`

- [ ] **Step 1: Add a failing three-frontend composition test**

```rust
#[tokio::test]
async fn library_cli_and_mcp_share_work_but_not_a_runner() {
  let call = CountingCall::new();
  let library = call_as_library(&call).await.unwrap();
  let cli = call_as_cli(&call).await.unwrap();
  let mcp = call_as_mcp(&call).await.unwrap();
  assert_eq!((library, cli.value, mcp.value), (7, 7, 7));
  assert_eq!(call.call_count(), 3);
  assert_ne!(cli.run_id, mcp.run_id);
  assert_eq!(cli.stored_event_run_ids, [cli.run_id]);
  assert_eq!(mcp.stored_event_run_ids, [mcp.run_id]);
}
```

The test helper owns a typed function plus thin frontend adapters. It must not implement an `Operation` trait or use a central session/runner.
Make that typed function emit one test event synchronously while constructing
its returned future and another when the future is polled; the CLI and MCP
assertions require both events to carry their frontend-created run ID.

Add a source-boundary assertion to the same test:

```rust
#[test]
fn cli_and_mcp_do_not_call_a_shared_recording_wrapper() {
  let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let cli = std::fs::read_to_string(manifest.join("src/cli_frontend.rs")).unwrap();
  let mcp = std::fs::read_to_string(manifest.join("../../src/mcp.rs")).unwrap();
  for forbidden in ["RunRecordingBackend", "recorded::", "execute_with_tracing", "run_operation"] {
    assert!(!cli.contains(forbidden), "CLI uses {forbidden}");
    assert!(!mcp.contains(forbidden), "MCP uses {forbidden}");
  }
}
```

Run: `cargo test -p auv-cli --test run_recording_frontends`

Expected: FAIL because invoke still requires `RunRecordingBackend`.

- [ ] **Step 2: Make the shared command function return its value directly**

```rust
pub struct InvokeCommandInput {
  pub command_id: String,
  pub target_application_id: Option<String>,
  pub inputs: std::collections::BTreeMap<String, String>,
  pub dry_run: bool,
}

pub type InvokeCommandFuture = std::pin::Pin<
  Box<dyn std::future::Future<Output = Result<InvokeCommandOutput, String>> + Send + 'static>,
>;

pub type InvokeCommandHandler = fn(InvokeCommandInput) -> InvokeCommandFuture;

pub struct InvokeCommand {
  pub id: &'static str,
  pub namespace: InvokeNamespace,
  pub summary: &'static str,
  pub args: &'static [ArgSpec],
  handler: InvokeCommandHandler,
}

impl InvokeCommand {
  pub fn invoke(&self, input: InvokeCommandInput) -> InvokeCommandFuture {
    (self.handler)(input)
  }
}
```

Keep the existing concrete metadata/registry/group/help model; only make its
owned handler future asynchronous. Each CLI handler calls a named typed domain
function and maps that direct value into the existing CLI-only
`InvokeCommandOutput`; MCP calls the same domain function through its own
adapter and does not use `InvokeCommand` or reuse its output type.
`command_id` belongs to the CLI catalog and is not added to canonical run
facts.

Update `#[invoke_command]` to generate a private boxed adapter for each
annotated async handler before passing it to `command::spec`:

```rust
fn generated_handler(
  input: ::auv_cli_invoke::InvokeCommandInput,
) -> ::auv_cli_invoke::InvokeCommandFuture {
  Box::pin(annotated_handler(input))
}
```

Keep `id`, `group`, `summary`, and `args` macro metadata unchanged. Add a macro
unit test that expands an async handler and type-checks the generated
`InvokeCommand`; do not replace the concrete command with a trait object.

Command implementations may call `Context::current`, `start_span!`,
`emit_event!`, and await `emit_artifact!`; they never receive a runner or
recording backend. Replace
`ProducedArtifact` path/role/note records at each command with an owned reader,
`ArtifactPurpose`, `ContentType`, digest, length, and attributes passed directly
to `emit_artifact!`. When emission returns `Some`, a command-specific CLI
adapter may expose its canonical URI in that command's presentation; disabled
tracing returns `None` and does not change the primary domain value or error.
Do not add a generic attachment/result/failure shape to `auv-tracing`.
`InvokeResult` remains
`auv-cli-invoke` presentation data; it is never rebuilt from `RunStore` and no
field is canonical run truth.

- [ ] **Step 3: Configure CLI and MCP independently**

Implement the two composition paths separately. The CLI path selects its
authority from CLI options, builds a dispatch, creates a fresh `RunId`, creates
`Context::root(run_id)`, resolves the concrete CLI command, constructs its
future while the root is current, and then awaits it with poll/drop propagation:

```rust
let future = root.in_scope(|| command.invoke(input));
let result = root.instrument(future).await;
```

It then
maps the returned value to CLI output, applies CLI flush-error policy, and
prints the result. The MCP path owns its own dispatch configuration and its own
tool adapter; each tool call creates or explicitly continues a run context and
uses the same two-phase construction/poll pattern around the named typed domain
function:

```rust
let future = root.in_scope(|| invoke_domain_function(input));
let value = root.instrument(future).await;
```

MCP maps that domain value to MCP content and applies MCP flush-error policy.
Artifact emission remains at the app/command call site that
knows the typed payload and purpose. Do not extract the duplicated orchestration into
`auv-cli-invoke`, `src/runtime.rs`, or a new shared service.

Library callers invoke the same command function directly. They get opt-in
instrumentation only when they make a context current; otherwise the typed AUV
calls are disabled. Keep `crates/auv-cli-invoke/src/recorded.rs` only until all
remaining legacy callers migrate in Task 22. Add a `NOTICE(run-recording-v1)` at
its module export saying it is a temporary legacy adapter and that no new call
site may depend on it; do not delete it while the workspace still compiles
against it.

- [ ] **Step 4: Prove tracing failure does not re-execute work**

Extend the test with a store returning `CommitUnknown` and with a projector
returning `TelemetryError`; assert `CountingCall::call_count()` remains one for
each frontend call and the direct result/error policy is frontend-owned. Assert
that neither failure adds an operation-success claim, verification claim, retry
advice, or recommended action to any canonical fact. There must be no
whole-command retry based on tracing errors.

- [ ] **Step 5: Run focused frontend tests**

Run: `cargo test -p auv-cli --test run_recording_frontends && cargo test -p auv-cli-invoke && cargo test -p auv-runtime mcp`

Expected: PASS.

- [ ] **Step 6: Commit frontend composition**

```bash
git add crates/auv-cli-invoke crates/auv-cli-invoke-macros/src/lib.rs crates/auv-cli/src/cli_frontend.rs src/mcp.rs Cargo.toml Cargo.lock
git commit --only crates/auv-cli-invoke crates/auv-cli-invoke-macros/src/lib.rs crates/auv-cli/src/cli_frontend.rs src/mcp.rs Cargo.toml Cargo.lock -m "refactor: compose auv tracing at frontend roots"
```

### Task 17: Migrate The Root Scroll-Scan Artifact Slice

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/scroll_scan/mod.rs`
- Modify: `src/scroll_scan/observation.rs`
- Modify: `src/run_read/mod.rs`
- Modify: `src/inspect/sections.rs`
- Create: `tests/scroll_scan_run_artifact.rs`

- [ ] **Step 1: Add a failing typed producer-to-reader round-trip test**

```rust
#[test]
fn scroll_scan_round_trips_through_the_run_store() {
  futures_executor::block_on(async {
    let fixture = RootRunFixture::memory();
    let expected: ScrollScanArtifact = sample_scroll_scan_artifact();
    let published = fixture.publish_scroll_scan(&expected).await.unwrap();
    assert_eq!(published.purpose().as_str(), "auv.runtime.scroll_scan");
    let decoded = read_scroll_scan(fixture.store(), fixture.snapshot().await.as_ref(), published.uri()).await.unwrap();
    assert_eq!(decoded, expected);
  });
}
```

Run: `cargo test -p auv-runtime --test scroll_scan_run_artifact`

Expected: FAIL because the producer and reader still require role/path-based
legacy recording.

- [ ] **Step 2: Emit the exact owned payload**

Define `SCROLL_SCAN_PURPOSE = "auv.runtime.scroll_scan"` beside
`ScrollScanArtifact`. Serialize that exact Rust type as `application/json` into
`NewArtifact<R>` and await `emit_artifact!` from the scroll-scan call site.
Known limits and observation snapshots remain fields of `ScrollScanArtifact`;
they are not normalized into generic tracing verification or observation
objects. Remove only this slice's preferred filename, role, summary, and path
inputs.

- [ ] **Step 3: Read by purpose, URI, and committed metadata**

`read_scroll_scan` accepts `&dyn RunStore`, `&RunSnapshot`, and `&ArtifactUri`.
It requires exact purpose `auv.runtime.scroll_scan`, exact content type
`application/json`, matching committed length/digest, then decodes
`ScrollScanArtifact`. The root inspect section uses this reader. Other legacy
root readers and adapters remain untouched until Task 22; this task neither
dual-writes the scroll-scan artifact nor deletes unrelated compatibility paths.

- [ ] **Step 4: Run the focused root slice**

Run: `cargo test -p auv-runtime --test scroll_scan_run_artifact && cargo test -p auv-runtime scroll_scan`

Expected: PASS.

- [ ] **Step 5: Commit the scroll-scan slice**

```bash
git add Cargo.toml Cargo.lock src/scroll_scan src/run_read/mod.rs src/inspect/sections.rs tests/scroll_scan_run_artifact.rs
git commit --only Cargo.toml Cargo.lock src/scroll_scan src/run_read/mod.rs src/inspect/sections.rs tests/scroll_scan_run_artifact.rs -m "refactor(auv-runtime): migrate scroll scan artifacts"
```

### Task 18: Migrate NetEase Recording And Lineage To Artifact URIs

**Files:**
- Modify: `crates/auv-netease-music/Cargo.toml`
- Modify: `crates/auv-netease-music/src/invoke/sidebar_scan_proof.rs`
- Rewrite: `crates/auv-netease-music/src/recording.rs`
- Create: `crates/auv-netease-music/tests/run_store_contract.rs`

- [ ] **Step 1: Add a failing typed round-trip test**

```rust
#[test]
fn playlist_scan_and_view_memory_round_trip_by_uri() {
  futures_executor::block_on(async {
    let fixture = NeteaseRunFixture::memory();
    let persisted = fixture.persist_playlist_scan(sample_scan(), sample_memory()).await.unwrap();
    assert!(persisted.lineage.scan_uri.as_str().starts_with("auv://runs/"));
    assert_eq!(fixture.read_scan(&persisted.lineage.scan_uri).await.unwrap(), sample_scan());
    assert_eq!(fixture.read_memory(persisted.lineage.memory_uri.as_ref().unwrap()).await.unwrap(), sample_memory());
  });
}
```

Run: `cargo test -p auv-netease-music --test run_store_contract`

Expected: FAIL because `ViewMemoryRunLineage` stores string IDs and readers open
files through `LocalStore::artifact_file`.

- [ ] **Step 2: Replace role/path matching with exact purposes**

Define validated constants in `recording.rs` and use only these values:

```rust
pub const PLAYLIST_SIDEBAR_SCAN_PURPOSE: &str = "auv.netease.playlist_sidebar_scan";
pub const VIEW_MEMORY_PURPOSE: &str = "auv.netease.view_memory";
pub const PLAYLIST_SELECT_RESULT_PURPOSE: &str = "auv.netease.playlist_select_result";
```

Encode `PlaylistSidebarScan`, `ViewMemory`, and the existing playlist-select
result as `application/json`. `ViewMemoryRunLineage` stores `scan_uri:
ArtifactUri` and `memory_uri: Option<ArtifactUri>`; remove `run_id`, bare
artifact IDs, `parse_lineage_scan_artifact_id`, whitespace
`artifact_id=...` references, preferred filenames, and path suffix tests. Keep the lineage file
only as an app-local pointer to canonical URIs, not as another run authority.

- [ ] **Step 3: Use the caller's context and direct values**

Replace `RunRecordingBackend::local_only` and `run_recorded_operation` with
ordinary async functions that run inside the caller's current context and await
`emit_artifact!`. Return `PersistedLineage` or the existing select result
directly. Read through `RunStore::open_artifact`, verify purpose/content type/
digest/length from committed metadata, then deserialize the owning type.

- [ ] **Step 4: Run feature and recording tests**

Run: `cargo test -p auv-netease-music --no-default-features && cargo test -p auv-netease-music --features tracing --test tracing_opt_in && cargo test -p auv-netease-music --test run_store_contract`

Expected: PASS, and `rg -n 'auv_tracing_driver|auv-tracing-driver|LocalStore|RunRecordingBackend' crates/auv-netease-music` exits 1.

- [ ] **Step 5: Commit NetEase migration**

```bash
git add crates/auv-netease-music Cargo.lock
git commit --only crates/auv-netease-music Cargo.lock -m "refactor(auv-netease-music): adopt run artifact URIs"
```

### Task 19: Migrate Balatro Run Producers And Readers

**Files:**
- Modify: `crates/auv-game-balatro/Cargo.toml`
- Modify: `crates/auv-game-balatro/src/card_detection_eval_witness.rs`
- Modify: `crates/auv-game-balatro/src/card_detection_quality.rs`
- Modify: `crates/auv-game-balatro/src/card_detection_semantic.rs`
- Modify: `crates/auv-game-balatro/src/card_detection_spatial_query.rs`
- Modify: `crates/auv-game-balatro/src/inspect/sections.rs`
- Modify: `crates/auv-game-balatro/src/inspect/tests_smoke.rs`
- Rewrite: `crates/auv-game-balatro/src/run_read.rs`
- Create: `crates/auv-game-balatro/tests/tracing_contract.rs`

- [ ] **Step 1: Add a failing Balatro public-behavior migration test**

The test writes the existing typed witness through `MemoryRunStore`, loads the
snapshot, finds it by exact `ArtifactPurpose`, opens the URI, and decodes the
existing domain type:

```rust
let balatro: CardDetectionEvalWitnessManifest =
  read_card_detection_witness(&store, balatro_published).await.unwrap();
assert_eq!(balatro, expected_balatro);
```

Implement the named reader in its owning existing module. Each reader verifies
the exact purpose/content type and decodes bytes returned by `open_artifact`;
none accepts a filesystem path.

Run: `cargo test -p auv-game-balatro --test tracing_contract`

Expected: FAIL because the readers still require `CanonicalRun` and file paths.

- [ ] **Step 2: Convert game artifacts and diagnostics to V1 facts**

Define these purpose constants in the artifact-owning modules:

```text
auv.balatro.card_detection.quality
auv.balatro.card_detection.semantic
auv.balatro.card_detection.spatial_query
auv.balatro.card_detection.eval_witness
```

Serialize each existing typed value directly into a bounded reader and emit it
with `application/json`. Do not create diagnostic event schemas in this task;
only the four listed typed JSON artifacts are in scope. Delete
role/summary/preferred-name/path matching.

- [ ] **Step 3: Rewrite game run readers and inspect sections**

Readers accept `&dyn RunStore` plus `&RunSnapshot`, select exact schema or
purpose values, call `open_artifact`, verify the committed metadata, and decode
within the owning crate. Inspect sections consume those typed readers. Preserve
existing semantic/quality/spatial-query domain types; do not normalize them
into core verification or observation records.

- [ ] **Step 4: Run all affected game suites**

Run: `cargo test -p auv-game-balatro`

Expected: PASS.

- [ ] **Step 5: Commit game migration**

```bash
git add crates/auv-game-balatro Cargo.lock
git commit --only crates/auv-game-balatro Cargo.lock -m "refactor(auv-game-balatro): migrate run artifacts"
```

### Task 20: Migrate Minecraft Run Producers And Readers

**Files:**
- Modify: `crates/auv-game-minecraft/Cargo.toml`
- Modify: `crates/auv-game-minecraft/src/artifact.rs`
- Modify: `crates/auv-game-minecraft/src/inspect/sections.rs`
- Modify: `crates/auv-game-minecraft/src/inspect/tests_smoke.rs`
- Modify: `crates/auv-game-minecraft/src/prep.rs`
- Rewrite: `crates/auv-game-minecraft/src/run_read.rs`
- Modify: `crates/auv-game-minecraft/src/sample_builder.rs`
- Modify: `crates/auv-game-minecraft/src/scene_packet.rs`
- Modify: `crates/auv-game-minecraft/src/training_job.rs`
- Modify: `crates/auv-game-minecraft/src/training_launch.rs`
- Modify: `crates/auv-game-minecraft/src/training_package.rs`
- Modify: `crates/auv-game-minecraft/src/training_result.rs`
- Modify: `crates/auv-game-minecraft/src/training_result_artifact.rs`
- Modify: `crates/auv-game-minecraft/src/training_result_holdout_preview.rs`
- Modify: `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`
- Modify: `crates/auv-game-minecraft/src/training_result_semantic.rs`
- Modify: `crates/auv-game-minecraft/src/training_result_spatial_query.rs`
- Create: `crates/auv-game-minecraft/tests/tracing_contract.rs`

- [ ] **Step 1: Add a failing projection round-trip test**

```rust
#[test]
fn projection_round_trips_without_a_file_locator() {
  futures_executor::block_on(async {
    let fixture = MinecraftRunFixture::memory();
    let published = fixture.publish_projection(expected_projection()).await.unwrap();
    let decoded: MinecraftProjectionArtifact =
      read_minecraft_projection(fixture.store(), &published).await.unwrap();
    assert_eq!(decoded, expected_projection());
    assert_eq!(published.purpose().as_str(), "auv.minecraft.projection");
  });
}
```

Run: `cargo test -p auv-game-minecraft --test tracing_contract`

Expected: FAIL because the reader still consumes `CanonicalRun` and artifact
paths.

- [ ] **Step 2: Assign exact purposes to existing Minecraft payloads**

```text
auv.minecraft.projection
auv.minecraft.scene_packet
auv.minecraft.training.job
auv.minecraft.training.package
auv.minecraft.training.result
auv.minecraft.training.holdout_preview
auv.minecraft.training.holdout_render_quality
auv.minecraft.training.semantic
auv.minecraft.training.spatial_query
```

Keep each payload's existing Rust struct and validation. Every purpose listed
above is a typed JSON artifact with `application/json`; this task does not add a
second preview-image artifact for `holdout_preview`. Do not translate
projection, readiness, mismatch-refusal, or training status into generic core
verification or observation records.

- [ ] **Step 3: Rewrite Minecraft readers against committed metadata**

Readers accept `&dyn RunStore` and `&RunSnapshot`, select an exact purpose,
open the canonical URI, verify content type/length/digest, and decode in the
Minecraft crate. Inspect sections call those typed readers. Remove
`EvidenceCorrelationKey`, role, summary, preferred filename, and path lookup
from the storage boundary; preserve domain correlation fields inside the typed
payload where they still have consumers.

- [ ] **Step 4: Run the Minecraft suite**

Run: `cargo test -p auv-game-minecraft && cargo test -p auv-game-minecraft --test tracing_contract`

Expected: PASS, and `rg -n 'auv_tracing_driver|auv-tracing-driver|CanonicalRun|ArtifactRecordV1Alpha1' crates/auv-game-minecraft` exits 1.

- [ ] **Step 5: Commit Minecraft migration**

```bash
git add crates/auv-game-minecraft Cargo.lock
git commit --only crates/auv-game-minecraft Cargo.lock -m "refactor(auv-game-minecraft): migrate run artifacts"
```

### Task 21: Migrate osu! Run Producers And Readers

**Files:**
- Modify: `crates/auv-game-osu/Cargo.toml`
- Modify: `crates/auv-game-osu/src/detection_eval_quality.rs`
- Modify: `crates/auv-game-osu/src/detection_eval_witness.rs`
- Modify: `crates/auv-game-osu/src/inspect/sections.rs`
- Modify: `crates/auv-game-osu/src/inspect/tests_smoke.rs`
- Modify: `crates/auv-game-osu/src/projection.rs`
- Rewrite: `crates/auv-game-osu/src/run_read.rs`
- Modify: `crates/auv-game-osu/src/visual_truth_semantic.rs`
- Modify: `crates/auv-game-osu/src/visual_truth_spatial_query.rs`
- Create: `crates/auv-game-osu/tests/tracing_contract.rs`

- [ ] **Step 1: Add a failing projection round-trip test**

```rust
#[test]
fn osu_projection_round_trips_by_canonical_uri() {
  futures_executor::block_on(async {
    let fixture = OsuRunFixture::memory();
    let published = fixture.publish_projection(expected_projection()).await.unwrap();
    let decoded: ProjectionArtifact =
      read_osu_projection(fixture.store(), &published).await.unwrap();
    assert_eq!(decoded, expected_projection());
    assert_eq!(published.purpose().as_str(), "auv.osu.projection");
  });
}
```

Run: `cargo test -p auv-game-osu --test tracing_contract`

Expected: FAIL because the reader still consumes the legacy store shape.

- [ ] **Step 2: Assign exact purposes to existing osu! payloads**

```text
auv.osu.projection
auv.osu.detection_eval.quality
auv.osu.detection_eval.witness
auv.osu.visual_truth.semantic
auv.osu.visual_truth.spatial_query
```

Each listed purpose encodes its existing owning Rust payload as
`application/json`. Do not add benchmark captures, dataset directories, or a
generic diagnostic event schema in this crate migration task; the CLI-owned
integration is removed from the recording wrapper in Task 22 without turning
those output directories into new canonical artifact families.

Emit the existing typed JSON payloads and keep their domain validation. Do not
promote visual-truth status or a projection reference into a generic tracing
claim.

- [ ] **Step 3: Rewrite osu! readers and inspect sections**

Select committed artifacts by exact purpose, open them through `RunStore`,
verify committed metadata, and decode into `ProjectionArtifact` or the owning
manifest type. Remove role/path matching and `EvidenceCorrelationKey` from the
storage boundary.

- [ ] **Step 4: Run the osu! suite**

Run: `cargo test -p auv-game-osu && cargo test -p auv-game-osu --test tracing_contract`

Expected: PASS, and `rg -n 'auv_tracing_driver|auv-tracing-driver|CanonicalRun|ArtifactRecordV1Alpha1' crates/auv-game-osu` exits 1.

- [ ] **Step 5: Commit osu! migration**

```bash
git add crates/auv-game-osu Cargo.lock
git commit --only crates/auv-game-osu Cargo.lock -m "refactor(auv-game-osu): migrate run artifacts"
```

### Task 22: Migrate Final Root/CLI Callers And Delete Shared Recording Adapters

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/api/session_service/handler.rs`
- Modify: `src/api/session_service/mapper.rs`
- Modify: `src/api/session_service/operation_result_store.rs`
- Modify: `src/api/session_service/registry.rs`
- Modify: `src/api/session_service/summary.rs`
- Modify: `src/api/session_service/summary_store.rs`
- Modify: `src/api/session_service/test_fixtures.rs`
- Modify: `src/app/analysis.rs`
- Modify: `src/app/infra.rs`
- Modify: `src/app/mod.rs`
- Modify: `src/app/tests.rs`
- Modify: `src/candidate_promotion.rs`
- Modify: `src/contract.rs`
- Modify: `src/inspect/mod.rs`
- Modify: `src/inspect/sections.rs`
- Modify: `src/lib.rs`
- Modify: `src/model.rs`
- Modify: `src/run_read/mod.rs`
- Modify: `src/runtime.rs`
- Modify: `src/scene_state_read.rs`
- Modify: `src/scroll_scan/mod.rs`
- Modify: `src/session.rs`
- Modify: `src/view_parser_read.rs`
- Modify: `crates/auv-cli/Cargo.toml`
- Modify: `crates/auv-cli/src/lib.rs`
- Modify: `crates/auv-cli/src/cli_frontend.rs`
- Modify: `crates/auv-cli/src/mcp.rs`
- Delete: `crates/auv-cli/src/invoke.rs`
- Modify: `crates/auv-cli/src/inspect/goldens.rs`
- Modify: `crates/auv-cli/src/inspect/mod.rs`
- Modify: `crates/auv-cli/src/inspect/sections.rs`
- Modify: `crates/auv-cli/src/integrations/balatro/mod.rs`
- Modify: `crates/auv-cli/src/integrations/minecraft/mod.rs`
- Modify: `crates/auv-cli/src/integrations/minecraft/query_live_action.rs`
- Modify: `crates/auv-cli/src/integrations/minecraft/session.rs`
- Modify: `crates/auv-cli/src/integrations/minecraft/verification.rs`
- Modify: `crates/auv-cli/src/integrations/osu/mod.rs`
- Modify: `crates/auv-cli/src/integrations/osu/query_live_action.rs`
- Modify: `crates/auv-cli/src/integrations/textedit/mod.rs`
- Modify: `crates/auv-cli/src/projection.rs`
- Modify: `crates/auv-cli/src/run_read/mod.rs`
- Modify: `crates/auv-cli/src/run_read/query_wired_live_action.rs`
- Modify: `crates/auv-cli/src/run_read/query_wired_projection.rs`
- Modify: `crates/auv-cli/tests/textedit_document_write_parity.rs`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/render.rs`
- Delete: `crates/auv-cli-invoke/src/recorded.rs`
- Delete: `crates/auv-cli-invoke/src/summary.rs`
- Create: `crates/auv-cli/tests/legacy_recording_boundary.rs`

- [ ] **Step 1: Add a failing whole-surface dependency test**

```rust
#[test]
fn product_cli_has_no_recording_runtime_or_shared_invoke_wrapper() {
  let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
  let roots = [repo.join("src"), repo.join("crates/auv-cli"), repo.join("crates/auv-cli-invoke")];
  let forbidden = [
    "auv_tracing_driver", "auv-tracing-driver", "RunRecordingBackend",
    "RecordedOperationContext", "OperationSummary", "invoke_recorded",
    "render_recorded_invoke",
  ];
  let matches = scan_rust_and_manifests(&roots, &forbidden);
  assert!(matches.is_empty(), "legacy product CLI references: {matches:?}");
}

fn scan_rust_and_manifests(roots: &[std::path::PathBuf], needles: &[&str]) -> Vec<std::path::PathBuf> {
  let self_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/legacy_recording_boundary.rs")
    .canonicalize()
    .unwrap();
  let mut pending = roots.to_vec();
  let mut matches = Vec::new();
  while let Some(path) = pending.pop() {
    if path.is_dir() {
      pending.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
    } else if path != self_path
      && (path.extension().and_then(|value| value.to_str()) == Some("rs")
        || path.file_name().and_then(|value| value.to_str()) == Some("Cargo.toml"))
      && std::fs::read_to_string(&path).is_ok_and(|source| needles.iter().any(|needle| source.contains(needle)))
    {
      matches.push(path);
    }
  }
  matches.sort();
  matches
}
```

Run: `cargo test -p auv-cli --test legacy_recording_boundary`

Expected: FAIL and list the remaining CLI integration and shared wrapper files.

- [ ] **Step 2: Convert each product integration to direct async functions**

Balatro, Minecraft, osu!, and TextEdit functions return their existing domain
values directly and use `Context::current` for optional spans/events. They emit
artifacts with the purpose constants established in Tasks 19-21. TextEdit owns
event schema `auv.textedit.document_write.verification` version 1 and continues
to return its existing `VerificationResult`; the canonical run records the
domain event but does not derive an operation status or retry recommendation.

Migrate the remaining root-owned artifacts with this exact table; all are
`application/json` and all readers are named here:

| Purpose | Rust payload | Owning reader |
|---|---|---|
| `auv.driver.input_action_result` | `auv_driver::InputActionResult` | `list_input_action_results` |
| `auv.runtime.detector_recognition` | `RecognitionResult` | `list_detector_recognition_lineage` |
| `auv.runtime.scene_state_input` | `SceneStateInputWire` | `read_scene_state_input` |
| `auv.runtime.scan_coverage` | `ScanCoverageWire` | `read_scan_coverage` |

Use `NewArtifact<R>` at each typed producer. Do not add a runtime-generic
verification event. `VerificationResult` remains in its app-owned direct result
or in the TextEdit-owned schema above. Retire persisted `OperationResult`,
`OperationSummaryRecord`, run status, and output summary instead of assigning
them new purposes.

The CLI composition root from Task 16 creates the run/context and chooses
flush-error policy. Root session handlers, CLI, and
`crates/auv-cli/src/mcp.rs` independently construct each domain future inside
`root.in_scope`, then await it through `root.instrument(future)` and map the
same domain functions to their own frontend. Neither calls a helper that owns run creation,
span lifecycle, result mapping, and persistence together.

- [ ] **Step 3: Rewrite product readers and remove the adapters atomically**

Rewrite root and CLI readers against `RunSnapshot` and
`RunStore::open_artifact`, using the exact domain purposes from Tasks 17-21 and
the root table above. Preserve domain types and
rendering; remove `CanonicalRun`, role/path matching, generic operation-summary
reconstruction, and synthetic run status. Replace whitespace-formatted
`kind=... artifact_id=... run_id=...` self-references in
`query_wired_projection.rs` with typed `ArtifactUri` fields owned by the CLI
projection DTO; parsing by string splitting is forbidden. After the final caller is gone,
delete `crates/auv-cli/src/invoke.rs` and
`crates/auv-cli-invoke/src/recorded.rs`, remove their exports, and rename
`render_recorded_invoke` to a pure renderer that accepts an already returned
`InvokeResult`. Delete `crates/auv-cli-invoke/src/summary.rs` and all
`OperationSummary*` exports/caches; no run-store or process-local summary
projection replaces them.

- [ ] **Step 4: Run CLI, invoke, and parity tests**

Run: `cargo test -p auv-runtime && cargo test -p auv-cli-invoke && cargo test -p auv-cli --test run_recording_frontends --test legacy_recording_boundary --test textedit_document_write_parity && cargo test -p auv-cli`

Expected: PASS. The parity test compares direct CLI and MCP values plus their
independently created run IDs; it does not require a shared operation ID, run
status, or persisted invoke result.

- [ ] **Step 5: Commit the CLI migration**

```bash
git add -A -- Cargo.toml Cargo.lock src crates/auv-cli crates/auv-cli-invoke
git commit --only Cargo.toml Cargo.lock src crates/auv-cli crates/auv-cli-invoke -m "refactor: remove shared recording adapters"
```

### Task 23: Remove `auv-tracing-driver` And Complete Contract Validation

**Files:**
- Remove: `crates/auv-tracing-driver/`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/ai/references/inspect/INDEX.md`
- Modify: `docs/ai/references/inspect/2026-07-20-auv-run-recording-contract-v1-spec.md`
- Modify: `docs/TERMS_AND_CONCEPTS.md`

- [ ] **Step 1: Verify the migration has no legacy import stragglers**

Run:

```bash
rg -n 'auv_tracing_driver|auv-tracing-driver|RunRecordingBackend|RunRecorder|RunUpdate|CanonicalRun|RecordedOperation|OperationSummaryRecord|OPERATION_SUMMARY_ARTIFACT_ROLE' \
  Cargo.toml crates src --glob '*.rs' --glob 'Cargo.toml' \
  --glob '!**/auv-tracing-driver/**' --glob '!legacy_recording_boundary.rs'
```

Expected: exit status 1 and no output. Any match belongs to Tasks 17-22 and
must be fixed before continuing; the exclusions cover the crate being deleted
and the boundary test's own string literals only.

- [ ] **Step 2: Remove the legacy crate and rejected names**

Delete `crates/auv-tracing-driver/`, its workspace member/dependencies, and
regenerate `Cargo.lock`. Run:

```bash
rg -n '(^|[^[:alnum:]_-])auv[-_]run([^[:alnum:]_-]|$)|RunSession|OperationExecution|ExecutionId|RunOpened|RunSealed|OtelMirror|CompositeSink' \
  Cargo.toml crates/auv-tracing crates/auv-tracing-inspect crates/auv-tracing-otel \
  --glob '*.rs' --glob 'Cargo.toml' --glob '!crate_identity.rs'
```

Expected: exit status 1 and no output.

- [ ] **Step 3: Run the complete validation matrix**

```bash
cargo fmt --check
cargo check --workspace --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
npm --prefix crates/auv-inspect-server/viewer run smoke
npm --prefix crates/auv-inspect-server/viewer run build
cargo run --quiet -- invoke --help
```

Expected: all commands exit 0. `invoke --help` lists commands without
initializing a tracing authority or creating a run.

- [ ] **Step 4: Mark implementation state and preserve the research audit**

Change the spec status to `implemented V1 contract`. Mark this plan complete in
the Inspect index. Keep the 2026-07-17 research/audit document and all of its
comparison tables intact. Remove historical terms from
`TERMS_AND_CONCEPTS.md` only when no live reader or migration note needs them;
retain concise historical definitions otherwise.

- [ ] **Step 5: Validate the final document and index edits**

Run: `git diff --check && git diff --cached --check`

Expected: exit 0 with no output.

- [ ] **Step 6: Commit retirement and documentation**

```bash
git add -A -- Cargo.toml Cargo.lock crates/auv-tracing-driver docs/TERMS_AND_CONCEPTS.md docs/ai/references/inspect
git commit --only Cargo.toml Cargo.lock crates/auv-tracing-driver docs/TERMS_AND_CONCEPTS.md docs/ai/references/inspect -m "refactor: complete auv tracing contract migration"
```

## Spec Coverage Matrix

| Spec surface | Implementing tasks |
|---|---|
| vocabulary, ownership, no central runner | 1, 15-17, 22, 23 |
| primitive values, attributes, event payload, artifact URI | 2 |
| canonical span/event/artifact facts and appendable runs | 3 |
| object-safe store, idempotency, snapshots, pages, subscriptions | 4, 5 |
| dispatch, current context, disabled behavior | 6 |
| span clone/drop semantics, async wrappers, propagation | 7 |
| telemetry routing, error reporting, ordering, flush | 8 |
| detached binary artifact jobs | 9 |
| durable local authority and crash recovery | 10 |
| Inspect run HTTP/SSE protocol and viewer recovery | 11, 12 |
| Inspect upload/read/resolve protocol | 12 |
| strict Inspect decoding and remote conformance | 13 |
| Rust tracing and OTEL bounded projection | 14 |
| driver/app opt-in callsites | 15 |
| CLI/MCP/library composition without a runner | 16 |
| root runtime producer/reader migration | 17 |
| NetEase typed artifact/lineage migration | 18 |
| Balatro, Minecraft, and osu! owner-specific migration | 19-21 |
| product CLI integration and recorded-wrapper removal | 22 |
| legacy retirement and workspace validation | 23 |

## Explicitly Out Of Scope

The implementation must not add generic verification or observation models, continuous media streams, run sealing/amendment, store replication, resumable multipart upload, a generic resource resolver, automatic ingestion of arbitrary Rust tracing data, metrics projection, or a common CLI/MCP/library result model. Reopening any of those surfaces requires a separate owner-approved contract.
