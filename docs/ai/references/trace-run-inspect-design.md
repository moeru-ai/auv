# Trace Run Inspect Design

Date: 2026-05-19

Status: approved for implementation planning

## Purpose

AUV needs one inspectable run model for command execution, recipe execution,
app probing, distillation, validation, and future UI inspection. The current
runtime persists one run per command invocation, while higher-level workflows
keep their own separate JSON structures. That makes it difficult to inspect a
complete workflow, stream live progress, or convert recorded data to OTLP later.

This design introduces a versioned `v1alpha1` trace/run format, a canonical run
store layout, and a read-only inspect server contract for historical viewing and
live streaming.

Terminology follows `docs/TERMS_AND_CONCEPTS.md`.

## Scope

In scope:

- A unified trace/run/span/event/artifact model.
- A canonical `.auv/runs/{run_id}/` storage layout.
- `v1alpha1` API version markers on all canonical records.
- Read-only HTTP APIs for viewer data access.
- WebSocket live stream APIs for in-progress runs.
- Mapping existing command, recipe, probe, analyze, distill, and validate flows
  into the new model.
- Removing old split text files from new run writes.

Out of scope:

- Replay APIs or replay UI.
- Mutation APIs for editing, deleting, annotating, or replaying runs.
- Legacy run conversion or viewer fallback for old run directories.
- A native Rust GUI.
- Direct OTLP export implementation. The format should be OTLP-friendly, but
  export can be implemented later.

## Data Model

### Run

A run is the top-level user-visible record and the storage unit under
`.auv/runs/{run_id}/`.

`run.json` uses `api_version: "auv.run.v1alpha1"` and contains:

- `run_id`: user-facing run handle and directory name.
- `trace_id`: OpenTelemetry-compatible trace identifier.
- `run_type`: `command`, `execute`, `probe`, `analyze`, `distill`, or
  `validate`.
- `state`: `running` or `ended`.
- `status_code`: `unset`, `ok`, or `error`, matching the OpenTelemetry span
  status vocabulary.
- `started_at` and optional `finished_at`.
- `root_span_id`.
- `attributes`: structured metadata such as recipe id, target application id,
  source analysis path, or source distillation path.
- `summary`: optional human-readable summary.
- `failure`: optional structured failure information.

`run_id` remains optimized for humans and local paths. `trace_id` is optimized
for telemetry compatibility. Implementation should keep one top-level run
record type that includes telemetry identifiers; it should not introduce a
second top-level trace record with duplicate fields.

### Span

A span is a timed unit of work inside a run. Spans form a tree through
`parent_span_id`.

Each line in `spans.jsonl` uses `api_version: "auv.span.v1alpha1"` and contains:

- `span_id`.
- `parent_span_id`, omitted for the root span.
- `name`, such as `auv.execute`, `auv.recipe.step`, or `auv.command.invoke`.
- `state`: `running` or `ended`.
- `status_code`: `unset`, `ok`, or `error`, matching the OpenTelemetry span
  status vocabulary.
- `started_at` and optional `finished_at`.
- `attributes`.
- Optional `summary` and `failure`.

Expected span levels include workflow phases, case-matrix cases, recipe steps,
command invocations, and driver actions.

OpenTelemetry stores events inside spans. AUV stores events in `events.jsonl`
for append-friendly local writes and live streaming. Exporters should group
events back under their parent spans by `span_id` when converting to OTLP.

### Event

An event is a timestamped occurrence attached to a span. Events are small and
append-friendly.

Each line in `events.jsonl` uses `api_version: "auv.event.v1alpha1"` and
contains:

- `event_id`.
- `span_id`.
- `name`, such as `command.resolved`, `driver.invoke`, `action.started`,
  `artifact.captured`, `assertion.passed`, or `assertion.failed`.
- `timestamp`.
- `attributes`.
- Optional `message`.
- Optional references to `artifact_ids`.

Events should not embed large payloads.

### Artifact

An artifact is persisted inspection material produced during a run.

Each line in `artifacts.jsonl` uses `api_version: "auv.artifact.v1alpha1"` and
contains:

- `artifact_id`.
- `span_id`.
- Optional `event_id`.
- `role`, such as `screenshot.before`, `screenshot.after`, `ax.before`,
  `ax.after`, `click.overlay`, `driver.output`, `distillation.report`, or
  `validation.report`.
- `mime_type`.
- `path`, relative to the run directory.
- Optional `sha256`.
- `attributes`.
- Optional `summary`.

Artifacts hold metadata only. Large payloads remain as files under
`artifacts/`.

## Store Layout

New runs use this layout:

```text
.auv/runs/{run_id}/
  run.json
  spans.jsonl
  events.jsonl
  artifacts.jsonl

  artifacts/
    artifact_0001_screenshot.png
    artifact_0002_ax.json
    artifact_0003_click_overlay.png
    artifact_0004_driver_output.json
```

The run store only persists canonical machine-readable data and artifact files.
It does not write derived text files such as `inspect.txt`, `meta.txt`,
`inputs.txt`, `events.log`, `artifacts.txt`, or `output.txt`.

Human-readable output is produced by formatters at read time. For example,
`auv inspect <run_id>` can render text, JSON, or Markdown from the canonical
files without creating another stored source of truth.

## Versioning And Read Rules

All canonical records carry an `api_version` field.

Supported first-version values are:

- `auv.run.v1alpha1`
- `auv.span.v1alpha1`
- `auv.event.v1alpha1`
- `auv.artifact.v1alpha1`

Read rules:

- A run directory must contain `run.json`.
- `run.json.api_version` must equal `auv.run.v1alpha1`.
- JSONL records must use their matching `v1alpha1` API version.
- A run with missing or unsupported canonical files is considered invalid for
  the inspect server.
- Direct requests for invalid run directories should return a structured
  `unsupported_run_format` or `invalid_run_format` error.
- Old run directories without `run.json` are ignored by `GET /runs`.
- No legacy fallback or automatic migration is part of this design.

If migration becomes necessary later, it should be an explicit migration tool,
not implicit viewer behavior.

## Runtime Recording Semantics

The runtime should record runs through an explicit recording context.

`Runtime::invoke` should support two modes:

- Without a recording context, it creates an ad-hoc `command` run.
- With a recording context, it records the command invocation as child spans in
  the current run.

This allows CLI, library calls, skill execution, app probing, distillation, and
validation to share the same execution model while preserving their different
workflow shapes.

Writers should append events as they happen, register artifacts when persisted,
and update spans/run status when work completes or fails. The inspect server's
live stream can reuse the same record shapes.

For `v1alpha1`, `events.jsonl` is append-only. `spans.jsonl`, `artifacts.jsonl`,
and `run.json` represent current state and should be rewritten atomically when
their records change. This avoids turning the run store into an event-sourced
database while still keeping events append-friendly.

## Workflow Mapping

### Ad-hoc Command

```text
Run type: command
Span tree:
  auv.command
    auv.command.invoke
      auv.driver.invoke
```

This covers direct runtime invocations.

### Recipe Execute

```text
Run type: execute
Span tree:
  auv.execute
    auv.recipe.step step_id=open
      auv.command.invoke
        auv.driver.invoke
    auv.recipe.step step_id=type
      auv.command.invoke
        auv.driver.invoke
```

A recipe execution should be one run. Individual recipe steps should not become
unrelated top-level runs.

### Case Matrix Validate

```text
Run type: validate
Span tree:
  auv.validate
    auv.case case_id=...
      auv.execute
        auv.recipe.step ...
```

Validation should retain the full run structure for every selected case instead
of only retaining success or failure summaries.

### App Probe

```text
Run type: probe
Span tree:
  auv.probe
    auv.probe.step id=permissions
      auv.command.invoke
    auv.probe.step id=window-state
      auv.command.invoke
```

Probe-specific step lists should be replaced by span references in the shared
run model.

### App Analyze

```text
Run type: analyze
Span tree:
  auv.analyze
    auv.analysis.input
    auv.analysis.output
```

The analysis result remains a domain document, persisted as artifacts and linked
from spans or events.

### App Distill

```text
Run type: distill
Span tree:
  auv.distill
    auv.distill.candidate recipe_id=...
    auv.distill.output
```

Distillation keeps its domain payload: candidate recipes, case matrices,
assessment status, rationale, and known boundaries. It shares the run/span/event
model but does not reuse execute action payloads.

### App Validate

```text
Run type: validate
Span tree:
  auv.validate
    auv.distilled_candidate recipe_id=...
      auv.case case_id=...
        auv.execute
```

Validation records candidate-level and case-level spans, then nests recipe
execution spans underneath.

## Inspect Server Contract

The inspect server is read-only. It serves stored run data and streams live run
updates. It does not execute commands, replay runs, mutate records, or delete
data.

HTTP endpoints:

```text
GET /runs
GET /runs/{run_id}
GET /runs/{run_id}/spans
GET /runs/{run_id}/events
GET /runs/{run_id}/artifacts
GET /runs/{run_id}/artifacts/{artifact_id}
```

WebSocket endpoint:

```text
WS /runs/{run_id}/stream
```

Endpoint behavior:

- `GET /runs` lists valid `v1alpha1` runs.
- `GET /runs/{run_id}` returns `run.json`.
- `GET /runs/{run_id}/spans` returns all span records.
- `GET /runs/{run_id}/events` returns all event records.
- `GET /runs/{run_id}/artifacts` returns artifact metadata.
- `GET /runs/{run_id}/artifacts/{artifact_id}` returns the artifact file or
  redirects to a local static-file route.
- `WS /runs/{run_id}/stream` streams incremental updates for a running run.

Viewer load flow:

```text
1. GET /runs/{run_id}
2. In parallel, GET spans, events, and artifacts.
3. Render the current timeline and artifact panels.
4. If the run is still running, connect to WS /runs/{run_id}/stream.
5. Append span, event, artifact, and run updates as they arrive.
```

WebSocket messages should reuse the canonical shapes:

```json
{ "type": "span.started", "span": {} }
{ "type": "event.appended", "event": {} }
{ "type": "artifact.created", "artifact": {} }
{ "type": "span.finished", "span": {} }
{ "type": "run.finished", "run": {} }
```

The exact message wrapper can evolve during implementation, but the embedded
records should remain the same records persisted to disk.

## Suggested Rust Components

Suggested implementation components:

- `trace` or `recording` module for run/span/event/artifact types.
- `RunWriter` for canonical writes.
- `RunReader` for validated reads from `.auv/runs/{run_id}`.
- Runtime recording context passed through command, skill, probe, distill, and
  validate flows.
- Formatter layer for `auv inspect` output.
- Inspect server using `axum`, `tokio`, `tower-http`, and `serde_json`.
- Live stream fanout using `tokio::sync::broadcast`.

The first implementation does not need to build the browser viewer. It only
needs to make the data and server contract reliable enough for a viewer to
consume.

The first implementation should treat runtime recording and the inspect server
as same-process components for live streaming. Cross-process file watching is
out of scope for `v1alpha1`.

## Error Handling

Recording failures should not silently produce malformed runs.

Rules:

- If a run cannot write `run.json`, the command should fail early.
- If a span/event/artifact record cannot be written, the active span
  and run should be marked failed when possible.
- If artifact file persistence fails, record an `artifact.failed` event and fail
  the associated span unless the caller explicitly marks the artifact optional.
- Readers should validate `api_version` before parsing version-specific fields.
- Inspect server errors should be structured and stable enough for viewers to
  display useful messages.

## Testing

Focused tests should cover:

- Creating an ad-hoc command run with valid `v1alpha1` files.
- Executing a multi-step recipe as one run with child spans.
- Validating a case matrix as one run with case and execute child spans.
- Rejecting run directories without `run.json`.
- Rejecting unsupported `api_version` values.
- Serving historical run data through HTTP endpoints.
- Streaming live updates through the WebSocket endpoint.
- Rendering CLI inspect output from canonical files without stored text
  snapshots.

## Open Decisions

These decisions are intentionally left for implementation planning:

- Exact Rust module names.
- Exact `run_id`, `trace_id`, and `span_id` generation functions.
- Whether the inspect server is started by a CLI subcommand, embedded in future
  UI flows, or both.
- Whether artifact hashes are required on every artifact or best-effort for
  large files.
