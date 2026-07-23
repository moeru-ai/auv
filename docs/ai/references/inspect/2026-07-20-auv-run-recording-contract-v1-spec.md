# AUV Tracing, Run Storage, And Inspection Contract V1

Status: implemented V1 contract

Responsibility: typed instrumentation / context propagation / run storage /
artifact routing / inspection / telemetry projection

> The filename is retained so the repository does not gain another competing
> spec. `auv-run` is not a crate or architecture requirement in this revision.

## Relationship To The Earlier Design

The earlier
[`2026-07-17-auv-tracing-contract-and-invoke-output-design.md`](2026-07-17-auv-tracing-contract-and-invoke-output-design.md)
remains the repository audit, source comparison, migration evidence, and
research record. Its code inventory and Markdown tables must remain available
during implementation.

This document is the approved V1 contract. It does not treat existing
AUV runtime, tracing, invoke, or inspect code as a compatibility constraint.
When the two documents disagree, this document contains the proposed design and
the earlier document explains the system being replaced.

The public names and semantics in this document define V1.
Accepted core terms must also be added to `docs/TERMS_AND_CONCEPTS.md` before
implementation begins.

## Decision Summary

V1 is based on these decisions:

1. `auv-tracing` is an opt-in typed instrumentation and run-data library. It is
   not an operation runtime, command dispatcher, catalog, workflow engine, or
   centralized runner.
2. Applications own execution. A CLI binary, MCP host, app crate, REPL, or
   third-party application configures AUV tracing and then continues to call its
   own Rust APIs and drivers.
3. Drivers and operation crates may contain AUV instrumentation call sites.
   Without an installed AUV dispatch, those call sites do not create runs or
   activate persistence.
4. The application explicitly creates a `RunId` and root scope. AUV tracing
   never synthesizes a root `Context` because an event happened to be emitted.
5. `auv_tracing::Context::current()` exposes the current AUV scope. Parentage
   propagates through scoped synchronous execution, instrumented futures, and
   explicit cross-process propagation rather than a central `RunSession`.
6. AUV spans, events, and artifacts enter through typed functions and thin macro
   APIs. The internal router decides which configured routes receive each type.
7. Binary artifacts never enter Rust `tracing`, OpenTelemetry attributes, or
   OTLP payload fields. Only bounded metadata and AUV resource URIs may be
   projected there.
8. `RunStore` owns ordered commits, durable run data, artifact bytes, replayable
   reads, and subscription recovery. Store state is not the source of a
   function's immediate return value.
9. Inspect Server can implement the remote run/artifact store contract. It
   accepts binary uploads, serves raw bytes, and resolves AUV artifact URIs in
   batches for browser and viewer clients.
10. OpenTelemetry and OTLP are optional lossy projections. They are not the AUV
    state model, storage authority, verification model, or binary transport.
11. A run has at most one authority `RunStore`, identified by a stable
    `AuthorityId`. Context propagation rejects an attempt to continue the same
    persisted run under another authority. Live inspection reads the authority;
    Rust tracing and OTEL routes consume committed facts when it is present.
    Telemetry-only dispatches may project validated volatile spans and events.
12. Canonical span lifecycle records only start and end. Ending a span does not
    state whether the scoped work succeeded, failed, was cancelled, or was
    semantically verified.
13. A root `Context` establishes correlation but does not write a separate run
    lifecycle. V1 runs remain appendable and have no close, finish, seal, or
    amendment state.
14. Span and event emission is tracing-like: synchronous and non-fallible to
    the application call site. Artifact persistence and explicit dispatch
    flushing remain asynchronous and fallible.

The intended composition is:

```text
application / CLI / MCP host / app crate
  -> configures auv-tracing Dispatch
  -> explicitly starts a run root
  -> calls its own Rust APIs and drivers
       -> typed AUV spans, events, and artifacts
       -> Context::current()
       -> internal router
            -> zero or one authority RunStore
                 -> optional live inspection
            -> optional Rust tracing projection
            -> optional OpenTelemetry / OTLP projection
```

No route acquires the right to execute or retry the application operation.

## Non-Goals

This spec does not define:

- an `Operation` trait that applications must implement;
- `ErasedOperation`, `OperationCatalog`, or a shared command runtime;
- a central `RunSession` used by CLI, MCP, libraries, and REPLs;
- one shared CLI/MCP/library response object;
- a generic verification, observation, recommendation, or retry framework;
- automatic semantic-success claims derived from delivery or span completion;
- raw image, audio, video, or arbitrary byte values in attributes;
- an AUV replacement for OpenTelemetry SDK processors or OTLP exporters;
- local-to-Inspect or store-to-store replication;
- a generic resource resolver when only artifact resolution has a current use;
- compatibility with the existing `auv-run` implementation.

CLI, MCP, and typed Rust APIs may share the same underlying app operation, but
they remain separate frontends with separate request and response contracts.

## Vocabulary And Cardinality

| Term | Meaning |
|---|---|
| `Dispatch` | The configured in-process destination for typed AUV emissions. It owns routing policy, not application execution. |
| `Context` | A cloneable snapshot of the current AUV scope and its associated dispatch. |
| `Run` | An explicitly created correlation, inspection, persistence, and replay scope. |
| `AuthorityId` | The stable non-secret identity of the one `RunStore` allowed to persist a run. It prevents one propagated `RunId` from silently splitting across stores. |
| operation scope | A caller-defined span around one atomic app or driver operation. It is a usage of the span API, not a separate stored entity or required Rust operation trait. |
| span | A timed nested scope inside a run or operation scope. |
| event | A typed point-in-time fact associated with the current run and optional span. |
| artifact | Typed metadata plus externally stored bytes used for inspection, evidence, replay, or domain output. |
| artifact URI | The transport-independent AUV identity of an artifact. |
| content locator | A resolver-specific URL used to fetch one artifact representation. It is not canonical AUV identity. |
| `RunStore` | The contract for ordered run commits, artifacts, reads, and recovery. |
| `RunCommit` | One atomic ordered set of facts accepted by a `RunStore`. |
| projection | A lossy mapping from AUV types into another read or telemetry model. |

The current relationship is:

```text
Run 1 -> 0..N spans
Run 1 -> 0..N events
Run 1 -> 0..N artifacts
span 1 -> 0..N child spans
span 1 -> 0..N events
artifact 1 -> 1 AUV artifact URI
artifact 1 -> 0..N resolver-specific content locators
```

An application represents an operation scope by starting an ordinary typed AUV
span with a namespaced `SpanName`. The span's `SpanId`, parentage, timing, and
lifecycle facts are the complete tracing representation of that scope. V1 has
no `OperationId`, operation discriminator, or persisted operation record. A
future consumer that needs operation-specific indexing must first demonstrate
why span name and hierarchy are insufficient before adding another identity or
entity.

## Ownership

| Layer | Owns | Does not own |
|---|---|---|
| Application composition root | Installing `Dispatch`, choosing routes and failure policy, creating `RunId`, and establishing the root scope. | Driver implementation or store internals. |
| Application/app operation | Control flow, typed inputs and outputs, cancellation, verification policy, and whether an artifact failure affects its own result. | Store layout or OTLP encoding. |
| `auv-tracing` | Context propagation, typed emission APIs, macros, IDs, validation, routing, run-store ports, and projection ports. | Operation execution, command parsing, MCP protocol, or semantic verification. |
| Drivers | Capability-oriented APIs and optional instrumentation at meaningful lifecycle points. | Installing global tracing, selecting stores, or claiming semantic success. |
| `RunStore` | Commit ordering, idempotency, artifact bytes, integrity, snapshots, history, and subscription recovery. | Application execution or retries. |
| Inspect Server | Remote writes, binary uploads, URI resolution, raw artifact reads, live queries, and viewer-facing projections. | Application scheduling or implicit operation retries. |
| Rust tracing projection | Bounded diagnostic spans/events for the Rust tracing ecosystem. | AUV binary payloads or canonical run state. |
| OpenTelemetry projection | Bounded AUV correlation and observability data mapped into OTEL signals. | AUV state authority, artifact bytes, replay, or semantic truth. |
| CLI/MCP adapters | Parsing and frontend-specific result presentation. | AUV context propagation or run storage semantics. |

## Primitive Types

Values with invariants use private-field newtypes. Callers do not supply
version strings, storage paths, or physical object keys on individual facts.

```rust
pub struct RunId(Uuid);
pub struct AuthorityId(Uuid);
pub struct SpanId(Uuid);
pub struct EventId(Uuid);
pub struct ArtifactId(Uuid);
pub struct RunRevision(u64);
pub struct IdempotencyKey(Uuid);
pub struct PageLimit(NonZeroU32);
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
pub struct ArtifactUri(url::Url);

pub struct SpanLink {
  span_id: SpanId,
}

pub struct Timestamp {
  unix_seconds: i64,
  nanoseconds: u32,
}
```

ID constructors reject the nil UUID. An authority ID is non-secret and stable
for the lifetime of one store: a file store persists it below its configured
root, and an Inspect Server publishes its configured value. `Timestamp` rejects nanoseconds greater
than `999_999_999` and seconds outside JSON's exact integer range; ordering
compares seconds and nanoseconds together. `RunRevision` is limited to
`9_007_199_254_740_991`, so browser clients can parse it without precision
loss.
`Sha256Digest` parses exactly 64 lowercase hexadecimal characters on JSON input
and serializes in that form. Private fields prevent callers from bypassing
these invariants.

`PageLimit` accepts `1..=1024`; larger pages are rejected before a store read.
`ContentType` accepts one concrete parsed MIME value whose canonical text is at
most 256 UTF-8 bytes. Wildcard types and subtypes are invalid for committed
artifacts.
One ordinary `RunCommitRequest` contains `1..=256` mutations, and one
`RunCommit` contains `1..=256` facts. The Inspect run JSON endpoint rejects a
body larger than 32 MiB before strict decoding; artifact content continues to
use the separate streaming 512 MiB boundary.

`ArtifactUri` accepts only the canonical AUV artifact grammar selected by this
spec. It rejects user info, query strings, fragments, path traversal, unknown
resource families, invalid IDs, and non-canonical escaping.

V1 form:

```text
auv://runs/{run_id}/artifacts/{artifact_id}
```

This is the V1 artifact URI form. The `runs` authority selects the resource
family; the path contains the owning run and artifact IDs. The wire
representation has exactly one canonical form, and AUV provides the parser.
Frontends must not reconstruct or parse it with ad-hoc string splitting.
The URI is resolved against the viewer's connected authority; `AuthorityId` is
carried by run data and context propagation rather than duplicated inside every
artifact URI.

`RunId` remains independent from OpenTelemetry `TraceId`. An OTEL projection
exports the AUV run ID as a correlation attribute; it does not derive one ID
from the other.

## Dispatch And Context

### Dispatch

`Dispatch` is analogous to the configured dispatch side of Rust `tracing`: it
holds the active AUV router and route policy. It contains no operation catalog
or scheduler.

The V1 configuration shape is:

```rust
let dispatch = auv_tracing::configure()
  .run_store(authority)
  .project_telemetry(telemetry, TelemetryRoutePolicy::fixed_fields_only())
  .on_error(error_reporter)
  .build()?;

auv_tracing::dispatcher::set_global_default(dispatch.clone())?;
```

Tests and embedded applications bind a dispatch without changing the process
global:

```rust
let root = auv_tracing::dispatcher::with_default(&dispatch, || {
  auv_tracing::Context::root(run_id)
});
```

`with_default` restores the preceding dispatch after normal return or unwind.
The returned root context retains the selected dispatch and can then instrument
async work explicitly.

The default dispatch worker uses a runtime-independent thread task spawner.
Applications whose authority, projector, or artifact reader must be polled on
a particular async runtime override that boundary explicitly:

```rust
let dispatch = auv_tracing::configure()
  .task_spawner(runtime_spawner)
  .run_store(authority)
  .build()?;
```

The task boundary is scheduling for instrumentation IO only. It does not run
application operations and is not an AUV runtime or runner.

- applications configure AUV tracing at a composition root;
- libraries and drivers do not install a global dispatch;
- tests and embedded applications can use a scoped default instead of a global
  one;
- installing a dispatch does not create a run;
- a dispatch may project spans and events without a run store;
- a process may execute multiple independent runs concurrently.

`Dispatch` is the public producer-side handle. The router is its private
implementation and is not another public trait. A configured run store is the
sole authority. When one is present, telemetry projectors consume authority
output. Without a run store, telemetry projectors may consume validated
volatile span and event facts, but no run is durable and artifacts are not
accepted. When Inspect Server is the authority, its `InspectRunStore` client is
passed to `run_store`.

`DispatchErrorReporter` receives validation, encoding, queue, authority, and
downstream projection errors that cannot be returned from synchronous span and
event call sites. Its callback must be non-blocking and must not panic. The
default reporter discards diagnostics; applications that enable a store or
projection should install one explicitly.

```rust
#[derive(Clone)]
pub struct Dispatch {
  // Private router and worker state.
}

pub type DispatchTask =
  Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub trait TaskSpawner: Send + Sync {
  fn spawn(&self, task: DispatchTask) -> Result<(), TaskSpawnError>;
}

pub struct TaskSpawnError {
  code: ErrorCode,
}

impl TaskSpawnError {
  pub fn new(code: ErrorCode) -> Self;
}

pub struct ThreadTaskSpawner {
  // Private runtime-independent worker pool.
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

impl DispatchFailure {
  pub fn stage(&self) -> DispatchStage;
  pub fn code(&self) -> &ErrorCode;
}

pub trait DispatchErrorReporter: Send + Sync {
  fn report(&self, failure: &DispatchFailure);
}

pub struct FlushError {
  failures: NonEmptyVec<DispatchFailure>,
}

impl FlushError {
  pub fn failure_count(&self) -> NonZeroUsize;
  pub fn first(&self) -> &DispatchFailure;
}
```

The reporter is diagnostic output, not a retry callback. It cannot mutate the
failed fact, execute application code, or make an authority commit successful.
`DispatchBuilder` uses `ThreadTaskSpawner` unless the application supplies a
different `TaskSpawner`. Built-in memory and file stores work with the default;
`auv-tracing-inspect` supplies a Tokio-backed spawner for its runtime-bound HTTP
client. The selected spawner polls store, projector, and artifact-reader
futures, so a third-party adapter must document when it requires an override.

### Context

`auv_tracing::Context` owns AUV semantics even if its private implementation
later reuses a mature context propagation primitive.

```rust
#[derive(Clone)]
pub struct Context {
  // Private current run/span state and dispatch association.
}

pub struct ContextGuard<'a> {
  // Private thread-bound restoration guard.
}

pub struct WithContext<F> {
  // Private future and captured context.
}

impl Context {
  pub fn root(run_id: RunId) -> Self;
  pub fn current() -> Self;
  pub fn authority_id(&self) -> Option<&AuthorityId>;
  pub fn run_id(&self) -> Option<&RunId>;
  pub fn span_id(&self) -> Option<&SpanId>;
  pub fn is_enabled(&self) -> bool;
  pub fn enter(&self) -> ContextGuard<'_>;
  pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T;
  pub fn instrument<F>(&self, future: F) -> WithContext<F>;
}

impl<F: Future> Future for WithContext<F> {
  type Output = F::Output;

  fn poll(
    self: Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> Poll<Self::Output>;
}
```

The public API does not expose `opentelemetry::Context`. OTEL may be absent
while AUV run recording and inspection remain active.

`Context::root` captures the current AUV dispatch and associates it with the
caller-provided `RunId`. It does not emit a run-start fact. The context exists
even when no dispatch is installed, in which case `is_enabled` is false. Child
scopes inherit the run and current span. Constructing a root context does not
make it current; the application must use `enter`, `in_scope`, or `instrument`.
A root context is never synthesized from an event or artifact emission.

Synchronous code uses a bounded scope guard or `in_scope`. Async code uses an
instrumented future that activates the context only while that future is being
polled. Holding a thread-local guard across `.await` is not a valid propagation
strategy. Spawned work requires explicit propagation, as it does in Rust
`tracing` and OpenTelemetry.

If constructing a future can synchronously emit instrumentation before its
first poll, construct it inside `in_scope` and then instrument the returned
future:

```rust
let future = root.in_scope(|| make_domain_future(input));
let value = root.instrument(future).await;
```

`ContextGuard` is thread-bound and must not be held across `.await`.
`WithContext<F>` activates its captured context only while `F` is being polled.
Neither wrapper creates or ends a span. `Context` is cloneable, `Send`, and
`Sync`.

### Span Handle

```rust
#[derive(Clone)]
pub struct Span {
  // Private identity, context, and shared close state.
}

pub struct Instrumented<F> {
  // Private future and owned span handle.
}

impl Span {
  pub fn id(&self) -> Option<&SpanId>;
  pub fn is_enabled(&self) -> bool;
  pub fn context(&self) -> Context;
  pub fn enter(&self) -> ContextGuard<'_>;
  pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T;
  pub fn instrument<F>(self, future: F) -> Instrumented<F>;
}

impl<F: Future> Future for Instrumented<F> {
  type Output = F::Output;

  fn poll(
    self: Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> Poll<Self::Output>;
}
```

Starting an enabled span submits one `SpanStarted`. A disabled span has no ID
and submits nothing. `Span::enter` changes only the current context; dropping
the guard does not end the span. The last clone of an enabled `Span`, including
contexts derived from it, submits one `SpanEnded`. There is no public
`finish`, `close`, or status-setting method.

The handle captures both its wall-clock `started_at` and a private monotonic
instant. It derives `ended_at` as `started_at + monotonic_elapsed`, so a wall
clock adjustment cannot make a valid local span end before it started. Stores
still reject externally supplied end timestamps earlier than the committed
start.

`Span::instrument` consumes one span handle. It activates the span context only
while the wrapped future is polled. Completion or cancellation by dropping the
wrapper destroys the inner future while that context is active, then releases
the span handle and eventually ends the span when no other clone remains. The
end fact does not distinguish completion from cancellation.

Both wrappers use pinned `Option<F>` storage. After `Ready` and from
`PinnedDrop`, they enter the captured context and call the projected option's
`set(None)` before leaving the scope. `Instrumented` then releases its owned
span handle exactly once. A direct `F` field cannot satisfy this contract in
safe Rust because automatic field destruction happens after `PinnedDrop`.

### Cross-Process Propagation

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
  fn values<'a>(
    &'a self,
    name: &str,
  ) -> Box<dyn Iterator<Item = &'a str> + 'a>;
}

pub struct PropagationError {
  code: ErrorCode,
}

impl Context {
  pub fn inject(&self, carrier: &mut dyn TextMapWriter);
  pub fn from_remote(
    remote: RemoteContext,
  ) -> Result<Self, PropagationError>;
}

pub fn extract(
  carrier: &dyn TextMapReader,
) -> Result<Option<RemoteContext>, PropagationError>;
```

The V1 text carrier uses `auv-context-version: 1`, `auv-run-id`, optional
`auv-authority-id`, and optional `auv-span-id` fields. Decoding rejects duplicate
fields, invalid IDs, unknown versions, and partial values. `inject` first
removes all four AUV fields and writes none when the context has no run;
`extract` returns `Ok(None)` only when all four fields are absent.
Extraction never serializes or accepts a store, dispatch, writer, or content
locator. `Context::from_remote` binds the remote values to the receiver's
current local `Dispatch` and does not make the resulting context current. When
both contexts have an authority, their `AuthorityId` values must match;
otherwise construction fails instead of creating split histories. A
telemetry-only side may propagate or consume a context without an authority ID.

An extracted remote span is a link, never a local parent. Every span started
directly from a remote context, before entering a local span, carries that link.
Child spans then use ordinary local parentage. The store therefore never
accepts a parent that its own run history may not contain. Standard W3C Trace
Context propagation remains separate; OTEL correlation is not hidden inside
the AUV carrier.

### Disabled Behavior

The intended opt-in behavior is:

| Condition | Span/event call | Artifact call |
|---|---|---|
| No dispatch or no current run | `start_span!` returns a disabled handle; `emit_event!` returns `()`. No implicit run is created. | Returns `Ok(None)` without reading the body. |
| Dispatch without a run store | Validated spans and events may reach telemetry projectors but are not durable. | Returns `Ok(None)` without reading the body. |
| Dispatch with a run store | The call submits validated data and returns without an authority acknowledgement. | Returns `Ok(Some(ArtifactMetadata))` after atomic publication. |
| Authority write fails | Reports through `DispatchErrorReporter`; a later `flush` reports uncommitted accepted facts. | Returns `Err(ArtifactWriteError)` to the receipt waiter and reports it if no waiter remains. |
| Telemetry projection fails after commit | Reports through `DispatchErrorReporter`; a later `flush` includes the failure. | The committed artifact still returns `Ok(Some(ArtifactMetadata))`; projection failure cannot undo it. |

`Ok(None)` has one meaning: the current context has no artifact authority.
It is not returned for rejected, failed, or publication-unknown writes. This keeps
driver-only use independent from AUV tracing while allowing callers that need
an artifact to require `Some` explicitly.

## Typed API And Macros

Macros are convenience frontends over typed functions. They do not define a
second schema language and do not accept arbitrary binary fields.

The V1 action-oriented macros are:

```rust
let span = auv_tracing::start_span!(span_value);
auv_tracing::emit_event!(event_value);
let artifact = auv_tracing::emit_artifact!(artifact_value).await?;
```

They delegate to the same-named typed functions. `start_span!` returns `Span`,
`emit_event!` returns `()`, and `emit_artifact!` yields
`Result<Option<ArtifactMetadata>, ArtifactWriteError>`.

```rust
pub trait SpanSpec {
  const NAME: &'static str;
  fn attributes(&self) -> Attributes;
}

pub fn start_span(spec: impl SpanSpec) -> Span;
pub fn emit_event(event: impl EventPayload);

pub struct ArtifactEmission {
  // Private receipt state. The body has already moved to the artifact worker.
}

pub fn emit_artifact<R>(
  artifact: NewArtifact<R>,
) -> ArtifactEmission
where
  R: futures_io::AsyncRead + Unpin + Send + 'static;

impl Future for ArtifactEmission {
  type Output = Result<Option<ArtifactMetadata>, ArtifactWriteError>;
}
```

`NewArtifact` has private fields and generates its `ArtifactId` and
idempotency key in its constructor. Its required inputs are purpose, content
type, expected byte length, expected SHA-256, bounded attributes, and the owned
body reader. The current `Context` supplies authority, run, and optional span
identity. `emit_artifact` copies those values plus the dispatch into a private
detached correlation token and submits the artifact job synchronously before
returning the future. The token does not own a span lifecycle handle, so the
associated span may end while bytes are still being persisted. Moving or
polling the future under another context does not change ownership. Admission
captures a prerequisite fence for preceding span/event submissions. The worker
waits for a referenced `SpanStarted` to commit, then transfers bytes without
occupying the fact FIFO. Later `SpanEnded` and event submissions may therefore
commit before the eventual `ArtifactPublished` fact.

`artifact!` is not used because it does not say whether the macro declares,
writes, uploads, or commits anything. `include_artifact!` is not recommended
for runtime emission because Rust developers already associate `include_*` with
compile-time embedding.

The macro accepts a typed value:

```rust
auv_tracing::emit_event!(InputDelivered {
  method: InputMethod::Accessibility,
});
```

It must not use an unbounded field-list API as the canonical contract:

```rust
// Not the canonical typed API.
auv_tracing::emit_event!(
  kind = "input_delivered",
  arbitrary_key = arbitrary_json,
);
```

`artifact_value` owns validated metadata, expected integrity, and an
`AsyncRead` body. The macro produces a future and does not buffer that body.
The returned future waits for the receipt; dropping it does not cancel a job
already accepted by the dispatch. The worker still reaches one terminal result:
commit, confirmed pre-publication failure, or `PublicationUnknown` after
idempotency lookup. When no waiter remains, failures go to
`DispatchErrorReporter`. Callers that require the receipt must await the future.
Continuous frame, audio, and video sessions are separate lifecycle contracts
and are not represented by repeatedly extending one V1 artifact.

## Internal Routing

The entry API determines the data lane. A public string `kind`, target prefix,
or caller-selected exporter list does not decide whether bytes are safe for
OTLP.

Projectors receive only this closed, bounded core type:

```rust
pub enum TelemetryItem {
  SpanStart {
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    remote_span_id: Option<SpanId>,
    name: SpanName,
    started_at: Timestamp,
    start_revision: Option<RunRevision>,
    attributes: Attributes,
  },
  SpanEnd {
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: SpanId,
    ended_at: Timestamp,
    end_revision: Option<RunRevision>,
  },
  Event {
    authority_id: Option<AuthorityId>,
    run_id: RunId,
    span_id: Option<SpanId>,
    event_id: EventId,
    schema: EventSchema,
    occurred_at: Timestamp,
    revision: Option<RunRevision>,
  },
  Artifact {
    authority_id: AuthorityId,
    run_id: RunId,
    span_id: Option<SpanId>,
    uri: ArtifactUri,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    byte_length: ByteLength,
    sha256: Sha256Digest,
    attributes: Attributes,
    revision: RunRevision,
  },
}

pub struct TelemetryError {
  code: ErrorCode,
}

pub struct TelemetryRoutePolicy {
  span_attribute_keys: BTreeSet<AttributeKey>,
  artifact_attribute_keys: BTreeSet<AttributeKey>,
}

impl TelemetryRoutePolicy {
  pub fn fixed_fields_only() -> Self;
  pub fn allow_span_attribute(self, key: AttributeKey) -> Self;
  pub fn allow_artifact_attribute(self, key: AttributeKey) -> Self;
}

pub trait TelemetryProjector: Send + Sync {
  fn project(
    &self,
    item: TelemetryItem,
  ) -> BoxFuture<'_, Result<(), TelemetryError>>;

  fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>>;
}
```

Calls to one projector are serialized in dispatch order. The dispatch awaits a
`project` result before submitting the next item to that projector. At a flush
barrier it first completes all pre-barrier `project` calls and then awaits the
projector's own `flush`. A projector error is diagnostic and cannot roll back a
committed fact.

| Typed input | Run store | Inspect authority | Rust tracing / OTEL |
|---|---:|---:|---:|
| span lifecycle | configurable | configurable | bounded projection |
| typed point event | configurable | configurable | allowlisted projection |
| artifact metadata | committed with artifact bytes | authority upload | URI and bounded metadata only |
| artifact bytes | consumed once by the authority | authority upload | never |

The router applies these stages:

```text
typed API or macro
  -> validate type and bounds
  -> assign AUV identities and parentage
  -> append through the selected authority when one is configured
  -> route committed facts to optional downstream processors
  -> otherwise route validated volatile spans/events to telemetry only
  -> project only approved bounded fields to external telemetry
```

The AUV route policy is the primary payload boundary. OpenTelemetry filters are
an optional secondary control for sampling or deployment policy; they are not
responsible for detecting and removing binary values after an event has been
constructed.

An AUV event emitted through the typed API may be projected into Rust
`tracing`. Generic Rust `tracing` events do not automatically become canonical
AUV facts. An optional compatibility layer may ingest selected external spans
or diagnostics later, but it cannot provide the typed artifact contract.

### Route Failure

Routing failure never instructs the caller to repeat a UI operation. The router
knows whether an AUV fact or artifact was accepted; it does not know whether an
external application side effect happened.

The contract distinguishes:

- tracing disabled;
- an authority write rejected before commit;
- artifact bytes committed but a response was lost;
- artifact upload failed before publication;
- telemetry projection failed after the authority commit.

Projection failure does not roll back the authority commit. V1 does not call a
second `RunStore` from the routing path. Reliable local-to-Inspect publication
requires source authority identity, revision-preserving ingest, artifact copy,
and resume semantics and is deferred as one replication contract.

### Ordering And Flush

One `Dispatch` preserves submission order for synchronous span and event facts.
With an authority configured, it does so by serializing the corresponding
`RunStore::commit` calls per run; it does not issue concurrent commits and then
reorder their returned futures. This preserves start/parent/event/end causality
at the authority itself.
Artifact-job admission captures its preceding fact fence but does not occupy
that FIFO while bytes are transferred; `ArtifactPublished` receives commit
order only when publication completes. The producer API may enqueue or apply
bounded backpressure, but it must not silently discard an accepted fact or job.
Validation, encoding, capacity, and worker failures are sent to
`DispatchErrorReporter` and retained for the next flush result.

Authority-backed telemetry consumes committed facts in authority order. The
dispatch establishes a snapshot-plus-subscription cursor before releasing a
run's first queued mutation, registers its idempotency keys before store calls,
and projects only commits owned by that dispatch. Commits from another writer
advance the cursor but are not duplicated into this dispatch's projectors. Gap
recovery uses revisioned page reads, and a pre-commit mutation is never
projected as durable telemetry. This one committed cursor also orders artifact
publications that complete on the separate byte-transfer lane. A telemetry-only
dispatch has no authority revision and uses submission order.

```rust
impl Dispatch {
  pub fn flush(
    &self,
  ) -> Pin<Box<dyn Future<Output = Result<(), FlushError>> + Send + '_>>;
}
```

`flush` is a barrier for every emission submitted through that dispatch before
the call. The barrier position is captured when `flush()` returns its future,
not when that future is first polled. It waits until each such emission has
reached its configured authority and downstream processors or has a recorded
terminal routing error. Flush calls are serialized internally. Each result
covers the interval after the preceding completed flush barrier, or dispatch
creation for the first call; a failed flush still advances that reported
barrier. Calling `flush` does not end a span, close a context, seal a run, or
prevent later emissions.

Applications that need a complete boundary drop their span handles and derived
contexts before calling `flush().await`. Dropping `Dispatch` does not imply a
successful flush, because Rust `Drop` cannot await durability.

## Run Facts And Direct Results

Application return values do not come from `RunStore`, a trace projection, or
an inspect query. The application calls its own operation and returns that
operation's typed value directly.

```text
typed operation call
  -> direct Result<T, E> returned to CLI/MCP/library caller
  -> optional AUV emissions observe the same execution
```

Disabling AUV tracing must not change `T`, `E`, operation dispatch, verification
policy, or UI side effects. A recording failure must not cause AUV tracing to
re-execute the operation.

The committed run model contains span lifecycle facts, typed point events, and
artifact publication. It contains no run lifecycle record. The following
invariants apply:

- every fact belongs to one explicit `RunId`;
- span parentage is acyclic and run-local;
- an operation scope is an ordinary caller-named span rather than a duplicate
  operation-state authority;
- a span may start once and finish at most once;
- an event is immutable;
- an artifact becomes visible only after its bytes and required metadata are
  committed;
- an unfinished span states only that no finish fact was committed;
- a span finishing does not claim semantic success;
- delivery acknowledgement and verification remain separate typed domain
  facts when an operation chooses to emit them;
- no generic `summary`, `boundary`, `claim`, `recommended_next_action`,
  `retryable`, or `side_effects_may_have_occurred` field is added by core.

The canonical span lifecycle has exactly two facts:

```rust
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
```

`parent_span_id` refers only to an existing local span in the same run.
`remote_link` is present on a span created directly from a `RemoteContext` and
contains the propagated remote span ID. The containing commit already supplies
the matching authority and run IDs. A span with a local parent does not repeat
the remote link, and a span cannot use the same identity as both parent and
remote link.

`SpanEnded` means only that the scope is no longer active. It has no generic
status, outcome, error, cancellation, verification, or final-attributes bag.
Applications may emit their own typed facts before ending the span when those
facts have a concrete producer and consumer. A missing `SpanEnded` means only
that no end fact was committed; readers must not infer why.

This spec does not stabilize a generic verification object. An app crate may
emit a typed verification result whose schema is owned by that app or by a
future shared verification crate. AUV tracing records and correlates it without
inventing verification domains, methods, or aggregate verdicts.

Typed point events use an explicit payload schema rather than arbitrary event
attributes:

```rust
pub struct EventSchema {
  name: EventName,
  version: NonZeroU32,
}

pub struct JsonPayload(Box<serde_json::value::RawValue>);

pub struct EventOccurred {
  event_id: EventId,
  span_id: Option<SpanId>,
  occurred_at: Timestamp,
  schema: EventSchema,
  payload: JsonPayload,
}

pub trait EventPayload: serde::Serialize {
  const NAME: &'static str;
  const VERSION: u32;
}
```

`emit_event!` accepts only a type implementing `EventPayload`. The adapter
validates the namespaced name, rejects version zero, serializes once, and
enforces a 64 KiB encoded JSON limit. Larger structured output is an artifact.
Integers outside `-(2^53-1)..=(2^53-1)` and non-finite numbers are rejected;
schemas that need larger integers encode them as validated decimal strings.
Event JSON is canonical run data but is never forwarded wholesale to Rust
tracing or OTEL.

## Committed Run History

When a `RunStore` is configured, one append-only commit sequence is the store's
authority for that run. A snapshot is derived from commits and is not a second
write authority.

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

pub enum RunMutation {
  StartSpan(SpanStarted),
  EndSpan(SpanEnded),
  EmitEvent(EventOccurred),
}

pub enum RunFact {
  SpanStarted(SpanStarted),
  SpanEnded(SpanEnded),
  EventOccurred(EventOccurred),
  ArtifactPublished(ArtifactPublished),
}
```

`RunMutation` is the closed writer input. `RunFact` is the committed union and
additionally contains the store-produced artifact publication fact. Generic
`commit` rejects artifact publication; only `write_artifact` can create it after
the bytes pass integrity checks. Neither union contains frontend projections,
arbitrary plugin objects, a run terminal state, or a second operation
lifecycle.

The store allocates contiguous revisions beginning at one. Revision zero means
that no commit exists. A revision is a store ordering and recovery cursor; it
is not a payload schema version and does not belong in ordinary CLI or MCP
results.

Format versioning is applied at a protocol or private storage boundary. V1 HTTP
JSON uses `application/vnd.auv.run+json; version=1`. `FileRunStore` owns a
private, versioned, checksummed layout and rejects unsupported versions; that
layout is not a cross-implementation interchange format in V1. Callers do not
fill a `schema_version` or `api_version` string on every span, event, or
artifact. JSON unions use one externally tagged key such as `span_started` or
`artifact_published`; they do not add a generic `kind` field to every object.

JSON uses lowercase hyphenated UUID strings, integer revisions and lengths,
`Timestamp` objects with `unix_seconds` and `nanoseconds`, lowercase digest
hex, and canonical MIME text. Protocol decoders reject unknown object fields
and duplicate JSON keys rather than silently ignoring them.

### Idempotency And Response Loss

Idempotency keys are scoped by `(run_id, idempotency_key)`. For a new key the
store validates the complete request, serializes it with other accepted writes
for that run, allocates the next revision, and commits atomically. Ordinary
instrumentation appends do not predict the current revision. For a repeated
key the store returns the original commit only when the stable request
fingerprint matches.

The store must expose lookup by idempotency key so a caller can resolve an
unknown commit outcome without replaying the application operation or UI
action.

When a dispatch receives `CommitUnknown`, it performs one idempotency lookup.
An equal committed request resolves the submission. No result or a failed
lookup makes that dispatch's lane for the run indeterminate: the current flush
reports the routing error, and later facts for the same run are rejected
locally for the lifetime of the dispatch. This quarantine prevents a later
fact from reaching the authority before the unknown write. A caller starts a
new `RunId`; merely replacing the dispatch does not make the indeterminate run
safe to continue. The dispatch never resubmits the mutation and never invokes
application code as recovery.

Idempotency equality is defined over the validated typed request, not its
incoming JSON spelling or map order. An implementation may hash a deterministic
encoding, but two requests match only when their normalized fields, event JSON
values, and artifact integrity metadata are structurally equal. Numeric values
must not pass through JavaScript or IEEE-754 coercion before comparison.

### Snapshot And Reducer

A `RunSnapshot` is a read model through one revision:

```rust
pub struct RunSnapshot {
  authority_id: AuthorityId,
  run_id: RunId,
  through_revision: RunRevision,
  spans: BTreeMap<SpanId, SpanSnapshot>,
  events: Vec<EventOccurred>,
  artifacts: BTreeMap<ArtifactUri, ArtifactPublished>,
}

pub struct SpanSnapshot {
  started: SpanStarted,
  ended: Option<SpanEnded>,
}
```

`load_snapshot` deliberately materializes the complete read model and therefore
uses memory proportional to the run. Bounded consumers use `commits_after` and
`subscribe`; an Inspect client must not reject an otherwise valid snapshot at
an arbitrary transport-size threshold lower than the run contract.

`through_revision` means the snapshot includes all accepted commits up to that
revision. A viewer uses it as the subscription cursor; it is unrelated to
format compatibility.

Minimum reducer invariants are:

- revisions are contiguous and strictly increasing per store and run;
- every child span and event refers to an existing run-local parent scope;
- a span starts once and finishes at most once;
- finish time is not earlier than start time;
- a span-scoped event is committed after its span start and before its span end;
- an `EventId` is unique within its run and cannot be reused under another
  idempotency key, even for an equal payload;
- an artifact's optional span association refers to an existing span but may
  be published after that span ended;
- an artifact URI is unique within the run;
- facts in one commit become visible atomically;
- replaying the complete commit sequence produces the same snapshot;
- a cached snapshot identifies its source revision and can be discarded and
  rebuilt.

Events retain commit order and within-commit order in the snapshot. V1 does not
finalize runs: later facts and artifacts remain valid after any flush. A future
seal protocol must be designed as a separate versioned contract rather than
adding optional finished fields to this snapshot.

## Run Store

`RunStore` is the storage authority port. It is not an exporter, hook,
subscriber, application runtime, or operation handler.

```rust
pub type BoxFuture<'a, T> =
  Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type ArtifactBody =
  Pin<Box<dyn futures_io::AsyncRead + Send>>;

pub type ArtifactReader =
  Pin<Box<dyn Stream<Item = Result<Bytes, ArtifactReadError>> + Send>>;

pub type RunSubscription =
  Pin<Box<dyn Stream<Item = Result<RunCommit, SubscriptionError>> + Send>>;

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

pub trait RunStore: Send + Sync {
  fn authority_id(&self) -> AuthorityId;

  fn commit(
    &self,
    request: RunCommitRequest,
  ) -> BoxFuture<'_, Result<RunCommit, CommitError>>;

  fn write_artifact(
    &self,
    request: StoreArtifactRequest,
    body: ArtifactBody,
  ) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>>;

  fn lookup_commit(
    &self,
    run_id: RunId,
    key: IdempotencyKey,
  ) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>>;

  fn load_snapshot(
    &self,
    run_id: RunId,
  ) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>>;

  fn commits_after(
    &self,
    run_id: RunId,
    after_revision: RunRevision,
    limit: PageLimit,
  ) -> BoxFuture<'_, Result<RunCommitPage, ReadError>>;

  fn subscribe(
    &self,
    run_id: RunId,
    after_revision: RunRevision,
  ) -> BoxFuture<'_, Result<RunSubscription, ReadError>>;

  fn open_artifact(
    &self,
    uri: ArtifactUri,
  ) -> BoxFuture<'_, Result<ArtifactReader, ReadError>>;
}

pub enum CommitError {
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
  IdempotencyMismatch,
  Rejected(ErrorCode),
  Unavailable(ErrorCode),
  CommitUnknown(ErrorCode),
}

pub enum ArtifactWriteError {
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
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
  HistoryGap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  CursorAhead {
    requested_after: RunRevision,
    latest: RunRevision,
  },
  Unavailable(ErrorCode),
  Integrity(ErrorCode),
}

pub enum ArtifactReadError {
  Unavailable(ErrorCode),
  Integrity(ErrorCode),
}
```

Every write request's `authority_id` must equal `RunStore::authority_id`; a
mismatch is rejected before mutation validation or body polling. The authority
derives the `ArtifactUri` from `StoreArtifactRequest.{run_id, artifact_id}`. The
caller cannot submit both IDs and a separately encoded URI. A successful
`write_artifact` appends one
artifact-publication fact and returns that `RunCommit`. `lookup_commit` uses the
same idempotency table for ordinary and artifact writes, so it resolves response
loss without uploading again or repeating application work. An equal repeated
artifact request returns the original commit without polling the replacement
body; a different key cannot overwrite the derived artifact URI.

`RunCommitPage.last_revision` is the last returned revision, or the requested
`after_revision` when the page is empty. A subsequent page starts after that
value. A page stops before adding a commit that would take its compact canonical
JSON representation above 32 MiB and sets `has_more`; because one valid commit
is itself below that boundary, a non-empty remainder always makes progress.
`commits_after` returns `HistoryGap` when retained history can no longer
serve the cursor and `CursorAhead` when the requested cursor exceeds the
authority's latest revision; neither condition is represented as an empty page.

`load_snapshot` returns `Ok(None)` only when the authority has no committed fact
for that run ID. Creating a root `Context` alone does not write a commit.
Revision zero is the cursor before the first accepted fact.

The single object-safe `RunStore` trait above is the V1 authority port. Typed
generic decoding helpers do not belong on it. Raw readers return bounded
records and byte streams; typed decoding belongs in extension traits or free
functions.

Expected implementations and roles are:

| Component | Contract |
|---|---|
| disabled dispatch | Accepts instrumentation call sites without installing a store or creating runs. |
| memory run store | Complete in-process commit/read/artifact/subscription implementation for tests, REPLs, and short sessions. History is unbounded by default; an explicit history limit enables bounded-session gap behavior. |
| file run store | Durable local commits and private content-addressed artifact storage. |
| Inspect Server client | Remote run facts, binary upload, resolution, and read protocol as the selected authority store. |
| OTLP projection | Bounded external telemetry projection, never a `RunStore`. |

The concrete public names for these responsibilities are fixed in
`Feature And Crate Boundaries` below.

`FileRunStore::open(root)` accepts one administrator-selected root directory.
It derives all run-log, temporary-object, and content-addressed blob locations
from validated IDs and SHA-256 digests below that root. Per-artifact APIs never
accept a path. The implementation rejects symlink escapes, writes blobs through
private temporary files, makes blob data durable before publishing metadata,
and serializes commits under a per-run lock. A partial final log entry is
discarded during recovery; an integrity failure in an earlier committed entry
fails the read rather than being skipped.
Indexes loaded at `open` are caches. After acquiring the per-run lock, every
writer rereads and verifies the current log tail before allocating a revision
or applying idempotency/event/artifact checks. Two `FileRunStore` values opened
on the same root therefore cannot commit from stale in-memory indexes.

### Subscription And Gap Recovery

A run subscription starts after an explicit revision and emits later commits in
order. It must not silently skip data when a consumer lags.

```rust
pub enum SubscriptionError {
  Gap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  Store(ReadError),
}
```

Viewer startup is race-free by contract:

```text
load snapshot through revision R
  -> subscribe(after_revision = R)
  -> receive R+1 or an explicit Gap
```

On `Gap`, the consumer reloads a snapshot or pages committed history and then
subscribes again. A broadcast channel may provide process-local wakeups but is
not the recovery authority.

## Inspect Server Run Protocol

When Inspect Server is the authority, `InspectRunStore` maps the store port to
these V1 endpoints:

| Operation | HTTP endpoint |
|---|---|
| authority identity | `GET /v1/authority` |
| commit mutations | `POST /v1/runs/{run_id}/commits` |
| idempotency lookup | `GET /v1/runs/{run_id}/commits/by-idempotency-key/{key}` |
| snapshot | `GET /v1/runs/{run_id}/snapshot` |
| committed page | `GET /v1/runs/{run_id}/commits?after_revision={revision}&limit={limit}` |
| live commits | `GET /v1/runs/{run_id}/commits/stream?after_revision={revision}` |

Commit requests and JSON responses use
`application/vnd.auv.run+json; version=1`. The commit request carries the
idempotency key in `Idempotency-Key`; its JSON body contains `authority_id` and
the externally tagged `RunMutation` array. The path supplies `run_id`. Clients
cannot submit `ArtifactPublished`, and an authority mismatch returns 409.

`GET /v1/authority` returns the server's stable `AuthorityId` in the shape
`{"authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa"}`.
`InspectRunStore::connect` fetches and caches this value before it can be
installed in a `Dispatch`; `RunStore::authority_id` therefore remains
synchronous.

A new commit returns HTTP 201; an equal idempotent replay returns HTTP 200 with
the original `RunCommit`; a mismatched replay returns 409. Lookup returns the
same commit or 404. Snapshot returns `RunSnapshot` or 404. Paging returns
`RunCommitPage` and applies the `PageLimit` bound.

Run API errors use the same versioned media type and one externally tagged
variant:

```rust
pub enum RunApiError {
  NotFound,
  Forbidden,
  InvalidReference { code: ErrorCode },
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
  IdempotencyMismatch,
  Rejected { code: ErrorCode },
  HistoryGap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  CursorAhead {
    requested_after: RunRevision,
    latest: RunRevision,
  },
  Integrity { code: ErrorCode },
  Unavailable { code: ErrorCode },
}
```

`NotFound` maps to 404, `Forbidden` to 403, `InvalidReference` to 400,
`AuthorityMismatch`, `IdempotencyMismatch`, and `CursorAhead` to 409, `Rejected` to 422,
`HistoryGap` to 410, `Integrity` to 500, and `Unavailable` to 503. For example,
a paged read that can no longer serve revision 4 returns:

```json
{
  "history_gap": {
    "requested_after": 4,
    "earliest_available": 9
  }
}
```

`InspectRunStore` reconstructs the corresponding typed store error from the
variant and never attempts to infer a gap from an empty success response.

The live endpoint is Server-Sent Events. Each `commit` event uses its revision
as the SSE `id` and one `RunCommit` as JSON `data`. A `gap` event carries
`requested_after` and `earliest_available`; the server then closes the stream.
Reconnect uses the greater of the query cursor and a valid `Last-Event-ID`.
SSE is a notification transport, not the recovery authority; every gap is
recovered through snapshot or committed-page reads.

## Attributes

Attributes are bounded searchable scalar metadata, not a payload escape hatch.

```rust
pub struct Attributes(BTreeMap<AttributeKey, AttributeValue>);

pub enum AttributeValue {
  Bool(bool),
  I64(i64),
  F64(FiniteF64),
  String(BoundedString),
}
```

The type enforces namespaced keys, entry count, string size, and total encoded
size. V1 does not admit bytes, arbitrary JSON objects, null, or heterogeneous
arrays. Invalid values return a typed error; there is no silent flattening,
stringification, redaction, or dropped-count field in the initial contract.

V1 limits are:

- namespaced names contain at least two lowercase ASCII segments separated by
  `.`, each segment starts with `[a-z]`, and remaining characters are
  `[a-z0-9_]`;
- a complete namespaced name is at most 128 UTF-8 bytes;
- an `Attributes` value contains at most 32 entries;
- one string value is at most 1,024 UTF-8 bytes;
- integer values stay within `-(2^53-1)..=(2^53-1)` for exact JSON transport;
- one attributes map is at most 16 KiB in compact UTF-8 JSON form.

Structured data belongs in a typed event payload. Large or binary data belongs
in an artifact.

## Artifacts

An artifact consists of committed metadata and bytes addressable by one AUV
artifact URI. It is not a client-provided filesystem path.

```rust
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
```

`ArtifactPurpose` is a bounded namespaced query field for stable relationships
such as a capture, input evidence, or operation output. Inspect viewers and
operation-specific readers may filter artifacts by purpose without decoding
arbitrary attributes. Purpose does not identify the byte format and does not
replace `ContentType`.

Committed metadata always contains digest and byte length. V1 writers also
require expected digest and length before consuming the one-shot body; the
store computes both and rejects a mismatch. They are requirements on the write,
not optional fields on the committed fact.

One V1 artifact is limited to 512 MiB (536,870,912 bytes). A deployment may
advertise and enforce a lower limit but cannot accept a larger V1 artifact.
Continuous media uses the deferred stream contract rather than raising this
whole-object limit.

The write lifecycle is:

```text
typed metadata + byte stream
  -> private temporary/content-addressed object
  -> compute and validate length and digest
  -> atomically publish artifact metadata and bytes
  -> append ArtifactPublished in the authority RunCommit
  -> return that RunCommit
```

Only after publication may the artifact URI appear in committed facts or
external telemetry. A failed metadata commit may leave an unreachable blob;
stores need startup or periodic garbage collection. Readers verify length and
digest and report integrity failure if the stream does not match metadata.

Caller-controlled local paths are forbidden. A local or remote store derives
its private locator from validated IDs or a digest. Artifact URIs contain no
filesystem components supplied by an untrusted producer.

### Inspect Server Upload And Read

Inspect Server is a binary resource service as well as a run-data query server.
Upload uses a short-lived draft followed by one streaming content write. The
upload resource is not a canonical run fact. A pending upload expires; after
successful publication it retains only the committed lookup needed for
idempotent replay and contains no second copy of artifact metadata.

```http
POST /v1/runs/{run_id}/artifact-uploads
Content-Type: application/vnd.auv.artifact-upload+json; version=1
Idempotency-Key: 019f8b1e-4b2d-7a00-8f00-000000000006
```

```json
{
  "authority_id": "019f8b1e-4b2d-7a00-8f00-0000000000aa",
  "artifact_id": "019f8b1e-4b2d-7a00-8f00-000000000002",
  "span_id": "019f8b1e-4b2d-7a00-8f00-000000000004",
  "purpose": "display.capture",
  "content_type": "image/png",
  "byte_length": 182734,
  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "attributes": {}
}
```

`span_id` may be omitted when the artifact is run-scoped. The server derives
the canonical `ArtifactUri` from validated path and artifact IDs; the request
cannot provide a path, object key, or locator. A successful new draft returns
HTTP 201:

```json
{
  "upload_id": "019f8b1e-4b2d-7a00-8f00-000000000005",
  "artifact_uri": "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002",
  "expires_at": {
    "unix_seconds": 1784620800,
    "nanoseconds": 0
  }
}
```

`upload_id` is an Inspect-protocol `ArtifactUploadId` UUID newtype, not an
artifact or run ID. `expires_at` uses the same validated `Timestamp` wire shape
as run data; the draft response does not introduce a second timestamp format.

Repeating the POST with the same idempotency key and structurally equal
metadata returns the same draft with HTTP 200. Reusing the key or artifact ID
for different metadata returns HTTP 409. Drafts expire 24 hours after creation;
idempotent replay does not extend the deadline, and content upload after expiry
returns 410. When `span_id` is present, the span must already exist in that
authority; it may have ended before artifact publication.

An artifact ID is immutable within its run. A different idempotency key cannot
reserve or overwrite an existing artifact ID even when metadata and bytes are
identical.

The client then streams the unencoded artifact bytes:

```http
PUT /v1/runs/{run_id}/artifact-uploads/{upload_id}/content
Content-Type: image/png
Content-Digest: sha-256=:ASNFZ4mrze8BI0VniavN7wEjRWeJq83vASNFZ4mrze8=:
```

The request may use HTTP chunked transfer; the server does not buffer the full
body in memory. `Content-Type` and the RFC 9530 `Content-Digest` value must match
the draft. The server counts and hashes decoded content while writing a private
temporary object, rejects excess bytes immediately, and publishes only after
the final length and SHA-256 match.

The first successful publication returns HTTP 201 with
`Content-Type: application/vnd.auv.run+json; version=1` and the complete
`RunCommit`:

```json
{
  "authority_id": "019f8b1e-4b2d-7a00-8f00-0000000000aa",
  "run_id": "019f8b1e-4b2d-7a00-8f00-000000000001",
  "revision": 7,
  "idempotency_key": "019f8b1e-4b2d-7a00-8f00-000000000006",
  "committed_at": {
    "unix_seconds": 1784534400,
    "nanoseconds": 0
  },
  "facts": [
    {
      "artifact_published": {
        "span_id": "019f8b1e-4b2d-7a00-8f00-000000000004",
        "metadata": {
          "uri": "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002",
          "purpose": "display.capture",
          "content_type": "image/png",
          "byte_length": 182734,
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          "attributes": {}
        }
      }
    }
  ]
}
```

A repeated PUT for an already published draft returns the same response with
HTTP 200 without consuming another body. If the response is lost and the
one-shot body cannot be replayed, the client uses the run store's
idempotency-key commit lookup; it does not
re-execute the application operation. Integrity mismatch returns 422,
the V1 or advertised deployment size limit returns 413, and neither case
publishes a run fact.

Malformed metadata or digest syntax returns 400; failed authentication or
authorization returns 401 or 403; an unknown span or upload returns 404; an
expired upload returns 410; idempotency or artifact identity conflict returns
409; and authority unavailability returns 503. Error responses use the
endpoint's versioned JSON media type and the shape
`{"error":"auv.inspect.namespaced_code"}`. They contain no recursive cause
object or retry recommendation.

Original bytes are read with:

```http
GET /v1/runs/{run_id}/artifacts/{artifact_id}
```

A successful response uses the committed content type and length and includes
`Content-Digest`. An unpublished, expired, or failed upload is not readable as
an artifact. Range and resumable multipart protocols are outside V1.

### Artifact URI Resolution

Canonical AUV facts and OTLP projections carry `ArtifactUri`, not a deployment
specific `https://` locator.

The accepted batch endpoint is:

```http
POST /v1/resources/artifacts/resolve
Content-Type: application/json
```

```json
{
  "authority_id": "019f8b1e-4b2d-7a00-8f00-0000000000aa",
  "uris": [
    "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002",
    "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000003"
  ]
}
```

The response contains one result for every input URI and supports partial
success:

```json
{
  "results": [
    {
      "available": {
        "uri": "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002",
        "content_type": "image/png",
        "byte_length": 182734,
        "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "content_url": "https://inspect.example/v1/runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002"
      }
    },
    {
      "not_found": {
        "uri": "auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000003"
      }
    }
  ]
}
```

Each result is an externally tagged `available` or `not_found` variant. The
response order matches the request order. Duplicate input URIs produce duplicate
same-position results. A syntactically valid authorized batch returns HTTP 200
with one per-item result. Malformed input, failed request authorization, an
oversized batch, or server-wide failure uses request-level 4xx/5xx handling.
An `authority_id` mismatch returns 409 before resolution. The V1 maximum is 256
URIs per request.

`content_url` is an absolute resolver-specific HTTP(S) locator and may vary by
deployment. It is not persisted as canonical AUV identity and must not contain
bearer credentials or long-lived signed secrets.

The browser fetches the original bytes from `content_url`. The resolver does
not base64-encode arbitrary images or video into the JSON response.

This contract does not define `references:resolve`, a generic `AuvRef`, or a
custom generic resource service. Other resource families get their own API
only after a concrete producer and consumer require one.

## Inspect Server Roles

Inspect Server supports these concrete responsibilities:

| Responsibility | Contract |
|---|---|
| run write | Accept validated run facts according to the selected store/route policy. |
| artifact upload | Accept bytes and metadata without accepting a caller filesystem path. |
| artifact resolution | Resolve AUV artifact URIs in batches. |
| artifact read | Return original bytes through a resolver-specific content locator. |
| run read | Return snapshots and committed history. |
| live inspection | Subscribe from a revision and report gaps. |
| viewer projection | Build captions, layouts, joins, and previews without changing canonical facts. |

Inspect Server participates in V1 only as the selected `RunStore` authority.
Calling remote persistence a hook would be incorrect because upload
acknowledgement, idempotency, and durability affect whether a fact committed.

## Rust Tracing And OpenTelemetry Projection

The canonical direction is:

```text
AUV typed API
  -> AUV validation and routing
  -> bounded Rust tracing projection and/or OTEL projection
```

It is not:

```text
arbitrary tracing event containing bytes
  -> multiple sibling Layers
  -> hope that the OTEL Layer filters the bytes
```

Projectors do not receive arbitrary `RunFact` values. The router first produces
a closed telemetry projection containing only bounded scalar attributes and
canonical AUV URI strings. Rust tracing and OTEL integrations accept that safe
projection type. Every projected item includes `auv.run.id` and includes
`auv.authority.id` when an authority exists. Single-fact committed signals use
`auv.run.revision`; a projected span uses separate start and end revisions.
The fixed field vocabulary is:

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

It never includes bytes from any source, unbounded structured event payloads,
local paths, object-store keys, upload credentials, or resolver-specific
content URLs. Digests use lowercase hexadecimal text in telemetry.

The V1 OTEL mapping is:

| AUV fact | OTEL signal |
|---|---|
| `SpanStarted` + `SpanEnded` | One OTEL span named by `SpanName`, with distinct `auv.span.start_revision` and `auv.span.end_revision` when durable. A root-level AUV span starts a new OTEL trace; a child uses the projector's OTEL parent mapping. AUV and OTEL IDs remain independent. |
| span-scoped `EventOccurred` | An event on the corresponding live OTEL span, named by `EventSchema.name`. The canonical JSON payload is omitted. |
| run-scoped `EventOccurred` | An OTEL event-form LogRecord with `EventName` set from the schema and no fabricated trace/span context. |
| `ArtifactPublished` | An OTEL event-form LogRecord named `auv.artifact.published`, containing the artifact URI and bounded metadata. It remains a log even when associated with an AUV span because publication may finish after that OTEL span ended. |

OTEL span status remains `Unset`; AUV span end, delivery facts, app errors, and
verification facts do not automatically set `Ok` or `Error`. The AUV remote
link is exported as AUV correlation attributes. A standard OTEL span link is
added only when the application separately propagated valid W3C Trace Context
and the OTEL SDK supplied that context.

Canonical event JSON and artifact attributes are not projected by default.
The OTEL route configuration may allowlist individual scalar span or artifact
attribute keys at runtime. The Rust `tracing` projector uses only the fixed
vocabulary above because tracing callsite fields are static. The router applies
each route's policy while constructing its `TelemetryItem`; a projector never
receives rejected fields.

OTLP is an optional wire projection for external observability and frontend
aggregation. A frontend groups projected data by AUV IDs and batch-resolves
artifact URIs through its connected Inspect Server. The AUV run store remains
the complete read source for replay and inspection. Without a run store, the
same span and event mapping is allowed but has no `auv.run.revision`, durable
history, replay, or artifact publication.

`tracing-opentelemetry::Layer` may still be used by an application for its
ordinary Rust tracing spans. It is not the AUV artifact router. If installed
beside an AUV compatibility layer, per-layer filters can reduce diagnostic
volume but cannot sanitize a binary payload that should never have entered a
tracing event.

The Rust `tracing` projector uses the static callsite names `auv.span`,
`auv.event`, and `auv.artifact.published`, with the AUV names and IDs in bounded
fields. Rust `tracing` metadata requires static callsites, so a dynamic
`SpanName` is stored in the fixed `auv.span.name` field rather than callsite
metadata. The span callsite declares both revision fields up front and records
the end revision when `SpanEnded` arrives. Projected callsites use a reserved
target excluded from AUV ingestion to prevent projection loops.

V1 emits no OTEL metrics. Export failure does not alter accepted AUV facts,
span status, application results, or verification claims.

## CLI, MCP, Libraries, And REPLs

`auv-tracing` does not own one invoke output model.

- CLI returns and renders the result of the Rust operation it called.
- MCP returns its protocol-specific result.
- library callers receive the typed Rust value directly.
- each frontend may include `RunId` and artifact URIs when useful.
- none of them reads an immediate result back from the trace store.
- timeline events and expanded artifact descriptors belong to inspect queries
  unless a frontend explicitly requests them.

An app crate can expose atomic operations without depending on a central AUV
runtime. A third-party application can compose `auv-driver-*`, app operations,
and its own control flow, then opt into AUV tracing at its composition root.

A REPL owns its command loop and script state. It may keep one run root active
across multiple operations or create a run per command. Context propagation and
the run store work the same way; no `RunSession` execution abstraction is
required in `auv-tracing`.

## Feature And Crate Boundaries

The intended dependency direction is:

```text
drivers and app operations
  -> optional lightweight auv-tracing instrumentation surface

application composition root
  -> selects concrete stores, Inspect client, live inspection, and telemetry
```

The `auv-tracing` core crate contains:

- validated IDs and bounded values;
- `Context` and `Dispatch`;
- the runtime-independent default `ThreadTaskSpawner` and the `TaskSpawner`
  port for runtime-bound integrations;
- typed functions and macro frontends;
- routing ports and failure policy;
- run fact and artifact contracts;
- object-safe store/read ports;
- no-op behavior when disabled.

Concrete integrations are divided as follows:

| Package or feature | Public responsibility |
|---|---|
| `auv-tracing` default surface | Typed API, macros, `Context`, `Dispatch`, facts, artifacts, and ports. No HTTP, filesystem, Tokio runtime, or OTEL SDK dependency. |
| `auv-tracing/memory-store` | `MemoryRunStore` for tests, REPLs, and short-lived applications. |
| `auv-tracing/file-store` | `FileRunStore` with private content-addressed blobs and durable commit logs. |
| `auv-tracing/rust-tracing` | `RustTracingProjector` only. |
| `auv-tracing-inspect` | `InspectRunStore` authority client over the Inspect HTTP protocol. |
| `auv-tracing-otel` | `OtelProjector` into an application-supplied OpenTelemetry SDK provider. It is not an OTLP exporter. |

Driver and app-operation crates make their dependency on `auv-tracing`
optional behind their own `tracing` feature. Enabling that feature adds call
sites but does not install a dispatch or select a store.

`opentelemetry_sdk`, OTLP transport dependencies, HTTP clients, and local file
store dependencies do not become mandatory dependencies of driver-only
consumers. AUV context propagation does not depend on `opentelemetry::Context`;
applications may install standard OTEL propagation separately.

## Deferred Work

| Deferred surface | Reason | Re-open trigger |
|---|---|---|
| Generic verification model | Verification domains, assertions, evidence, and verdict policy are not yet shared across proven producers and consumers. | A separate verification design demonstrates shared semantics. |
| Generic observation model | AX, OCR, vision, driver acknowledgements, and app state do not share one proven value/confidence model. | At least two producers and one query consumer need the same normalized type. |
| Continuous media stream model | Timebase, ordering, buffering, seek, and retention requirements are not approved. | A recording or continuous-frame slice defines a concrete producer and viewer. |
| Run sealing and amendment protocol | V1 runs intentionally remain appendable; flush is a durability barrier, not a terminal state. | A consumer requires an immutable or signed run boundary. |
| Store replication | V1 selects one authority and defines no local-to-Inspect copy path. | A use case requires revision-preserving replication, artifact copy, and resume. |
| Resumable multipart upload | Whole-object/streaming upload is enough to define the first atomic artifact boundary. | Approved artifacts exceed deployment limits or require resume. |
| Generic resource resolver | Only artifacts currently require cross-system resolution. | Another resource family has a concrete producer and consumer. |
| Automatic tracing ingestion | Arbitrary Rust tracing fields do not satisfy typed AUV contracts. | A bounded compatibility use case is approved. |

Deferral means the type or behavior is intentionally absent. It is not approval
to add placeholder enums or optional fields to avoid making the decision later.

## Review State

The V1 contract has no unresolved design placeholders and was approved by the
owner for implementation on 2026-07-20. Deferred surfaces above are absent from
V1 by decision and must not be represented by placeholder fields, enum variants,
or compatibility shims.

## Required Contract Tests

Implementation approval requires at least:

- no dispatch and no current run create no implicit run;
- creating a root context without emissions creates no stored run;
- constructing a root context does not replace the current context;
- a propagated authority mismatch is rejected before any local fact is
  submitted;
- direct store writes with another `AuthorityId` are rejected before mutation
  validation or artifact body polling;
- disabled instrumentation does not change an app operation's direct result;
- concurrent async runs retain independent `RunId` and parentage;
- spawned work receives only explicitly propagated context;
- dropping an enter guard does not end its span;
- the last span/context clone emits exactly one span end;
- dropping an instrumented future ends its span without adding a cancellation
  or failure status;
- both future wrappers poll and destroy their inner futures under the captured
  context, including cancellation before `Ready`;
- event emission returns `()` and routes encoding or submission failure to the
  configured reporter and next flush result;
- flush covers every pre-barrier emission, does not end spans, and permits
  later emissions;
- one projector receives ordered items and its flush acknowledgement is part of
  the dispatch barrier;
- the default task spawner drives runtime-independent adapters, while an
  explicitly selected runtime spawner drives runtime-bound adapters;
- span end records no success, failure, cancellation, or verification status;
- a missing span end remains unfinished without an inferred reason;
- an event after its referenced span end is rejected;
- reusing an `EventId` under another idempotency key is rejected;
- remote propagation creates links on direct local roots and never fabricates
  a local parent;
- invalid, duplicate, partial, and unknown-version propagation fields fail
  extraction;
- typed attributes reject bytes, arbitrary JSON, and fixed V1 size excess;
- typed event JSON over 64 KiB is rejected without reaching a store or
  projector;
- artifact bytes cannot reach the Rust tracing or OTEL projector interfaces;
- disabled or telemetry-only artifact emission returns `Ok(None)` without
  polling the body;
- dropping an artifact receipt future does not cancel the accepted write; the
  job commits or reports one terminal failure;
- detached artifact correlation allows publication after the associated span
  has ended without delaying `SpanEnded`;
- a slow artifact body waits for its referenced span start but does not block
  later span/event commits;
- OTEL projection contains artifact URI and bounded metadata only;
- OTEL span status remains unset and AUV IDs are not reused as OTEL IDs;
- OTEL spans expose distinct start/end revisions, while event and artifact
  signals expose their single commit revision;
- Rust `tracing` projection uses only its fixed callsite field vocabulary;
- canonical event JSON is absent from Rust tracing and OTEL projections;
- `ArtifactUri` accepts one canonical form and rejects traversal, query,
  fragment, invalid IDs, and non-canonical escaping;
- interrupted artifact writes never publish metadata without committed bytes;
- committed artifact reads verify content type, digest, and length;
- callers cannot select or overwrite local store paths;
- Inspect Server accepts binary uploads and serves the original bytes;
- upload draft replay is idempotent, content digest and length mismatches
  publish no fact, and response loss is recoverable by idempotency lookup;
- equal artifact replay does not poll a replacement body, and a different key
  cannot overwrite the same artifact ID;
- batch artifact resolution returns one same-position result per requested URI
  and supports partial failure;
- batch resolution rejects an authority mismatch before resolving any URI;
- resolver-specific content URLs are not written back as canonical artifact
  identity;
- response-loss recovery uses idempotency lookup without re-executing an app
  operation or UI action;
- snapshot plus subscription cannot silently lose an intervening commit;
- HTTP history-gap and cursor-ahead bodies reconstruct the matching typed read
  errors;
- SSE reconnect respects the requested revision and `Last-Event-ID`;
- lagging subscriptions receive an explicit gap and recover from store reads;
- rebuilding a snapshot from the complete commit sequence is deterministic;
- a backward wall-clock adjustment cannot produce an end timestamp before the
  locally captured span start;
- OTEL or Inspect projection failure does not fabricate operation success,
  verification success, or retry advice;
- CLI, MCP, and library callers can use the same instrumented app operation
  without depending on a shared runner.

## References

- [Earlier AUV tracing research and repository audit](2026-07-17-auv-tracing-contract-and-invoke-output-design.md)
- [AUV terms and concepts](../../../TERMS_AND_CONCEPTS.md)
- [Rust tracing](https://docs.rs/tracing/latest/tracing/)
- [Rust Reference: dyn compatibility](https://doc.rust-lang.org/stable/reference/items/traits.html#dyn-compatibility)
- [OpenTelemetry Context](https://docs.rs/opentelemetry/latest/opentelemetry/context/struct.Context.html)
- [OpenTelemetry trace API](https://opentelemetry.io/docs/specs/otel/trace/api/)
- [OpenTelemetry logs data model](https://opentelemetry.io/docs/specs/otel/logs/data-model/)
- [OpenTelemetry Protocol](https://opentelemetry.io/docs/specs/otlp/)
- [RFC 9530: Digest Fields](https://www.rfc-editor.org/rfc/rfc9530)
- [Tokio broadcast lag behavior](https://docs.rs/tokio/latest/tokio/sync/broadcast/)
- [object_store](https://docs.rs/object_store/latest/object_store/trait.ObjectStore.html)
- [Godot resource paths](https://docs.godotengine.org/en/latest/tutorials/io/data_paths.html)
- [Godot ResourceUID](https://docs.godotengine.org/en/stable/classes/class_resourceuid.html)
