# AUV Tracing Write Pipeline

Status: current implementation handoff

Issue: [#142](https://github.com/moeru-ai/auv/issues/142)

## Decision

`auv-tracing` is a producer-side instrumentation library. Its durable contract
is an ordered stream of full-fidelity `TraceRecord` values plus artifact bytes.
It does not own run authority, commits, revisions, snapshots, reducers,
pagination, subscriptions, recovery, sessions, or inspection read models.

The public pipeline is:

```text
Context / span / event / artifact
  -> Dispatch
  -> TracingStore (full-fidelity, write-only)
  -> TraceExporter(s) (lossy external telemetry)
```

`RunId` is correlation only. Application operations continue returning their
typed result directly; tracing cannot reconstruct or replace it.

## Public boundaries

- `TraceRecord` contains span starts, span ends, canonical typed event payloads,
  and stored artifact metadata.
- `TracingStore` exposes only record write, artifact write, and flush.
- `MemoryTracingStore` exposes copied records and artifact bytes only as
  concrete test helpers, not through the generic port.
- `FileTracingStore` appends versioned JSON Lines record envelopes and stores
  artifact bodies by run/artifact identity. Byte emitters may declare a file
  extension through `EmitBytesOptions`; the file-store adapter appends that
  extension without deriving it from content type. Artifact URIs remain
  independent from that local layout. The generic port still exposes no
  reader; since 2026-10-21 the file layout has a sequential reader
  (`FileTracingStore::read_records`, envelope version 2, artifact
  `occurred_at`). See
  [`2026-10-21-trace-and-log-responsibility-split.md`](2026-10-21-trace-and-log-responsibility-split.md).
- Trace names, error codes, artifact purposes, content types, and attribute
  keys are caller-declared strings. `auv-tracing` preserves them without
  namespaced-name, MIME, count, or encoded-size validation. Structural and
  storage invariants such as UUID identity, finite JSON numbers, byte limits,
  and artifact integrity remain enforced.
- `TraceExporter` is independent from storage. Export failures do not prevent
  full-fidelity storage.
- `auv-tracing-otel::OtelExporter` uses application-supplied SDK providers and
  keeps only transient span-pairing state. It emits no authority or revision
  vocabulary.

## Removed model

`RunStore`, `RunCommit`, `RunRevision`, `RunSnapshot`, `AuthorityId`,
`IdempotencyKey`, the reducer, cursors, subscriptions, artifact readers, file
recovery, and the old tracing conformance crate are removed without
compatibility aliases. The legacy inspect model, inspect server, tracing
inspection adapter, CLI inspect surface, and game-specific read adapters were
removed when the producer migration completed. Remaining operation code emits
records and artifacts through `TracingStore`; a later read side must define its
own ingestion contract over those durable records.

## Deferred read side

`auv-inspector` is explicitly out of scope. No generic inspection API is
provided as a compatibility substitute. The file store's sequential reader and
the `auv export atif` projection (2026-10-21) are the only read paths; they do
not index, cursor, or subscribe. A later owner-approved slice may define
ingestion, indexing, artifact resolution, and viewer APIs over data written by a
tracing store.

## Verification seam

Tests configure a public dispatch with concrete recording destinations, emit
through `Context`/span/event/artifact APIs, flush, and assert observable records
or exported SDK data. Tests do not inspect dispatch worker state.
