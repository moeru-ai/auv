# auv-tracing-driver Runtime Recording Split Spec

Status: implementation landed for durable recording extraction; interaction tracing remains deferred

Update 2026-06-11: the legacy JSON `skill`/recipe/case-matrix lane has been
removed by PR #35. This document remains useful for the `auv-tracing-driver`
recording boundary, but any statement that assumes JSON recipes still exist as
`Runtime` callers is historical and should not guide new implementation.

Update 2026-06-16: `auv-cli-invoke` now owns command metadata and
`src/catalog.rs` has been removed. The next useful simplification is to extract
the durable run/span/event/artifact recorder from `Runtime`. The design should
follow the Rust ecosystem's observability split: library crates may emit
`tracing` spans/events, while binaries and servers configure subscribers and
OpenTelemetry exporters. AUV's durable run store remains product state, not a
best-effort telemetry side effect.

Scope classification: approved feature slice

## Purpose

Extract run/span/event/artifact recording from `Runtime` so typed driver calls
and Rust orchestration code can record inspectable evidence without depending
on the legacy command runtime.

This PR creates the recording substrate needed for direct typed driver
invocation. It should remove most of the practical reason for `runtime.rs` to
exist after `auv-cli-invoke` has taken command compatibility.

The PR should prove the new recording boundary with the smallest useful typed
operation coverage. It is not the broad command migration PR.

## Current State

Before extraction, `src/runtime.rs` still owned several unrelated responsibilities:

- starts and finishes runs
- records spans and events
- stages file artifacts and artifact refs
- historically resolved command ids through `src/catalog.rs`; after PR #36 it routes invoke resolution through `auv-cli-invoke`
- invokes legacy drivers through `DriverCall`
- previously invoked bundle-era recipe commands, retired 2026-06-11
- previously invoked JSON skill/recipe/case-matrix commands, retired by PR #35
- exposes read/inspect helper methods
- hosts recorded operation helpers through `recorded_operation.rs`

`recorded_operation.rs` described itself as the bridge between typed
Rust driver APIs and AUV run recording, but before this extraction it still
depended on `Runtime`.

## Target Boundary

Create an `auv-tracing-driver` boundary for driver-level recording:

- run lifecycle
- span lifecycle
- event recording
- artifact staging
- artifact refs
- local store persistence
- recorder fan-out to local snapshots and inspect server write mode

This boundary records evidence for atomic driver operations. It should not own
command compatibility, retired JSON recipe execution, retired bundle lookup, or
UI-specific interaction loops.

## Ecosystem Reference Notes

Local reference clones were captured under `/Users/neko/Git/github.com` on
2026-06-16. These repositories are references for API shape and layering, not
implementation dependencies:

- `tokio-rs/tracing` at `d9d4c54`: use as the model for library-side
  instrumentation. Libraries emit structured spans/events and do not own global
  subscriber setup.
- `tokio-rs/console` at `59e23ed`: use as the model for a layer plus consumer
  architecture. `console_subscriber::init` is convenient for applications,
  while `ConsoleLayer::builder().spawn()` returns a layer that callers compose
  into their own subscriber. AUV should prefer this composable shape for any
  future live-view bridge.
- `getsentry/sentry-rust` at `e33b7ff`: use `sentry-tracing` as the model for
  mapping `tracing` spans/events into product-specific records through filters
  and mappers. This validates a future AUV tracing bridge, but also shows that
  product semantics should be explicit and configurable.
- `open-telemetry/opentelemetry-rust` at `8882149`: use as the model for API
  versus SDK/exporter separation. Instrumentation libraries can use generic
  tracing APIs and named instrumentation scopes; applications configure
  providers/exporters.

Design guidance from those references:

- Do not make `auv-tracing-driver` call `tracing::subscriber::set_global_default`
  or initialize OTel exporters. That belongs in binaries, test harnesses, or
  inspect/server setup.
- Do not make durable artifact staging depend on a subscriber being installed.
  Missing telemetry configuration must not make AUV lose run records.
- Do emit `tracing` spans/events from the explicit recorder path using stable
  structured fields such as `auv.run_id`, `auv.span_id`, `auv.driver_id`,
  `auv.operation`, `auv.artifact_id`, and `auv.artifact_role`.
- Treat OpenTelemetry export as a bridge/layer around emitted `tracing` data,
  not as the owner of AUV's persisted run model.
- If AUV later adds a live inspector layer, follow the `console-subscriber`
  split: a composable layer for applications that already own a subscriber, and
  a convenience initializer only in binaries.

## API Shape

The exact type names are provisional, but the split should expose concepts
equivalent to:

```rust
let mut run = recorder.start_run(spec)?;
let span = run.start_span("auv.driver.invoke", attributes)?;
let artifact_ref = run.stage_artifact_file(&span, artifact)?;
run.finish_span(span, status)?;
recorder.finish_run(run, finish)?;
```

The preferred shape is **explicit durable recording plus `tracing`
instrumentation**, not a pure tracing-subscriber reconstruction model. A typed
operation should be able to record without global telemetry setup:

```rust
let recorder = DriverRecorder::new(store, recorder_sink);
let mut run = recorder.start_run(DriverRunSpec::new("auv.driver.invoke"))?;
let span = run.start_span("screen.captureRegion", attrs)?;

let artifact_ref = run.stage_artifact_file(
  &span,
  ArtifactFile::new("region-capture", screenshot_path)
    .preferred_name("capture.png")
    .summary("Captured region image"),
)?;

run.event(&span, "artifact.captured", attrs_for(&artifact_ref));
run.finish_span(span, SpanStatus::ok("capture completed"))?;
recorder.finish_run(run, RunStatus::ok("driver operation completed"))?;
```

The same operation may also emit normal Rust tracing instrumentation:

```rust
let span = tracing::info_span!(
  "auv.driver.operation",
  auv.driver_id = "macos.desktop",
  auv.operation = "capture_region",
  auv.run_id = tracing::field::Empty,
  auv.span_id = tracing::field::Empty,
);
```

After the durable `DriverRun` and child span exist, the recorder can record the
actual `run_id` and `span_id` fields on the tracing span. If no subscriber is
installed, this is a no-op for telemetry but the AUV run still persists.

Do not implement the inverse model in the first PR:

```text
tracing events -> subscriber/layer -> reconstruct AUV run/artifact store
```

That model is useful later for a live observer or external telemetry bridge,
but it is too implicit for AUV's source-of-truth run store and makes artifact
staging failures hard to surface to callers.

The API should be shaped so future consumers can use it, but PR2 should wire
only the recorder extraction path and the selected proof for this slice. The
selected proof is the existing `recorded_operation.rs` direct recorded-operation
path. Existing candidate-action, detector, and AX recognition callers may be
updated only as needed to keep that path compiling and recording. Do not wire
app probe/analyze/distill/validate flows or migrate command families in this
PR.

Future consumers include:

- `auv-cli-invoke`
- candidate action recording
- detector/AX recognition recording
- app probe/analyze/distill/validate flows
- tests that need inspectable artifacts

## Relationship To auv-tracing-interaction

`auv-tracing-driver` records atomic driver operations.

`auv-tracing-interaction` records macro interactions that compose multiple
driver operations, such as scroll scan. Interaction recording may create
higher-level spans and artifacts, but it should call into driver-level
recording for atomic observations and input actions.

## Non-Goals

- Do not rewrite all command implementations in this PR.
- Do not reintroduce JSON recipes or bundles. Both lanes have been retired and
  must not be restored as compatibility.
- Do not change persisted run record wire shapes unless a compatibility
  boundary is documented and tested.
- Do not make the inspect server an execution dependency.
- Do not redesign viewer APIs, inspect-server protocol, replay, or mutation
  semantics; reuse existing inspect write behavior where needed.
- Do not migrate, polish, or expand archived AX copilot behavior. This boundary
  may become usable by candidate-action code, but that is not approval to grow
  the archived vertical.

## Proposed Steps

1. Move run lifecycle and artifact staging APIs from `Runtime` into the new
   tracing-driver boundary.
2. Change `recorded_operation.rs` so its context depends on the tracing
   recorder, not on `Runtime`.
3. Move read-only inspect helpers toward store/read modules rather than
   keeping them on `Runtime`.
4. Update only the existing direct recorded operation path needed to prove the
   new recording context.
5. Preserve existing CLI invoke behavior through the compatibility path. Wire
   `auv-cli-invoke` to the new recorder only to keep current recording behavior
   working, and add at most one minimal typed proof handler with no
   command-family migration.
6. Shrink `Runtime` to a temporary facade only for remaining invoke/runtime
   compatibility paths that have not yet moved behind direct typed APIs.

## Exit Criteria

- Typed Rust code can create a recorded run and stage artifacts without
  constructing `Runtime`.
- `recorded_operation.rs` no longer imports `crate::runtime::Runtime`.
- `Runtime` no longer owns core recording semantics.
- Existing inspect/read behavior still loads persisted runs.
- Remaining `Runtime` methods are limited to temporary compatibility paths and
  carry clear TODO markers.

## Verification

Required checks for the PR:

```text
cargo fmt --check
cargo check
cargo test
git diff --check
```

Focused tests should cover:

- successful recorded operation with artifacts
- failed recorded operation still persists a failed run
- artifact refs include run id, span id, artifact id, and capture event id
- inspect/read helpers can load runs produced by the new recorder

## Deferrals

TODO(runtime-delete): full `runtime.rs` deletion is deferred until remaining
invoke/runtime facade responsibilities move behind direct typed APIs and
`auv-tracing-driver`.

NOTICE(runtime-facade): `Runtime` still exposes recording facade methods for
remaining invoke and historical callers. New typed workflows should use
`auv_tracing_driver::RecordingHandle` directly.

TODO(tracing-interaction): scroll scan and other macro-operation recording are
deferred to `auv-tracing-interaction` after driver-level recording is stable.

TODO(command-migration): broad invoke command rewrites are deferred until the
recording boundary is stable and the owner approves a typed migration slice.
