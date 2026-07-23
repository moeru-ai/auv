# AUV Tracing Contract And Invoke Output Design

Status: draft design direction under review, not implementation-ready

Responsibility: inspect / run recording / invoke output contract

Session: Codex `019f6ef9-a4be-7950-be6a-91316a77c2ab`

> **Non-normative research and migration input.** This document retains the
> repository audit, external references, comparison tables, rejected candidate
> shapes, and migration evidence collected during design. The greenfield V1
> target is now reviewed separately in
> [`2026-07-20-auv-run-recording-contract-v1-spec.md`](2026-07-20-auv-run-recording-contract-v1-spec.md).
> Do not implement candidate types from this document without reconciling them
> with that spec.

## Context

`auv-cli-invoke` currently returns a flat `InvokeCommandOutput` with
`signals`, `known_limits`, `verification`, `notes`, and `artifacts`.
Those fields mix several concepts:

- `signals` are final command-result attributes, but the name sounds like a
  trace signal or event.
- `verification` is currently a human string such as `capture-only`, not a
  structured set of evidence-backed verification records.
- `known_limits` are diagnostic events or warnings, but are rendered only in
  some surfaces.
- `artifacts` is overloaded across the repository and currently names both
  persisted trace records and invoke result attachments.
- `auv-tracing-driver` owns both trace contract records and the recorder/store
  implementation, even though the contract types are useful outside driver
  recording.

The redesign should align with Rust observability libraries instead of
inventing a parallel vocabulary. OpenTelemetry Rust provides the useful model:
spans have attributes, events, status, and links; applications configure SDKs
and exporters separately from library-side instrumentation.

## Goals

- Introduce an `auv-tracing` crate for the reusable AUV tracing substrate:
  OTEL-shaped records, recording lifecycle, listener routing, and store
  traits.
- Move reusable IDs, attributes, span/event/run records, and attachment record
  contracts out of `auv-tracing-driver`.
- Keep concrete storage, network listeners, and driver/runtime ergonomics
  behind reusable interfaces instead of letting every module invent its own
  tracing and trajectory recording path.
- Keep `auv-tracing-driver` focused on compatibility and concrete
  implementation helpers that have not yet moved into the tracing substrate or
  an implementation crate.
- Make Rust `tracing` / OpenTelemetry export a first-class path for
  performance analysis and flow visualization, while keeping AUV's durable
  trajectory store as the source of truth for inspect/replay/eval.
- Replace invoke `signals` with OTEL-shaped attributes.
- Replace invoke string `verification` with structured verification records
  and explicit unresolved design rules for aggregation.
- Replace invoke-facing `artifacts` terminology with attachments while still
  allowing storage-side compatibility during migration.
- Make CLI JSON, MCP invoke output, and persisted operation-summary records
  share the same field names.

## Non-Goals

- Do not make OpenTelemetry export the source of truth for AUV run storage.
- Do not reconstruct AUV runs from a `tracing` subscriber in this slice.
- Do not require an OpenTelemetry collector/backend for local AUV recording to
  work.
- Do not keep new execution on both `OperationResult` / `VerificationResult`
  and `OperationOutcome<T>`. Legacy types may remain only as historical
  storage/read compatibility during migration.
- Do not broaden direct invoke command behavior while changing the output
  contract.
- Do not add compatibility aliases such as `signals` unless a later owner
  decision requires them.

## Ecosystem Guidance

Prefer existing Rust observability crates over new infrastructure:

- Use `opentelemetry` / `opentelemetry_sdk` as the conceptual reference for
  API versus SDK/exporter separation, attribute values, span status, and
  events.
- Use `tracing` as the library-side instrumentation model.
- Use `tracing-opentelemetry` or an explicit adapter if AUV later exports
  durable runs to OTLP.
- Do not install subscribers or exporters in contract crates or recorder
  crates. Binaries, servers, and test harnesses own that setup.

The first implementation should not reimplement a private OpenTelemetry value
system. AUV's durable records should keep the existing JSON-friendly attribute
bag shape and add conversions to/from OTEL types only at adapter boundaries.
Add conversions to/from OTEL types behind a narrow adapter or optional feature
only when an actual exporter or bridge needs them.

OpenTelemetry is still an explicit design target. Every durable run/span/event
lifecycle should be mirrorable to Rust `tracing` spans/events with stable AUV
correlation fields so users can inspect latency and flow in Jaeger, Tempo,
Honeycomb, Langfuse, Phoenix, or similar OTEL-compatible tools. The mirror is
an export path, not the owner of AUV persistence.

## Architecture Boundary Clarification

`auv-tracing` should own AUV's runtime trajectory model, not only a conversion
layer to OpenTelemetry. The crate should define the type/struct/trait surface
for data that must be observable, state-bearing, inspectable, and replayable:
runs, spans, events, attachments, verification records, failures, update
streams, read/write traits, reducers, and read-side projections.

OpenTelemetry and Rust `tracing` remain adapter targets. AUV records may be
mirrored to OTEL spans/events or rewritten/rerouted through an OTEL bridge, but
OTEL must not become the source of truth for AUV state. Inspect, replay, eval,
dataset collection, attachment purpose reads, and operation/session projections
should read AUV records, not reconstruct state from a telemetry subscriber or
collector backend.

The intended relationship is:

```text
domain crate / driver / invoke handler
  -> auv-tracing lifecycle API
  -> canonical AUV records and RunUpdate stream
  -> one or more AUV stores / listeners / inspect adapters
  -> optional Rust tracing / OpenTelemetry mirror
```

This keeps the common call path simple for crates such as `auv-cli-invoke`,
`auv-media-macos`, and `auv-netease-music`: they emit typed facts and evidence
through a `TraceHandle`-like API without knowing whether the caller configured
local recording, inspect-server reporting, OTEL export, memory-only capture, or
no recording at all.

## Store / Recorder / Exporter Boundary

Current code uses `RunRecordingBackend` for a combined concern: one `LocalStore`
for canonical snapshots and artifact staging, plus one `RunRecorder` for live
update delivery. That shape is useful as migration glue, but the new contract
should keep the concepts separate:

| Concept | Owns | Does not own |
|---|---|---|
| Trace store | Authoritative AUV run persistence: `CanonicalRun`, spans, events, attachments, verification/failure records, attachment bytes, read/write semantics. | OTEL collector setup, command rendering, domain payload interpretation. |
| Run recorder / sink | Receives lifecycle updates, fans out, broadcasts, buffers, or forwards updates. | Canonical storage semantics unless implemented as a store writer adapter. |
| Recording backend | Runtime wiring that composes a store, sinks, local snapshot policy, cleanup policy, and optional mirrors. | Canonical contract ownership. |
| Inspect server adapter | HTTP/WebSocket read/write access over AUV records, live subscriptions, attachment upload/download. | Runtime execution, semantic domain policies, OTEL exporter setup. |
| Telemetry exporter / OTEL mirror | Converts selected AUV lifecycle data to Rust `tracing` / OTEL spans/events with stable correlation attributes. | Replay/eval/inspect source of truth, attachment purpose reads, durable AUV state reconstruction. |

This implies one missing implementation surface if configurable non-file state
is desired: a real in-memory trace store. The existing `MemoryRunRecorder`
captures updates for tests/listeners, but it is not a full `TraceReader` /
`TraceWriter` implementation because it does not provide durable read
semantics, attachment-byte handling, purpose reads, or projection reads.

Recommended store/sink variants:

```text
NoopTrace        accepts lifecycle calls and records nothing
InMemoryStore    full in-process TraceReader/TraceWriter for tests, REPL, short sessions
LocalStore       file-backed AUV store under .auv/runs/{run_id}
InspectHttpSink  forwards RunUpdate + attachment bytes to inspect server write API
BroadcastSink    same-process live viewer updates
CompositeSink    local + inspect server + broadcast + optional mirrors
OtelMirror       optional export path, not an AUV store
```

Noop tracing must not change command semantics. Runtime execution should
produce a canonical `OperationOutcome<T>` directly from the handler/driver path.
Recorders, stores, inspect sinks, and OTEL mirrors consume that outcome and its
events; they are not the source of the synchronous function return value. Store
projections are read-side reconstructions for inspection/session APIs after
recording, not the primary execution result.

The inspect server should be updated to consume the same canonical `RunUpdate`,
`AttachmentRecord`, verification records, `Attributes`, and event shapes as
local recording. Its viewer/read model should render from AUV tracing
projections, not from MCP/CLI-specific invoke result bags.

## New Crate Boundary

Create `crates/auv-tracing`:

```text
auv-tracing
  trace/result contract records
  run/operation/span/event/attachment recording lifecycle
  stream/segment metadata for high-volume media
  recorder/listener traits
  in-process listener fan-out and routing
  store traits and read/write interfaces
  projection records for invoke/session/MCP/inspect reads
  tracing/OTEL mirror hooks and correlation attributes
  no reqwest
  no subscriber/exporter setup
```

Initial responsibilities, not prescribed file names:

```text
identity        RunId, OperationId, TraceId, SpanId, EventId, AttachmentId, DeviceId, SessionId
attributes      Attribute entry wrapper, Attributes map, conversion errors
trace records   RunRecord, OperationRecord, SpanRecord, EventRecord, AttachmentRecord
status          RunState, OperationStatus, TraceStatusCode, TraceFailure
verification    VerificationRecord, VerificationStatus, verification aggregation inputs
recording       RecordingRun, RecordingHandle, RunMutation, CommittedUpdate, RunRecorder traits
storage         TraceReader / TraceWriter traits, store errors
media streams   StreamRecord, SegmentRecord, stream/segment ids and purposes
projection      InvokeProjection, OperationSummaryProjection, InspectProjection traits
otel bridge     stable correlation attribute keys, optional conversion adapter
```

This grouping is AUV's proposed responsibility split, not a copy of the
OpenTelemetry Rust crate layout. OpenTelemetry Rust organizes public APIs by
telemetry domains such as tracing, metrics, logs, context, propagation, and
baggage; AUV still needs its own durable run-store and trajectory-recording
responsibilities.

`auv-tracing-driver` should depend on `auv-tracing`. During migration it may
keep concrete implementation pieces that are not yet split into implementation
crates:

```text
filesystem attachment staging and path layout
local store implementation, unless moved directly into auv-tracing-store
HTTP / inspect-server listener, unless moved directly into auv-tracing-http
recorded operation context
```

The durable target is not "every module records traces itself." Modules that
produce trajectory evidence should go through the same `auv-tracing` lifecycle
APIs. This statement is about shared ownership of recording behavior, not a
fixed hierarchy among observations, spans, events, summaries, and attachments.
This keeps recording behavior reviewable: reviewers should see
command/driver-specific evidence decisions, not repeated hand-rolled run-store
plumbing.

## Protocol Shape Reference

The long-term inspect/runtime protocol can borrow one useful pattern from the
Chrome DevTools Protocol: divide the surface into domains, and let each domain
define typed commands, events, and shared payload types. AUV should not copy
CDP's browser-specific domains, but the structure fits AUV's need for stable
inspection and live-update surfaces.

Candidate AUV protocol domains:

```text
Run          start, finish, fail, read, list
Operation    start, finish, read, listByRun
Span         start, finish, read
Event        append, filter, subscribe
Attachment   stage, upload, read, readByPurpose
Stream       start, appendSegment, finish, readSegments
Projection   invoke, operationSummary, inspectDocument
Session      create, read, attachRun, subscribe
Input        selected delivery path and action-result evidence
Observation  normalized world facts derived from sensors/evidence
```

This protocol shape is separate from the Rust crate layout. It is a stable
wire/read model that future CLI, MCP, inspect server, browser viewer, IDE, and
REPL surfaces can share.

## Reference Model

Records should link to each other with typed references, not bare strings.
The JSON form can use URI-like references so clients can parse and route them
without knowing every ID prefix:

```text
auv://run/{run_id}
auv://run/{run_id}/operation/{operation_id}
auv://run/{run_id}/operation/{operation_id}/input/{input_id}
auv://run/{run_id}/operation/{operation_id}/target-resolution/{resolution_id}
auv://run/{run_id}/operation/{operation_id}/delivery/{delivery_id}
auv://run/{run_id}/operation/{operation_id}/observation/{observation_id}
auv://run/{run_id}/operation/{operation_id}/verification/{verification_id}
auv://run/{run_id}/attachment/{attachment_id}
auv://run/{run_id}/span/{span_id}
auv://run/{run_id}/event/{event_id}
```

Rust records can carry typed refs such as `OperationRef`, `InputRef`,
`DeliveryRef`, `ObservationRef`, and `AttachmentRef`; JSON/MCP projections can
serialize the same refs as `auv://...` strings or as `{ "type", "uri" }`
objects when extra metadata is useful.

Terminology:

- `invoke` is a frontend call. It returns a projection of one
  `OperationOutcome<T>`.
- `operation` is one command invocation inside a run. It owns the command
  output, terminal state, inputs, and operation-local evidence refs.
- `tracing` is the recording substrate: runs, operations, spans, events,
  attachments, streams, committed updates, and read-side projections.
- `delivery` means the input/action was dispatched, accepted, blocked, or
  failed by a driver/backend.
- `observation` means AUV recorded a normalized fact about the external world
  after reading some source. Examples: `focused = true` from AX, OCR text from a
  screenshot region, selected window title, app adapter state, screenshot diff
  region, or a driver feedback fact. It is not the raw bytes and not a success
  claim.
- `verification` means a verifier evaluated observations against asserted
  criteria. It may reference observations as evidence, but it is not the same
  thing as observation.

The reason to keep `observation` as a concept is to avoid forcing verifiers to
parse raw attachments/events every time. Without this layer, semantic
verification either embeds extracted facts inside verification records, making
them hard to reuse, or pushes large extracted data into attributes/events.
`observation` is optional: commands that do not inspect external state do not
need to produce one, and default invoke output should not expand observations
unless a detail/inspect mode asks for them.

Candidate observation shape:

```rust
pub struct ObservationRecord {
  pub observation_id: ObservationId,
  pub operation_id: OperationId,
  pub input_id: Option<InputId>,
  pub subject_ref: Option<SubjectRef>,
  pub target_ref: Option<TargetRef>,
  pub kind: ObservationKind,
  pub source: ObservationSource,
  pub observed_at_millis: TimestampMillis,
  pub value: ObservationValue,
  pub confidence: Option<Confidence>,
  pub attachment_refs: Vec<AttachmentRef>,
  pub event_refs: Vec<EventRef>,
  pub attributes: Attributes,
}
```

`ObservationValue` should be typed or schema-referenced. It should not become a
catch-all JSON dump. Large evidence remains in attachments or streams; events
remain sparse timeline facts.

## Telemetry Export Path

The recorder should have two outputs for the same lifecycle:

```text
durable store  inspect / replay / dataset / eval source of truth
tracing mirror performance analysis / flow visualization / external OTEL tools
```

The tracing mirror must use stable correlation attributes:

```text
auv.run_id
auv.trace_id
auv.span_id
auv.parent_span_id
auv.event_id
auv.command_id
auv.operation
auv.attachment_id
auv.stream_id
auv.segment_id
auv.verification.status
auv.verification.domain
auv.verification.check
```

Recorder lifecycle calls should be able to emit Rust `tracing` spans/events
without requiring callers to hand-roll duplicate instrumentation:

```rust
let command = recorder.start_span("auv.command.invoke", attrs)?;
// Internally mirrored as a tracing span with auv.run_id / auv.span_id.
recorder.event(&command, "auv.verification", attrs)?;
// Internally mirrored as a tracing event with the same correlation fields.
recorder.finish_span(command, status)?;
```

Applications may install `tracing-subscriber`, `tracing-opentelemetry`, OTLP
exporters, or plain formatting layers. If no subscriber/exporter is installed,
the AUV durable recording path still works. This follows the Rust ecosystem
split: libraries emit structured spans/events; binaries decide where telemetry
goes.

Rust OpenTelemetry's exporter model should remain an edge concern. A binary or
server may configure an SDK provider, span processor, and exporter such as
stdout or OTLP. `auv-tracing` should expose correlation fields and optional
conversion hooks; it should not require `opentelemetry`, `opentelemetry-otlp`,
or `tracing-opentelemetry` for callers that only need AUV records.

Feature/crate split:

```text
auv-tracing default        core records, traits, no-op/memory sink contracts
auv-tracing-store-local    filesystem store and physical artifacts/ compatibility
auv-tracing-store-memory   full in-process store for tests/REPL/short sessions
auv-tracing-http           inspect server read/write adapter and client
auv-tracing-otel           optional Rust tracing / OTEL mirror
auv-driver only            allowed; drivers may be used without auv-tracing
```

Driver and app crates should not be forced to depend on concrete inspect or
OTEL implementations. A driver-only consumer should be able to call typed
driver APIs and ignore AUV tracing. A runtime, product CLI, or integration crate
can opt into tracing by passing a trace handle or recording context.

## Core Types

Use OTEL-shaped names, but keep AUV attributes JSON-compatible. Public schema
types must not expose raw strings or primitive aliases for values with
invariants. Use private-field newtypes or closed enums for IDs, content types,
digests, schema names, purposes, failure codes, timestamps, durations, and
revisions.

Candidate primitive pattern:

```rust
pub struct RunId(Uuid);
pub struct OperationId(Uuid);
pub struct ContentType(mime::Mime);
pub struct AttachmentPurpose(Name);
pub struct FailureCode(Name);
pub struct TimestampMillis(u64);
pub struct Revision(NonZeroU64);
pub struct RunSequence(NonZeroU64);

pub enum Digest {
  Sha256([u8; 32]),
}
```

Attributes need one owning type and policy:

```rust
pub struct Attributes {
  entries: BTreeMap<AttributeKey, serde_json::Value>,
  dropped_count: u32,
}

pub struct AttributeKey(String);

pub struct AttributePolicy {
  pub max_entries: usize,
  pub max_key_bytes: usize,
  pub max_value_bytes: usize,
  pub max_total_serialized_bytes: usize,
  pub max_depth: usize,
  pub max_array_len: usize,
}
```

This keeps the store/read path JSON-compatible without letting every producer
create arbitrary keys, secrets, deeply nested values, high-cardinality values,
or unbounded payloads. Rejected, redacted, flattened, or dropped values should
increment `dropped_count` or an equivalent typed limit record.

OpenTelemetry adapters must choose an explicit conversion strategy because OTEL
attribute values do not support arbitrary JSON objects, `null`, or every mixed
array shape. Acceptable strategies include reject, drop, flatten, or stringify;
the adapter should record dropped/converted counts.

Keep timestamps as `*_millis` in AUV records for the existing store/read path.
This does not prevent an OTLP adapter from converting to OTEL timestamp types.

## External Reference Grounding

The next schema pass should use mature protocol/reporting designs as the
baseline, not the current AUV implementation. The current code is useful for
migration risk only.

Reference patterns to carry forward:

- OpenTelemetry keeps instrumentation, readable span data, processors, and
  exporters separate. Exporters receive telemetry snapshots; they do not own
  the canonical application state. Resource identity is also modeled as
  attributes plus schema metadata, not as backend-specific fields.
- Chrome DevTools Protocol splits protocol surface by domains. A domain owns
  commands, events, and types; command responses are not timeline dumps.
- Playwright records traces as a debugging artifact that can include actions,
  snapshots, screenshots, network activity, and assertions when configured
  through the test runner. The trace is separate from a normal API return
  value.
- Allure test-result files attach files to a test or step with a small object:
  human name, source file, and content type. It does not put free-form summaries
  on each attachment; richer human presentation is derived by the report layer.
- Playwright and Allure also show a useful naming split: APIs may accept a
  `contentType`, serialized files may call the same value `type`, and HTTP/HAR
  formats may say `mimeType`. The shared concept is the MIME-like
  representation of the bytes, not the AUV semantic purpose. AUV's candidate
  public field name is `content_type` to avoid confusing byte format with
  screenshot/video/OCR stream categories.

Sources:

- OpenTelemetry Resource Data Model:
  https://opentelemetry.io/docs/specs/otel/resource/data-model/
- OpenTelemetry Trace SDK:
  https://opentelemetry.io/docs/specs/otel/trace/sdk/
- Chrome DevTools Protocol overview:
  https://chromedevtools.github.io/devtools-protocol/
- Playwright tracing API:
  https://playwright.dev/docs/api/class-tracing
- Allure test result file:
  https://allurereport.org/docs/how-it-works-test-result-file/

## Attachment Record Shape Reset

Rename persisted user-facing evidence files from artifact to attachment at the
new contract layer. The previous draft carried over too much of the existing
artifact schema. In particular, attachment-level `summary`, mixed `kind` /
`role` naming, and arbitrary string `api_version` setters should not become the
new public contract.

Candidate shape for review, not approved:

```rust
pub struct AttachmentRecord {
  pub schema_version: String,
  pub attachment_id: AttachmentId,
  pub operation_id: OperationId,
  pub producer_span_id: SpanId,
  pub producer_event_id: Option<EventId>,
  pub purpose: AttachmentPurpose,
  pub content_type: ContentType,
  pub byte_length: Option<u64>,
  pub sha256: Option<String>,
  pub attributes: Attributes,
}
```

The storage directory can continue using `artifacts/` during migration if
changing paths would create unnecessary churn. The contract type and new public
fields should say `attachment`.

Field rules:

- `purpose` is the AUV semantic purpose of the attachment in a run, for example
  `display_screenshot`, `post_action_screenshot`, `ocr_result`,
  `verification_evidence`, or `operation_result`.
- `content_type` is the representation format of the bytes, normally the same
  value that would be used in an HTTP `Content-Type` header, for example
  `image/png`, `application/json`, or
  `application/vnd.auv.ax-snapshot+json`. This does not mean only audio/video
  media is supported; it is the MIME content type.
- `kind` should be reserved for Rust enum discriminants or protocol variant
  tags. If a persisted file is being selected by semantic purpose, use
  `purpose`.
- `summary` should not be a canonical attachment field. If a viewer needs a
  caption, derive it from `purpose`, `content_type`, command context, or a
  renderer-side presentation projection.
- `path` should not be part of the public attachment record. A local store must
  derive its private byte locator from the authenticated run/attachment identity
  and fixed layout rules. Inspect/write APIs must not accept caller-provided
  paths that can overwrite run metadata or escape the store layout.
- `operation_id`, `producer_span_id`, and `producer_event_id` tie the attachment
  to one command invocation and trace producer. `command_id` alone is not enough
  because one run may invoke the same command multiple times.
- Preview/rendering should normally be selected by `content_type`, not by
  `purpose`. `purpose` can choose semantic grouping and filtering, while vendor
  content types can opt into specialized viewers for structured AUV payloads.
- The serialized version field should be produced by the record type, not
  supplied by every caller. A future implementation can keep a serialized
  `record_format_version` or `schema_version` field for independent JSONL
  reads while exposing it through a trait or associated constant such as
  `VersionedRecord::RECORD_FORMAT_VERSION`.

This leaves one naming decision open: keep historical `api_version` for wire
compatibility, or rename new records to `record_format_version` /
`schema_version` because these versions describe persisted record shapes rather
than an HTTP API. OTEL `schema_url` is a separate concept for telemetry semantic
conventions and should not be used as the local file-format version.

## Operation Record Requirement

`RunRecord` is the container lifecycle. It cannot also represent each command
invocation once REPL/script sessions and multi-command runs are supported.
AUV needs a first-class `OperationRecord`:

```rust
pub struct OperationRecord {
  pub record_format_version: RecordFormatVersion,
  pub run_id: RunId,
  pub operation_id: OperationId,
  pub command_id: CommandId,
  pub producer_span_id: SpanId,
  pub started_at_millis: TimestampMillis,
  pub finished_at_millis: Option<TimestampMillis>,
  pub status: OperationStatus,
  pub outcome_ref: Option<OperationOutcomeRef>,
  pub attributes: Attributes,
}
```

`OperationStarted` and `OperationFinished` must be part of the mutation
protocol. `RunStatus` / `RunState` describes whether the run as a whole is
open, finished, failed, or cancelled. `OperationStatus` describes one command
invocation. `InvokeProjection` and MCP invoke should be projections of one
`OperationOutcome<T>` / `OperationRecord`, not of the entire run.

## Review Findings From This Pass

This document is still a design draft. The current review surfaced these
problems that must be resolved before implementation:

1. The first invoke projection draft copied too much from current code:
   `summary`, `report`, `events`, `boundary`, and `recommended_next_action`
   either mix presentation with data or move policy into the runtime contract.
2. `result` and `failure` cannot be anonymous JSON bags. JSON/MCP projections
   need typed envelopes, while Rust operation APIs can expose concrete types.
3. `status` and `verification_status` are separate axes. Command completion is
   not semantic success, and semantic verification failure is not necessarily a
   runtime failure.
4. Verification cannot be a flat list of `scope/status/method` objects. It
   needs a controlled taxonomy: domain, check, method, assertion/criteria,
   typed evidence refs, reason codes, timing, attempts, and aggregation.
5. Attachment `summary` should not be canonical. Human captions belong in
   renderers or read-side presentation projections.
6. Attachment semantic purpose must not be named `role` because AUV also deals
   with accessibility/ARIA/AX roles. New public schema should use `purpose`.
7. Byte representation should be named `content_type` in AUV public schema.
   The term maps to MIME media type / HTTP `Content-Type`; it is not the same
   thing as stream family, track kind, or attachment purpose.
8. Stream records need separate `purpose`, `track_kind`, optional default
   `content_type`, and optional codec/timebase metadata. Attachment
   `content_type` is authoritative for stored bytes; segment `content_type` is
   only an override or inline-payload marker.
9. Attachment records must not expose store paths. The store derives private
   byte locators from run/attachment identity; write APIs should reject or avoid
   caller-provided paths.
10. A projection needs an invocation identity such as `operation_id` plus a
    `producer_span_id`. `command_id` names a command definition and cannot
    disambiguate repeated calls in one run.
11. Input delivery and target resolution are execution facts, not semantic
    verification. They can supply evidence for verification, but they should
    not be aggregated as semantic success.
12. Failure and delivery records need `SafetyHint` entries. Storage and
    recorder failures may be transient even when a whole-command retry is
    unsafe because input may already have affected the UI or application.
13. Versioning still needs a deliberate split between persisted record format
   version, domain payload schema, wire API version, and OTEL `schema_url`.
14. Store, recorder/sink, exporter, and inspect adapter are separate concepts.
    Exporters should never become the canonical state store.

## Operation Outcome And Invoke Projection Reset

Status: unresolved. The previous draft copied too much of the existing
`InvokeResult` shape and introduced `summary`, `backend`, `report`, `events`,
and `boundary` as if they were stable public fields. That was premature.

The stable direction is narrower:

- The executor is authoritative for operation completion. Every command
  invocation directly produces exactly one canonical `OperationOutcome<T>`,
  including its `operation_id` and terminal classification.
- Trace sinks, recorders, stores, and read-side projections consume
  `OperationOutcome<T>` and observations. They do not construct or modify the
  function return value.
- `invoke` is a command-style frontend. Its JSON/MCP return value should be a
  projection of the operation outcome, not the full trace event stream.
- The trace store still records spans, events, attachments, failures, and
  correlation data. Callers that need the timeline should read or subscribe to
  the run by `run_id`.
- Human output is a renderer concern. Presentation text and report tables
  should not become JSON/MCP contract fields.
- Backend choices are attributes on the run/span/event/result where that choice
  was made, not a top-level invoke field.
- Result confidence should be expressed through verification records, not
  through a separate `boundary` / `claim` abstraction.

The outcome/projection model needs a fresh design around these questions:

| Question | Current leaning |
|---|---|
| What is the minimal final result shape? | A projection of `OperationOutcome<T>`: `run_id`, `operation_id`, `command_id`, terminal state, typed output or failure, execution evidence refs, attachments, and optional semantic verification. |
| Should verification be singular? | It should be an envelope with checks/criteria and aggregation, not a bare string and not a flat list mixed with delivery facts. |
| Should there be an aggregate verification status? | Probably yes, but it must be derived from records by explicit rules. |
| Should invoke include the full event stream? | No by default. Inspect/read/subscribe APIs own timelines. |
| Should `summary` be stored? | No as a canonical field. Renderers may derive presentation text. |
| Should `report` be public JSON/MCP? | No. Promote meaningful report data into typed `result`; keep table/field layout renderer-only. |
| Should retry guidance exist? | Not in core invoke projection. AUV should expose facts, evidence, stable reason codes, and typed failures; agent/tool policy can decide whether to retry, reobserve, adjust parameters, or ask the user. |

Candidate execution shape for review, not approved:

```rust
pub struct OperationOutcome<T> {
  pub run_id: RunId,
  pub operation_id: OperationId,
  pub command_id: CommandId,
  pub started_at_millis: TimestampMillis,
  pub finished_at_millis: TimestampMillis,
  pub terminal: OperationTerminal<T>,
}

pub enum OperationTerminal<T> {
  Succeeded {
    output: T,
    evidence: CompletionEvidence,
  },
  Failed {
    failure: OperationFailure,
  },
  Cancelled {
    reason: CancellationReason,
  },
}

pub enum CompletionEvidence {
  Local,
  Delivered {
    delivery: DeliverySuccess,
  },
  SemanticallyVerified {
    delivery: DeliverySuccess,
    verification: VerificationSuccess,
  },
}

pub enum OperationFailure {
  Runtime(RuntimeFailure),
  Delivery(DeliveryFailure),
  Verification {
    delivery: DeliverySuccess,
    verification: VerificationFailure,
  },
}
```

This closed state machine replaces the optional-field bag for newly produced
execution outcomes. It makes invalid combinations unrepresentable: success
always carries output, failure does not carry success output, semantic
verification cannot appear without the evidence needed to support it, and
activation-only/delivery-only completion is not confused with semantic success.

JSON/MCP `InvokeProjection` should be a transport projection of
`OperationOutcome<T>` plus recorded refs. It may flatten fields for caller
convenience, but the legal combinations must be inherited from
`OperationTerminal`, not reconstructed from independent `Option` fields.

`result` should not be an unlabelled plain object. It is the command's typed
payload projection with an explicit payload kind and schema/version identity:

```rust
pub struct CommandResultProjection {
  pub result_type: String,
  pub schema: Option<ResultSchemaRef>,
  pub value: serde_json::Value,
}
```

This keeps JSON/MCP flexible without turning the field into an anonymous bag.
Typed Rust operation APIs can expose stronger concrete types; the JSON/MCP
projection carries the same payload through an explicitly named envelope.

`failure` should also be structured, not a plain object. It should describe why
the command/runtime could not complete normally: stage, code, message,
transience/effect facts, and optional cause/evidence refs. It should not
absorb verification failures from a command that did run.

Candidate shape:

```rust
pub struct FailureProjection {
  pub stage: FailureStage,
  pub code: FailureCode,
  pub message: String,
  pub transience: FailureTransience,
  pub safety_hints: Vec<SafetyHint>,
  pub cause: Option<FailureRef>,
  pub evidence: Vec<EvidenceRef>,
  pub attributes: Attributes,
}
```

Retry guidance should not be part of canonical failure. Recorder/store failures
may happen after input was already delivered. The failure projection should
record factual classifications such as `stage`, `transience`, and
`safety_hints`. Higher-level callers can combine those facts with verification
records and their own policy, but AUV should not invite blind replay of
side-effecting commands.

Candidate safety hint shape:

```rust
pub struct SafetyHint {
  pub kind: SafetyHintKind,
  pub state: SafetyHintState,
  pub basis: SafetyHintBasis,
  pub evidence: Vec<EvidenceRef>,
  pub message: Option<String>,
}
```

Safety hints are deliberately an array. A single input delivery or failure can
carry several weak-but-useful facts: input was dispatched, side effects are
possible, no semantic confirmation was observed, or a recorder failure happened
after execution. These hints are not recommendations such as “retry”; they are
classified facts that a caller or agent policy may interpret.

Important constraints for the next design pass:

- `status` describes command/runtime execution, not necessarily semantic UI
  success.
- `operation_id` identifies this command invocation. `command_id` identifies
  the command definition. `producer_span_id` links the projection back to the
  trace span that produced it.
- `delivery` records input/action delivery facts such as backend, attempts,
  disturbance, fallback reason, and delivery status. It is orthogonal to
  semantic verification.
- `verification_status` is a derived aggregate over verification records. It is
  meaningful only for semantic assertions over observed state, and does not
  overwrite `status` or `delivery`.
- A completed command can have `verification_status = failed` or
  `inconclusive`. A failed command can have no semantic verification because the
  operation never reached the point where verification was possible.
- `verification` records need their own taxonomy. They should not be a loose
  set of `scope` / `method` strings. Target resolution, input delivery, and
  observed UI feedback can provide evidence, but semantic verification is the
  claim that observed world state satisfies asserted criteria.
- A command that did not request or perform verification should not imply
  success. It may project `verification_status = not_requested` with an empty
  verification list, or omit verification only when command metadata makes that
  absence unambiguous. This remains undecided.
- `failure` is for runtime/handler/recording failures. A command may complete
  execution while verification records say the semantic outcome failed or is
  inconclusive.
- The aggregate verification status must not hide the underlying records.
  Agents need the per-step records to decide whether to retry, adjust
  selectors, reobserve, switch delivery method, or stop.
- `recommended_next_action` should not be part of core AUV tracing or invoke
  output. AUV is an automation/runtime framework; it can expose evidence,
  failure codes, verification facts, and replayable inputs, but advice belongs
  to a separate agent policy layer or viewer affordance.

Mapping from the current model should be re-evaluated:

| Current field | Direction |
|---|---|
| `signals` | Do not mechanically rename. Classify each field per command as typed `result`, `delivery`, diagnostic, limit, evidence level, or small attribute. |
| `notes` | Record as trace events or diagnostics; do not default into invoke output. |
| `known_limits` | Preserve semantic limits as typed limit/diagnostic records when they affect safety or interpretation; project only selected diagnostics if an explicit option asks for them. |
| `verification` string | Replace with evidence-backed verification records; do not rename it to `boundary`. |
| `artifacts` | Replace public terminology with `attachments`. |
| `report` | Keep renderer-only or promote meaningful content to typed `result`. |

Example projection for an action command, not approved. The verification object
is intentionally shown as an envelope rather than a flat `scope/status/method`
array:

```json
{
  "run_id": "...",
  "operation_id": "op_0001",
  "producer_span_id": "span_0001",
  "status": "completed",
  "command_id": "input.pressButton",
  "result": {
    "result_type": "input.delivery",
    "schema": {
      "name": "auv.input.delivery",
      "version": "v1alpha1"
    },
    "value": {
      "delivery": {
        "status": "dispatched",
        "method": "ax_press"
      }
    }
  },
  "attributes": {},
  "inputs": [
    {
      "input_id": "input_0001",
      "kind": "activation",
      "action_ref": "auv://run/run_0001/operation/op_0001/input/input_0001/action/action_0001",
      "target": {
        "subject_ref": "auv://run/run_0001/operation/op_0001/subject/subject_0001",
        "resolved_target_ref": "auv://run/run_0001/operation/op_0001/candidate/candidate_0001",
        "resolution_ref": "auv://run/run_0001/operation/op_0001/target-resolution/target_resolution_0001"
      },
      "delivery": {
        "delivery_ref": "auv://run/run_0001/operation/op_0001/delivery/delivery_0001",
        "status": "delivered",
        "backend": "ax",
        "method": "ax_press",
        "safety_hints": [
          {
            "kind": "side_effect",
            "state": "possible",
            "basis": "input_dispatched"
          }
        ]
      },
      "observation_refs": [
        "auv://run/run_0001/operation/op_0001/observation/observation_0001"
      ]
    }
  ],
  "verification_status": "failed",
  "verification": {
    "schema_version": "auv.verification.v1alpha1",
    "status": "failed",
    "aggregation": "all_required",
    "checks": [
      {
        "id": "ver_0001",
        "domain": "semantic_outcome",
        "check": "state_changed",
        "status": "failed",
        "method": "ax_snapshot_diff",
        "reason": { "code": "no_observed_state_change" },
        "subject": {
          "type": "application_state",
          "uri": "auv://run/run_0001/operation/op_0001/subject/state_region_0001"
        },
        "evidence": [
          {
            "type": "observation",
            "uri": "auv://run/run_0001/operation/op_0001/observation/observation_0002",
            "purpose": "post_state"
          }
        ]
      }
    ]
  },
  "attachments": [],
  "failure": null
}
```

Each `inputs[]` entry describes an input/action requested by the operation. A
targeted input may include a resolved target; an untargeted input or read-only
operation may omit `target`. The `resolution_ref` URI points to a separate
execution evidence record, not to semantic verification:

```rust
pub struct TargetResolutionRecord {
  pub target_resolution_id: TargetResolutionId,
  pub operation_id: OperationId,
  pub subject_ref: SubjectRef,
  pub target_ref: TargetRef,
  pub method: TargetResolutionMethod,
  pub resolved_at_millis: TimestampMillis,
  pub evidence_refs: Vec<EvidenceRef>,
  pub attributes: Attributes,
}
```

This record answers “which concrete target did this operation use, and how was
it selected?” It does not claim the operation semantically succeeded.

Putting target resolution under `inputs[]` keeps the default projection aligned
with automation semantics:

- one operation may have zero, one, or many inputs,
- each input may have its own target resolution, delivery attempt, fallback,
  and observations,
- commands without a UI target do not fabricate target-resolution fields,
- inspect/detail views can expand refs into full execution evidence records.

`observation_refs` are not verification results. They identify facts AUV
captured, for example “post-click AX tree snapshot” or “focus changed event.”
Verification checks may later cite those observations as evidence and decide
whether the asserted semantic condition passed, failed, errored, or remained
inconclusive.

Verification naming rules for the next pass:

- `status` values should use assertion/reporting language:
  `passed`, `failed`, `error`, `skipped`, `inconclusive`.
- `domain` partitions semantic assertion context, for example `precondition`,
  `semantic_outcome`, `postcondition`, and `artifact_integrity`. Target
  resolution, input delivery, and observations can provide evidence, but they
  should not be aggregated as semantic verification.
- `check` describes what was asserted, for example `state_changed`,
  `value_equals`, `element_exists`, `window_frontmost`, or `focused`.
- `method` describes how the check was evaluated, for example
  `ax_snapshot_diff`, `ocr_observation`, `visual_diff`, or an app adapter
  query.
- First-party `domain` and status values should be small controlled enums.
  Checks, methods, and reason codes may be extensible, but third-party values
  should be namespaced.
- Evidence should use typed references, not bare string arrays. Inspect and
  replay readers need to know whether a reference points to an attachment,
  observation, trace event, candidate, input action, or app state snapshot.
- Input delivery confirmation is not semantic success. Delivery facts can
  contribute verification checks, but semantic outcome must be asserted through
  criteria or checks tied to observed state.

## JSON / MCP Shape

`auv invoke --json` and MCP invoke should expose the same final-result
projection, but the exact fields are now unresolved pending the verification
design reset above.

Default human output stays result-first. It may use command metadata, `result`,
attributes, attachments, verification, and failure to render concise text.
That rendered text is not part of the canonical JSON/MCP shape.

`--detail` may add human sections such as:

```text
Verification
Attachments
Attributes
```

Detailed event timelines should be read through inspect APIs, for example:

```text
auv inspect <run_id> --json
MCP run_inspect
MCP run_events
MCP subscribe_run
```

An explicit future option such as `--include-events=diagnostics` may include a
bounded subset of events in invoke output. The default remains event-free so
command results do not become timeline dumps.

## Operation Summary

Persisted operation-summary records should change with this contract, but the
exact shape is blocked on the invoke projection reset. The existing `signals`
field should become `attributes`. Operation summaries should store a final
projection, not duplicate the full event stream.

Candidate shape for review, not approved:

```rust
pub struct OperationSummaryRecord {
  pub record_format_version: RecordFormatVersion,
  pub run_id: RunId,
  pub operation_id: OperationId,
  pub command_id: CommandId,
  pub terminal: RecordedOperationTerminal,
  pub attributes: Attributes,
  pub inputs: Vec<OperationInputProjection>,
  pub verification: Option<VerificationProjection>,
  pub attachments: Vec<AttachmentProjection>,
}
```

This is an intentional breaking wire-shape change for the invoke/session
summary projection.

## Event Stream Access

Not returning events from `invoke --json` does not reduce the observability
value of trace recording. It separates result consumption from timeline
inspection:

| Surface | Default purpose | Event behavior |
|---|---|---|
| `auv invoke --json` | Final command result for scripts and MCP callers. | No full event stream by default; returns `run_id` for follow-up reads. |
| MCP `invoke` | Final command result for agent/tool calls. | Same projection as CLI JSON. |
| `auv inspect <run_id> --json` | Read the recorded run for inspection. | Can return spans/events/attachments with filtering and pagination. |
| MCP `run_events` | Programmatic event reads. | Paged/filterable event stream. |
| MCP/WebSocket `subscribe_run` | Live viewer or REPL follow mode. | Incremental `RunUpdate` stream. |
| OTEL mirror | External observability. | Exported spans/events with stable AUV correlation fields. |

This boundary keeps the trace store useful for observability, inspect, replay,
eval, and debugging while keeping invoke output stable for command consumers.

## High-Volume Media And REPL Sessions

Attributes must stay small. Events must remain sparse lifecycle facts. Images,
video frames, OCR dumps, model detections, frame batches, and recordings should
be stored as attachments or future stream segments, not embedded directly in
attributes/events or emitted as one event per frame.

Proposed high-volume records:

```rust
pub struct StreamRecord {
  pub schema_version: String,
  pub stream_id: StreamId,
  pub run_id: RunId,
  pub span_id: SpanId,
  pub purpose: String,
  pub track_kind: StreamTrackKind,
  pub content_type: Option<ContentType>,
  pub codec: Option<String>,
  pub timebase: StreamTimebase,
  pub started_at_millis: u64,
  pub finished_at_millis: Option<u64>,
  pub attributes: Attributes,
}

pub struct SegmentRecord {
  pub schema_version: String,
  pub segment_id: SegmentId,
  pub stream_id: StreamId,
  pub index: u64,
  pub timestamp_millis: u64,
  pub duration_millis: Option<u64>,
  pub attachment_id: Option<AttachmentId>,
  pub content_type: Option<ContentType>,
  pub attributes: Attributes,
}
```

Stream content rules:

- Streams should be mostly homogeneous. Use separate child streams for
  screenshots, OCR batches, detection batches, and video chunks instead of one
  mixed stream.
- `purpose` explains why the stream exists, such as `observe_screen`,
  `inspect_ui`, or `replay_evidence`.
- `track_kind` describes the stream family, such as `screenshot_sequence`,
  `video`, `ocr`, `detection`, `event`, or `observation_group`.
- `StreamRecord.content_type` is only the default byte/payload representation
  for stream segments. It is optional because grouping streams or pure metadata
  streams may not have one.
- `AttachmentRecord.content_type` remains authoritative for stored bytes.
- `SegmentRecord.content_type` is an optional override, used only when the
  segment carries inline payload or intentionally differs from the stream
  default.
- If all three appear, precedence is: segment inline/override for that segment,
  attachment content type for referenced bytes, stream content type only as the
  default contract. Readers should not need to reconcile three required fields.

Stream/event relationship:

```text
event stream.started       sparse lifecycle marker
stream record              purpose/track/default-content/timing metadata
segment records            frame or chunk index/timestamp metadata
attachments                actual screenshots/video chunks/OCR JSON/detections
event stream.finished      sparse lifecycle marker
```

This keeps inspect viewers able to list and filter streams by purpose, time,
span/event linkage, content type, and segment id without loading every frame by
default.

Interactive REPL or script-authoring frontends should be modeled as sessions
that contain multiple runs or a long run with multiple command spans. The
recording contract should preserve:

```text
session_id
turn_id or cell_id
script/source attachment
command span
result projection
state/observation snapshot
replay input
```

This does not require implementing a REPL in this slice. It does mean the
tracing contract should avoid assuming every frontend is one `invoke` process
with one terminal result.

## Recording Ownership Rule

AUV modules must not create independent run/span/event/attachment persistence
paths when the data belongs to the durable tracing trajectory. The reusable
recording substrate owns:

- starting and finishing runs and spans,
- assigning run/span/event/attachment IDs,
- recording events and verification records with attributes and attachment
  links,
- routing updates to listeners,
- writing/reading records through store traits,
- attaching operation projections and verification records to the recorded
  trajectory.

Domain modules own only domain decisions:

- which observation happened,
- which attributes describe it,
- which attachment was produced,
- which verification record, diagnostic, or limit should be emitted,
- which verification or operation result is warranted.

If a module needs a new recording behavior, it should extend the shared
`auv-tracing` API or an implementation crate rather than writing a local
recording path. Local one-off recording is allowed only for tests or temporary
migration shims, and should carry a `TODO:` / `NOTICE:` marker naming the
shared API it will move to.

## Repository Audit And Cleanup Map

Audit date: 2026-07-18. Scope included `auv-tracing-driver`, `auv-cli-invoke`,
session API, inspect server/model, Netease, scan/view-parser crates, runtime
contracts, and game integrations.

The cleanup rule is:

```text
Move durable trajectory envelope / lineage / storage mechanics into auv-tracing.
Keep domain payloads where they express app, scan, recognition, or game facts.
Retire local duplicate recording paths once the shared substrate exists.
```

| Area | Current surface | Current responsibility | Direction |
|---|---|---|---|
| Trace records | `RunRecordV1Alpha1`, `SpanRecordV1Alpha1`, `EventRecordV1Alpha1`, `ArtifactRecordV1Alpha1` in `crates/auv-tracing-driver/src/trace.rs` | Canonical trace record contract | Move to `auv-tracing`; rename public artifact terminology to attachment when feasible. |
| Run updates | `RunUpdate` in `crates/auv-tracing-driver/src/recording/update.rs` | Live/delta mutation shape | Move to `auv-tracing`; make it the single mutation/update contract. |
| Run reducer | `RecordingRun` in `crates/auv-tracing-driver/src/run_builder.rs`; `apply_update` in `crates/auv-inspect-server/src/server.rs` | Two independent reducers for the same run/span/event/artifact state | Consolidate into one shared reducer in `auv-tracing`; inspect-server routes remain implementation-side. |
| Store contract | `CanonicalRun`, `LocalStore`, `read_run`, `replace_run_snapshot`, `stage_artifact_*` in `crates/auv-tracing-driver/src/store.rs` | Filesystem store plus generic read/write semantics | Move `CanonicalRun` and store traits/semantics to `auv-tracing`; keep filesystem `LocalStore` in an implementation crate or adapter module. |
| Recorder fan-out | `RunRecorder`, `MemoryRunRecorder`, `BroadcastRunRecorder`, `CompositeRunRecorder`, `InspectServerRunRecorder` in `crates/auv-tracing-driver/src/recording/recorder.rs` | Listener/sink fan-out and HTTP inspect write client | Move traits, memory/noop/composite/broadcast to `auv-tracing`; keep HTTP client implementation outside core. |
| Recording facade | `RunRecordingBackend`, `RecordingHandle`, `RecordedArtifacts` in `crates/auv-tracing-driver/src/recording/backend.rs` | Joins run lifecycle, store persistence, listener fan-out, artifact staging | Move lifecycle/facade semantics into `auv-tracing`; concrete local wiring may live in an implementation crate. |
| Invoke output bag | `InvokeCommandOutput::{summary, backend, signals, notes, known_limits, verification, report, artifacts}` in `crates/auv-cli-invoke/src/command.rs` | Ad hoc result/evidence/presentation surface inherited from current renderer needs | Replace public projection with typed `result`, `attributes`, verification records/status, attachments, and failure. Record notes/limits/backend decisions into the trace, not default invoke output. Keep report-style data renderer-only or promote it to typed `result`. |
| Invoke result projection | `InvokeResult::{signals, artifacts, artifact_paths, output_summary}` in `crates/auv-cli-invoke/src/models/invoke_result.rs` | Return object duplicates trace state, leaks local paths, and carries presentation text as data | Replace with `OperationOutcome<T>` plus a JSON/MCP projection derived from that outcome. Store/read projections should match it after recording, but the synchronous return must not depend on the store. |
| Invoke recording wrapper | `crates/auv-cli-invoke/src/recorded.rs` | Records backend/notes/verification/limits as message-only events; does not record `signals` into trace | Promote attributes, diagnostic events, verification records, and attachments through the shared recorder. The invoke return value should come from the execution outcome, while the recorder consumes the same facts. |
| Operation summary | `OperationSummaryRecord`, `OperationSummaryCache` in `crates/auv-cli-invoke/src/summary.rs` | Separate summary projection with `signals` only | Replace with tracing-derived operation projection once recorder owns attributes, verification records/status, attachments, typed result payload, and failures. |
| Session post-run append | `persist_operation_summary`, `persist_operation_result` in `src/api/session_service/*_store.rs` | Appends JSON artifacts by `read_run -> stage -> replace_run_snapshot`, bypassing normal updates | Replace with a shared append/stage API that emits the same durable updates/listener notifications as normal recording. |
| MCP invoke output | `src/mcp.rs` raw `signals`, simplified `artifacts`, separate `artifact_paths` | Diverges from CLI JSON and session summary | Return the same final-result projection as CLI/session: `result`, attributes, verification records/status, attachments, failure, and `run_id`. Event timelines stay behind inspect/read/subscribe tools. |
| Netease artifact-dir bridge | `Inputs.artifact_dir`, `source_artifacts`, command `artifacts`, `recording.rs` lineage bridge | App-local path-string artifact mirror and store fallback bridge | Keep CLI compatibility temporarily; migrate path strings to attachment refs and retire app-local store bridge. |
| Netease interaction evidence | `PlaylistSidebarScan.interaction_events`, `PlaylistSelectResult.steps`, `PlaylistPlayResult.steps` | Domain command result doubles as execution trace | Keep domain result summaries; emit durable execution steps as tracing events/spans. |
| Netease diagnostics/limits | `ParserDiagnostic`, `diagnostics`, `known_limits`, `parser_notes` | Mixed domain parser notes, user limits, and persistence errors | Keep parser/domain diagnostics where local; durable limits and storage failures become tracing events/attributes with typed severity/classification. |
| Netease observation artifacts | `write_observation_artifacts`, `finish_artifacts` in sidebar live parser | Writes screenshots/overlay/OCR/observation JSON directly to `artifact_dir`; write failures become parser diagnostics | Route through tracing attachments or future streams/segments; storage failures should not masquerade as parser errors. |
| Runtime contracts | `OperationResult`, `VerificationResult`, `RecognitionResult`, `ObservationSnapshot`, `CandidateRef` in `src/contract.rs` | Runtime semantic contracts and durable domain records | Keep in runtime contract for now; migrate only lineage/reference primitives (`ArtifactRef`) and generic read mechanics to tracing. |
| Observation snapshots | `ObservationSnapshot`; `ScrollScanArtifact.snapshots`; `build_page_observation_snapshot` | Converged observation record exists, but often nested and still carries path-string source artifacts | Make `ObservationSnapshot` a first-class recorded attachment purpose; keep `ScrollScanArtifact` as collection evidence. |
| Scan-local lifecycle | `auv-scan::{LifecycleEvent, TransitionEvidence, AssociationDiagnostic, SceneDiagnostic}` | In-memory/fixture scan evidence models, no durable wire | Keep as scan domain models; when persisted, map to tracing events/attachments instead of creating a parallel store. |
| View parser evidence | `ViewEvidenceNode`, `ViewObservation`, `ParserDiagnostic` in `auv-view` | Domain parsing/reconstruction evidence and diagnostics | Keep parser framework terms; only durable recording of parser notes/evidence should go through tracing. |
| Game run-read loops | Minecraft/osu/Balatro repeated `run_read` role scans and lineage wrappers | Domain payload read projections over artifacts | Add object-safe attachment listing/byte reads plus typed helper functions for purpose+schema-version validation; keep game payload structs. |
| String artifact refs | Minecraft `screenshot_artifact_ref`, scan `source_artifacts`, Netease path refs | String/path lineage references to recorded files | Replace with typed attachment/artifact refs where they point to recorded evidence. |
| Local `OperationResult<T>` aliases | Apple Notes/TextEdit/QQMusic driver-local `OperationResult<T> = Result<T, String>` | Ordinary Rust error alias, not persisted operation result | Leave alone unless names cross durable API boundaries. |
| Domain `Store` names | `SteamLibraryStore`, game store state, app repositories | Domain repositories or game concepts | Do not migrate; these are not tracing stores. |

### Naming Cleanup Rules

- `record` should refer to persisted trace-store rows/envelopes, not arbitrary
  command reports.
- `event` should mean a sparse lifecycle/fact in the tracing trajectory when
  persisted. Domain crates may have local event enums, but persisting them
  should go through tracing events.
- `evidence` may remain a domain term for facts that support a decision. If the
  evidence is stored or linked across a run, it needs an attachment/ref/event
  envelope from `auv-tracing`.
- `candidate` remains a recognition/view-parser domain term. Do not move all
  candidates into tracing; move only candidate lineage references and recorded
  evidence envelopes.
- `summary` is presentation text, not a canonical result field. Persistent
  operation projections should be derived from tracing records and typed result
  payloads rather than maintained as a parallel free-text write path.
- `artifact` should be retired from new public invoke/tracing APIs in favor of
  `attachment` for evidence files attached to a run. Existing physical paths
  may remain `artifacts/` during migration if changing them would create churn.
- `purpose` names the semantic purpose of an attachment or stream inside an AUV
  run. `role` should be reserved for accessibility/ARIA/AX semantics where
  those concepts appear. `kind` should be reserved for enum/protocol variants.
  Current producer fields that use `kind` or `role` to mean attachment purpose
  should migrate to `purpose`.
- `content_type` is the preferred concept for attached bytes and stream segments.
  It should contain a standard content type such as `image/png` or
  `application/json`; API/wire layers may still choose `mediaType` or
  `contentType` as aliases, but the concept is MIME content type.
- `api_version` / `schema_version` should not be caller-filled boilerplate on
  every record constructor. The record type should own the version through a
  trait or associated constant, while serialization may still include a version
  field where independent JSONL/object reads need it.

## Tracing Package Capability Matrix

The audit implies `auv-tracing` cannot be a contract-only crate. It must be the
shared substrate that prevents every command, app crate, inspect surface, and
session path from rebuilding trajectory storage. The split below is the target
capability boundary.

| Package | Must provide | Must not own |
|---|---|---|
| `auv-tracing` | Stable IDs; validated attributes; run/operation/span/event/attachment records; status/failure types; verification record/status projection types; `RunMutation` / `CommittedUpdate`; shared reducer/state machine; recorder/listener traits; noop/memory/composite/broadcast listeners; object-safe store reader/writer traits; typed attachment-read helper APIs; append/stage contracts; correlation attribute constants; final-result projection records for invoke/session/MCP plus inspect/event read models. | Filesystem layout; HTTP clients; `reqwest`; subscriber/exporter setup; app/game payload schemas; parser policies; concrete command rendering. |
| `auv-tracing-store-local` or compatibility code in `auv-tracing-driver` | `LocalStore`; existing on-disk layout; atomic writes; path validation; physical `artifacts/` compatibility; attachment byte staging; list/read/replace snapshot implementation. | Domain payload interpretation; CLI/MCP output shape; OTEL exporter setup. |
| `auv-tracing-store-memory` | Full in-process `TraceReader` / `TraceWriter`; reducer-backed canonical run state; attachment bytes or explicit unsupported-byte policy; purpose reads and projections for tests/REPL/short-lived sessions. | Test-only update buffering as a substitute for store semantics; filesystem path layout; OTEL export. |
| `auv-tracing-http` or inspect-server adapter | HTTP write/update adapter; camelCase wire compatibility; auth/token/timeout policy; artifact/attachment byte upload route integration. | Canonical snake_case `RunUpdate` contract; reducer rules; domain event classification. |
| `auv-tracing-otel` or optional adapter feature | Conversion from AUV lifecycle calls to Rust `tracing` spans/events; OTEL attribute conversion; stable correlation fields; export-friendly span/event naming. | Durable run reconstruction; collector/backend configuration; mandatory OpenTelemetry dependency for local recording. |
| `auv-tracing-driver` during migration | Re-exports for moved types; compatibility aliases; existing recorded-operation ergonomics until they move; concrete glue for current callers. | New canonical contracts once `auv-tracing` owns them. |
| Runtime/app crates | Domain decisions, payload structs, recognition/candidate semantics, parser diagnostics, verification methods, operation behavior, human reports. | Run-store mutation plumbing, local role scanners, path-string lineage when a typed attachment ref exists. |

Required substrate APIs:

| Need | Capability |
|---|---|
| Durable lifecycle | Start/finish run, start/finish operation, start/finish span, record sparse event, record verification fact, record failure, attach files/bytes, finalize operation and run with failure-aware status. |
| Mutation protocol | Accept `RunMutation` batches with `expected_revision` and idempotency keys; persist as `CommittedUpdate` with store-assigned revision, run-local sequence, commit timestamp, and conflict classification. |
| Update routing | Emit committed updates to local store, memory listeners, inspect server, tests, and future realtime viewers. Timestamps and UUIDs are not substitutes for run-local sequence. |
| Reducer | Apply `CommittedUpdate` to `CanonicalRun`; validate parent operations/spans, duplicate IDs, finished-run mutation, idempotency, expected revision, batch atomicity, and conflict kinds. |
| Attachment staging | Stage file/bytes with purpose, content type, attributes, span/event linkage, checksum, and optional source ref. |
| Stream staging | Start/finish streams and append segment metadata with attachment linkage for video/frame/detection batches. |
| Post-run append | Append late results/attachments through the same update/listener path instead of `read_run -> mutate -> replace_run_snapshot` bypasses. |
| Purpose reads | Object-safe store methods list attachments and read bytes by opaque IDs. Generic typed helpers outside the trait can decode payloads by purpose, content type, schema version, and lineage rules without breaking trait-object compatibility. |
| Projection reads | Build final invoke/session/MCP projections from recorded status, typed result payload, attributes, verification records/status, attachments, and failures. Build separate inspect/event projections for timelines. |
| Diagnostics | Emit typed diagnostic/limit events with severity, class, source, message, attributes, and optional attachment/span/event linkage. |
| Correlation | Stamp `auv.run_id`, `auv.trace_id`, `auv.span_id`, `auv.event_id`, `auv.attachment_id`, `auv.stream_id`, and `auv.segment_id` consistently across durable records and telemetry export. |
| Compatibility | Keep v1alpha1 reads and physical `artifacts/` path layout during migration; rename public API concepts to attachment at the new boundary. |

## Required Use Cases

The tracing substrate must keep these use cases working during and after the
refactor:

| Use case | Required behavior |
|---|---|
| Direct CLI invoke | Commands return result-first human output and JSON from the execution `OperationOutcome<T>`, plus `run_id` / `operation_id` for follow-up inspection when recording is enabled. Noop tracing still returns the same command outcome. |
| MCP invoke | MCP returns the same outcome projection as CLI/session, writes to the selected store root when recording is enabled, and keeps returned IDs inspectable when a store is configured. |
| Session `GetOperation` | Persisted operation records can be read from the store after recording. Runtime cache fallback is migration-only; the long-term model is `OperationOutcome<T>` at execution time and `OperationRecord` / projection at read time. |
| Failed but inspectable runs | Handler failures, artifact/attachment failures, recorder delivery failures, and finalizer changes still produce inspectable run records. |
| Capture-only/read-only/activation-only commands | Completed execution remains distinct from verified semantic outcome; no command should imply semantic success just because activation or capture succeeded. |
| Netease playlist scan/select/play | Scan cache, view memory, command steps, parser diagnostics, known limits, proof attachments, and fallback lineage remain readable while app-local manifests are retired. |
| Observation snapshots | Per-page or per-view `ObservationSnapshot` records can be attached and read by purpose without depending on scroll-scan internals. |
| Scan/view parser evaluation | Lifecycle changes, parser notes, diagnostics, and source evidence can be inspected and replayed through trace read APIs without moving scan/view domain semantics into tracing. |
| Game integrations | Minecraft/osu/Balatro and future games can read typed payloads by purpose with consistent malformed/missing artifact reporting, while keeping game summaries in game crates. |
| Replay and inspection | Inspect surfaces can follow typed attachment refs from operation -> recognition/candidate -> observation -> capture/proof files. |
| Dataset and eval collection | Runs can be harvested as datasets with stable purposes, content types, schema versions, checksums, and lineage, without scraping command-specific JSON bags. |
| Performance visualization | The same lifecycle can be exported as Rust `tracing`/OTEL spans and events with stable AUV correlation fields. |
| Realtime viewers | Listener fan-out can feed in-process or server-side viewers from the canonical update stream without polling rewritten snapshots. |
| High-volume media | Screenshots, overlays, OCR JSON, video frames, detection outputs, and future YOLO/frame-analysis data are attachments or stream segments, not attributes or giant per-run JSON blobs. |
| Cross-run reuse | Long-lived scan caches and view-memory evidence can point to prior run attachments with freshness/staleness metadata and explicit unresolved-ref diagnostics. |

High-volume constraints:

- Attributes stay small and scalar/object-like. Images, video, OCR dumps, model
  detections, and frame batches are attachments or future stream segments.
- Events are sparse lifecycle facts. Per-frame data should use segment IDs,
  bulk attachment staging and lazy purpose reads.
- Inspect/read APIs should support filtering by purpose, time/span/event linkage,
  stream ID, segment ID, and content type so viewers do not load every frame by
  default.

## Migration Plan

1. Add `auv-tracing` with record types, attribute entry conversion, recording
   lifecycle traits, run update types, listener fan-out, and store traits.
2. Move or wrap the existing `auv-tracing-driver` recorder APIs so command,
   driver, scan, and runtime code can share one recording lifecycle.
3. Decide whether concrete local storage lands inside `auv-tracing` for the
   first slice or in a sibling implementation crate such as
   `auv-tracing-store`.
4. Update `auv-tracing-driver` to re-export migrated types while its internal
   code imports them from `auv-tracing`.
5. Rename storage-facing artifact concepts to attachment at public boundaries
   where practical; keep narrow compatibility only inside `auv-tracing-driver`
   if path/layout churn would distract from the contract slice.
6. Classify each existing invoke `signals`, `known_limits`, backend decision,
   disturbance, fallback reason, and selected path per command. Map them to
   typed result payloads, delivery facts, diagnostics/limits, evidence-level
   attributes, or attachments. Do not perform a mechanical `signals` ->
   `attributes` rename.
7. Change renderer, JSON, MCP invoke, and operation-summary records only after
   the final-result projection reset is reviewed. Keep detailed event timelines
   behind inspect/read/subscribe surfaces.
8. Update direct invoke handlers (`display`, `app`, `input`, `scan`) by
   mechanical mapping only. Do not add new command behavior.
9. Update `docs/TERMS_AND_CONCEPTS.md` with the new terms: attribute, event,
   attachment, verification record, verification status, and final-result
   projection.
10. Add explicit store/recorder/exporter terms to
    `docs/TERMS_AND_CONCEPTS.md`: trace store, run recorder/sink, recording
    backend, inspect server adapter, telemetry exporter / OTEL mirror.
11. Mark `auv-tracing-driver` as a migration/compatibility owner once
    `auv-tracing` owns canonical contracts.
12. Add provisional stream/segment terms for continuous frames and recordings.

## Testing

- Unit tests in `auv-tracing` for serialization of IDs, attributes, span/event
  records, attachment records, and verification records once the verification
  shape is approved.
- Reducer/store tests covering local and in-memory trace store behavior through
  the same `TraceReader` / `TraceWriter` traits.
- `auv-tracing-driver` tests proving the recorder still writes equivalent run,
  span, event, and attachment records.
- Inspect server adapter tests proving accepted `RunUpdate` and attachment
  writes use the same reducer rules as local recording.
- `auv-cli-invoke` renderer tests for default human, detail human, and JSON.
- CLI/MCP invoke tests asserting the approved final-result projection fields;
  they should also assert the default invoke shape does not dump the full event
  stream.
- Inspect/read API tests asserting event timelines remain available by `run_id`
  with filtering or pagination.
- Operation-summary tests covering the new persisted wire shape.

Run at minimum:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- invoke --help
```

## Open Decisions

- Whether the physical run-store subdirectory remains `artifacts/` for one
  migration slice while public types say `AttachmentRecord`.
- Whether `auv-tracing` should include optional `opentelemetry` / OTLP
  conversion features immediately or wait for the first exporter integration.
- Whether `AttachmentId` should serialize with the historical `artifact_`
  prefix during migration or move directly to an `attachment_` prefix.
- Whether the first implementation puts `LocalStore` directly in
  `auv-tracing`, or splits concrete storage into `auv-tracing-store` while
  `auv-tracing` owns only traits and routing.
- Whether the first memory implementation stores attachment bytes in-process,
  stores only metadata with explicit unsupported-byte errors, or offers both
  policies.
- Whether stream/segment records belong in the first `auv-tracing` slice or
  remain provisional terms until the first continuous-frame/recording producer.
- Whether the first inspect-server rewrite keeps existing HTTP routes with
  compatibility wire wrappers or introduces a versioned CDP-style domain route
  layout for runs, spans, events, attachments, streams, and projections.
- How REPL/script sessions should group repeated commands: multiple runs in a
  session, one long run with command spans, or both with explicit projection
  rules.
- The exact `InvokeProjection` shape after the reset: whether verification is
  always present, whether `verification_status` is a required aggregate, and
  the exact envelope for typed command results and structured failures.
- The exact split between execution evidence records and semantic verification
  records. Target resolution, input delivery, and observations should remain
  orthogonal facts; semantic verification should assert observed state against
  criteria.
- The semantic verification taxonomy. Candidate domains include `precondition`,
  `semantic_outcome`, `postcondition`, and `artifact_integrity`, but these are
  not approved.
- The aggregation rules for semantic verification records: precedence between
  failed, error, inconclusive, skipped, not requested, and passed checks; how
  optional checks affect the aggregate; and how partial success is represented.
- The distinction between `failure` and verification failure. Runtime,
  handler, storage, and recorder failures should not be collapsed with a
  completed command whose semantic outcome failed verification.
