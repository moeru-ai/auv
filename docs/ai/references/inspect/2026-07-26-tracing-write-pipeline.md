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
  artifact bodies by run/artifact identity. It intentionally exposes no reader.
- `TraceExporter` is independent from storage. Export failures do not prevent
  full-fidelity storage.
- `auv-tracing-otel::OtelExporter` uses application-supplied SDK providers and
  keeps only transient span-pairing state. It emits no authority or revision
  vocabulary.

## Removed model

`RunStore`, `RunCommit`, `RunRevision`, `RunSnapshot`, `AuthorityId`,
`IdempotencyKey`, the reducer, cursors, subscriptions, artifact readers, file
recovery, and the old tracing conformance crate are removed without
compatibility aliases. Consumers that depended on those APIs represented the
rejected architecture and must be deleted or redesigned at the later read-side
boundary.

## Deferred read side

`auv-inspector` is explicitly out of scope. The existing inspect server and
inspect model are not substitutes for it. A later owner-approved slice may
define ingestion, indexing, artifact resolution, and viewer APIs over data
written by a tracing store.

## Verification seam

Tests configure a public dispatch with concrete recording destinations, emit
through `Context`/span/event/artifact APIs, flush, and assert observable records
or exported SDK data. Tests do not inspect dispatch worker state.
