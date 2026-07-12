# Live Inspect Recording Design

Date: 2026-05-21

Status: implemented for phase 1

## Purpose

AUV needs a recording design that supports three runtime shapes:

- CLI execution with no inspect server.
- CLI execution reporting to an opt-in local inspect server.
- Library usage where the caller supplies recording behavior.

The design must avoid treating `.auv` as the only storage location. Project
stores are useful for development, but release builds and user machines need
configurable and OS-appropriate storage.

This design keeps the existing run/span/event/artifact model and adds explicit
recording backends, HTTP-based local inspect server writes, stable viewer-facing
attributes, and clearer CLI options.

## Goals

- Introduce a runtime-side `RunRecordingBackend` abstraction.
- Support cross-process local reporting to an opt-in inspect server over HTTP.
- Keep inspect server write disabled by default.
- Use camelCase for external HTTP write payloads; the current viewer WebSocket
  stream keeps the existing snake_case event tags.
- Keep current on-disk canonical JSON shape unchanged in this phase.
- Add stable step/span attributes for web viewer grouping.
- Make local run storage configurable through a store root.

## Non-Goals

- Do not switch the frontend viewer to consume OTLP directly.
- Do not migrate existing on-disk run JSON to camelCase.
- Do not require an inspect server for normal CLI execution.
- Do not make `.auv` the release-mode default storage location.
- Do not implement remote non-localhost write exposure beyond guarded options.

## Core Architecture

Runtime recording is routed through `RunRecordingBackend`:

```text
Runtime
  -> RunRecordingBackend
       -> one or more RunStore targets
       -> one or more RunRecorder targets
```

`RunStore` owns canonical run snapshots and artifact files. The current
`LocalStore` remains the first concrete implementation. The backend must support
multi-write recording: a single run may be written to more than one store or
reporting target, such as a local store and an inspect server.

When multiple write targets are enabled, AUV does not define a universal single
source of truth. Each target owns the records it accepted. Callers and viewers
choose which target they read from by selecting a store root or inspect server.
For live inspection, users will usually prefer the inspect server because it is
the active viewer endpoint, but the core recording API must not assume that only
one canonical target exists.

`RunRecorder` receives incremental updates:

- `runStarted`
- `spanStarted`
- `eventAppended`
- `artifactCreated`
- `spanFinished`
- `runFinished`

Recorder implementations can include:

- `LocalRunRecorder`
- `BroadcastRunRecorder`
- `InspectServerRunRecorder`
- `CompositeRunRecorder`
- `NoopRunRecorder`

`CompositeRunRecorder` fans updates out to multiple recorders. For example:

```text
CLI without server:
  LocalRunRecorder backed by RunStore

CLI with discovered inspect server:
  LocalRunRecorder backed by RunStore + InspectServerRunRecorder

same-process inspect viewer:
  LocalRunRecorder backed by RunStore + BroadcastRunRecorder
```

The runtime should not decide CLI policy. CLI parsing builds a
`RunRecordingBackend` and passes it to the runtime.

`RunRecordingBackend` should not collapse local and server recording into one
exclusive destination. It should preserve the ability to write to local and
server targets at the same time, and it should surface per-target failures
according to CLI policy.

## Inspect Server Write API

`auv inspect serve` remains read-only by default.

To accept runtime writes:

```text
auv inspect serve --enable-write
```

The server exposes write endpoints:

```text
POST /write/runs/{runId}/updates
POST /write/runs/{runId}/artifacts/{artifactId}
```

`/write/runs/{runId}/updates` accepts a batch of run updates:

```json
{
  "updates": [
    {
      "type": "runStarted",
      "runId": "run_...",
      "run": {
        "apiVersion": "auv.run.v1alpha1",
        "runId": "run_...",
        "traceId": "...",
        "runType": "execute",
        "state": "running",
        "statusCode": "unset",
        "startedAtMillis": 123,
        "finishedAtMillis": null,
        "rootSpanId": "0000000000000001",
        "attributes": {},
        "summary": null,
        "failure": null
      }
    },
    {
      "type": "spanStarted",
      "runId": "run_...",
      "span": {
        "apiVersion": "auv.span.v1alpha1",
        "spanId": "0000000000000002",
        "parentSpanId": "0000000000000001",
        "name": "auv.recipe.step",
        "state": "running",
        "statusCode": "unset",
        "startedAtMillis": 124,
        "finishedAtMillis": null,
        "attributes": {
          "auv.step.id": "capture_after_search",
          "auv.step.index": 0,
          "auv.step.kind": "recipe"
        },
        "summary": null,
        "failure": null
      }
    }
  ]
}
```

The WebSocket stream emits one update per message using the viewer-facing
snake_case event tags that the current inspect viewer consumes, such as
`span_started`, `event_appended`, `artifact_created`, `span_finished`, and
`run_finished`.

The server converts public camelCase HTTP DTOs into internal Rust/store records.
The on-disk canonical run files can stay with their current snake_case fields
until a separate format/version migration is planned.

When the inspect server accepts an update batch, it updates its configured
`RunStore` snapshot and publishes the same update records to connected
WebSocket viewers.

The inspect server write path is therefore:

```text
HTTP write request
  -> validate token and payload
  -> apply updates to the server configured RunStore
  -> publish accepted updates to the server WebSocket broadcast bus
```

The server is not just a relay. It stores accepted records in its own configured
store root so HTTP read APIs and later viewers can load the run from the server
even after the writer process exits.

If an update conflicts with already accepted records for the same run id, the
server should reject the conflicting update instead of overwriting existing
state. Conflicts include incompatible run metadata for an existing run id,
duplicate span/event/artifact ids with different content, invalid parent span
references, or terminal updates that try to mutate an already finished run.

Conflict responses should be structured enough for clients to recover or give a
useful error:

```json
{
  "error": {
    "code": "runConflict",
    "message": "run run_... already has different metadata",
    "runId": "run_...",
    "conflictKind": "runMetadataMismatch",
    "resolution": "startNewRun",
    "retryable": false
  }
}
```

Initial conflict kinds:

- `runMetadataMismatch`
- `duplicateSpanMismatch`
- `duplicateEventMismatch`
- `duplicateArtifactMismatch`
- `missingParentSpan`
- `runAlreadyFinished`

Initial resolutions:

- `startNewRun`: client should create a new run id and retry from a fresh run.
- `reloadServerRun`: client should stop writing and inspect the server copy.
- `failRun`: client should surface the conflict and fail the current run if the
  CLI policy requires server write success.

For the first implementation, conflicts are not retryable against the same run
id. If `--require-inspect-server-write` is not set, the CLI may continue local
recording after reporting a warning. If it is set, the CLI should fail the run
with the structured conflict message.

## Inspect Server Write Security

Server options:

```text
auv inspect serve --enable-write
auv inspect serve --write-token <token>
auv inspect serve --write-token-file <path>
auv inspect serve --no-write-token
```

Rules:

- `inspect serve` is read-only by default.
- `--enable-write` enables runtime writes.
- `--write-token` and `--write-token-file` imply write is enabled.
- `--no-write-token` does not imply write is enabled; it must be combined with
  `--enable-write`.
- `--no-write-token` is only allowed when the server binds to loopback.
- Non-loopback host plus write enabled always requires a token.
- If write is enabled without an explicit token, the server generates a
  temporary session token.
- Runtime writers send the token with `Authorization: Bearer <token>`.
- `--write-token` and `--write-token-file` are mutually exclusive.
- `--no-write-token` cannot be combined with `--write-token` or
  `--write-token-file`.

## Store Root

Local run data lives under a resolved store root:

```text
{storeRoot}/runs/{runId}/...
```

Use a shared option:

```text
--store-root <path>
```

It applies to `inspect serve` and ordinary run commands such as `skill run` and
`invoke`.

Resolution policy:

- Explicit `--store-root` wins.
- Project CLI builds default to `$PWD/.auv`.
- Release/user builds default to an OS user cache/data directory.
  - macOS: `~/Library/Caches/AUV`
  - Linux: `$XDG_CACHE_HOME/auv` or `~/.cache/auv`

The initial implementation should preserve the current project CLI behavior by
defaulting to `$PWD/.auv`. Release/user packaging can switch the default through
the same store-root resolver without changing runtime recording APIs.

## Inspect Server Session Discovery

The server session descriptor is separate from the store root. It is used only
for client discovery.

When write is enabled, `auv inspect serve` writes a session descriptor in an OS
user runtime/cache location. The descriptor path may be overridden for tests or
advanced local workflows with `AUV_INSPECT_SESSION`, but default discovery must
not trust arbitrary world-writable locations.

```json
{
  "url": "http://127.0.0.1:8765",
  "storeRoot": "/resolved/store/root",
  "writeEnabled": true,
  "writeToken": "...",
  "pid": 12345,
  "startedAtMillis": 123456789
}
```

Ordinary CLI execution uses this descriptor when
`--inspect-server-write default` is in effect.

Explicit `--inspect-server-url`, `--inspect-server-token`, or
`--inspect-server-token-file` overrides session discovery.

Discovery is intentionally conservative:

- The default session path is user-private.
- On Unix, descriptors must be owned by the current user and must not grant
  group or other access.
- Discovered session URLs must be local or loopback before ordinary CLI runs
  report trace data.
- Malformed, stale, unsafe, or non-local descriptors are ignored with a warning
  for best-effort/default reporting.
- The same descriptor problems fail command setup when inspect server write is
  required.

## Ordinary Run CLI Settings

Ordinary run commands keep the inspect prefix:

```text
--inspect-local-write true|false|default
--inspect-server-write true|false|default
--require-inspect-server-write
--inspect-server-url <url>
--inspect-server-token <token>
--inspect-server-token-file <path>
--store-root <path>
```

Semantics:

- `--inspect-local-write default`: use the configured default local store.
- `--inspect-local-write true`: force local store writes.
- `--inspect-local-write false`: do not write local run data.
- `--inspect-server-write default`: use discovered local inspect server if one
  is present; failures are warnings.
- `--inspect-server-write true`: explicitly attempt server reporting; failures
  are warnings unless `--require-inspect-server-write` is also set.
- `--inspect-server-write false`: do not discover or report to an inspect
  server.
- `--require-inspect-server-write`: server write failure fails the run.

When local and server writes are both enabled, the run is recorded to both
targets. This is multi-write behavior, not primary/replica behavior. The CLI
does not declare one target to be authoritative over the other.

## Artifact Handling

Phase 1 keeps artifact handling conservative:

- Local write stages artifacts into the configured local store.
- Server write sends artifact metadata through `artifactCreated`.
- `POST /write/runs/{runId}/artifacts/{artifactId}` accepts artifact bytes only
  after the corresponding artifact metadata exists in the server store.
- Uploaded bytes are written to the relative artifact path declared by the
  accepted artifact metadata, with path traversal and symlink-target checks.

This means live viewers can show artifact metadata immediately and can preview
server-side artifact bytes once the runtime uploads them.

## Stable Viewer Attributes

Add stable keys while preserving existing keys:

```text
auv.step.id
auv.step.index
auv.step.kind = recipe | probe
auv.recipe.id
auv.case.id
auv.command.id
auv.driver.id
auv.driver.operation
auv.target.application_id
```

These keys let a web viewer group and label steps without depending on older
workflow-specific attribute names.

## Error Handling

Local write failures should fail the run when local write is enabled or default
policy selects local write.

Server write failures:

- Do not fail the run under `--inspect-server-write default`.
- Do not fail the run under `--inspect-server-write true` unless
  `--require-inspect-server-write` is set.
- Fail the run when `--require-inspect-server-write` is set.

In multi-write mode, one target can succeed while another fails. Successful
targets should keep their accepted records. Failed targets should report the
failure according to policy; they should not force rollback of successful
targets.

WebSocket broadcast lag should eventually produce a reload signal such as:

```json
{
  "type": "snapshotRequired",
  "runId": "run_..."
}
```

That can be a follow-up after the write API and recording backend are in place.

## Implementation Stages

### Stage 1: Recording Backend Boundary

- Introduce `RunRecordingBackend`.
- Introduce `RunRecorder` and run update types.
- Move current `Runtime` store and event sink usage behind the backend.
- Keep current behavior unchanged.
- Add stable step attributes.

### Stage 2: Store Root Options

- Add `--store-root` parsing.
- Route default runtime/store construction through resolved store root.
- Keep development default as `$PWD/.auv`.

### Stage 3: Inspect Server Write

- Add `inspect serve` write options.
- Add write token validation.
- Add `/write/runs/{runId}/updates`.
- Write session descriptor when write is enabled.
- Add `InspectServerRunRecorder` for CLI reporting.

### Stage 4: Artifact Upload

- Implement `/write/runs/{runId}/artifacts/{artifactId}`.
- Add recorder support for uploading artifact bytes.
- Teach viewer APIs to serve server-side uploaded artifacts.

The phase 1 implementation now includes the local inspect-server artifact upload
path. Broader remote write exposure remains guarded by the existing localhost
and token policy.

## Testing

Focused tests should cover:

- Existing local run persistence still writes canonical files.
- Runtime can use `RunRecordingBackend` without behavior change.
- Recipe step spans include stable `auv.step.*` attributes.
- `inspect serve` rejects write requests unless write is enabled.
- Write token is required unless explicitly disabled on loopback.
- Non-loopback write without token is rejected.
- CLI server write defaults degrade gracefully when no session is present.
- `--require-inspect-server-write` turns reporting failure into run failure.
