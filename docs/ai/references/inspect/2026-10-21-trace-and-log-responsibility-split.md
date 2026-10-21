# Trace and operational log responsibility split

Date: 2026-10-21. Status: accepted, implemented on the local branch
`feat/trace-log-split` (not yet reviewed or merged). See
[Implementation](#implementation) for what landed and
[Acceptance](#acceptance) for the evidence.

This note decides which AUV layers record run data, which layers write
operational logs, and which layers do neither. It extends the producer-side
contract in
[`2026-07-26-tracing-write-pipeline.md`](2026-07-26-tracing-write-pipeline.md).
It does not change the `TraceRecord` schema or the `TracingStore` port.

## Motivation

The OSWorld ATIF handoff (vieval side, 2026-10-21) needs replayable episodes:
an ordered step timeline, the action of each step, and the captures around it.
The current store cannot give that:

- No production code opens command spans. Only `auv-media-macos` calls
  `start_span` (`crates/auv-media-macos/src/lib.rs:70-78`). `auv invoke` and
  `auv mcp` create a root `Context` without a span
  (`crates/auv-cli/src/commands/invoke.rs:98`,
  `crates/auv-cli/src/commands/mcp.rs:446`).
- `TraceRecord::Artifact` has no timestamp (`crates/auv-tracing/src/record.rs:37-42`).
- `InputActionResult` holds delivery evidence only, not the requested action
  (`crates/auv-driver-common/src/input.rs:617-628`).
- Only `auv invoke` and `auv mcp` configure a `FileTracingStore`. The daemon,
  `auv-api-server`, the Runner, and `@auv-js/sdk` record nothing.
- The daemon and `auv-api-server` write no logs. The only subscriber is
  `crates/auv-cli/src/cli.rs:179-190`, which is off unless `-v` is given and
  writes to stderr. The Runner child inherits the daemon's stderr
  (`crates/auv-daemon/src/daemon/runner.rs:530`), so Runner failures are not
  kept anywhere.
- No protocol message carries a span id or request id. `Context::inject` and
  `Context::from_remote` (`crates/auv-tracing/src/context.rs:98-107`) have no
  production callers. The `auv-run-id` metadata key is also the daemon's
  routing header, and the Runner proxy strips it before forwarding
  (`crates/auv-api-server/src/server/runner_grpc_proxy.rs`, `ROUTE_RUN_METADATA`).

## Two kinds of records

| | Trace (run recording) | Operational log |
|---|---|---|
| Purpose | replay, inspection, export (ATIF), eval evidence | diagnose why a daemon or Runner failed or was slow |
| Owner | `auv-tracing`: `records.jsonl` and `artifacts/`, keyed by Run | tokio `tracing` macros plus a file subscriber, keyed by process |
| Reader | inspector, exporters, vieval | developers, bug reports |
| Stability | versioned schema, public contract | no format promise |

One does not replace the other. A trace record is never written only to make a
failure debuggable, and a log line is never the only place a replay fact lives.

## Execution paths today

```text
auv invoke / auv mcp, no Device    -> command + driver in the CLI process        -> traced
auv invoke / auv mcp, Device/Run   -> command in the CLI, driver calls over gRPC -> traced, but
                                      capture pixels are pulled back to the CLI
                                      (TODO(runner-side-capture-artifacts))
@auv-js/sdk / playground           -> daemon proxy -> Runner driver RPCs,
                                      no command layer                           -> not traced
```

## Layer split

### 1. Command layer: `auv-cli-invoke` (shared by `auv invoke` and `auv mcp`)

The main trace producer. Both frontends get it from one place.

- One span per command run: `command_id`, inputs, status or failure layer,
  start and end time.
- The command's protocol inputs at start and its direct result or failure at
  the end. For input commands the direct result already carries the resolved
  action (`screen_point`, `screen_start`/`screen_end`, the
  `InputActionResult`), so no separate input-action schema was added; the
  ATIF projection normalizes the action kind. Only this layer knows the
  request; the driver does not need to know that a call is a replayable step.
- Artifacts it already emits (captures, OCR sources, scan coverage), plus a
  recorded time.
- Frontend lifecycle events (already present).

### 2. Runner RPC boundary: `crates/auv-cli/src/runner/local_driver.rs` service handlers

Not the driver layer: this is the process boundary, and the only place where
the SDK path can be traced.

- One span per driver RPC: service and method, a bounded request summary,
  result status, duration.
- Evidence artifacts persisted on the Runner side from `CaptureRef`, following
  the Image Payloads rule in `AGENTS.md`. This also resolves
  `TODO(runner-side-capture-artifacts)` for the command layer.
- The parent span is read from request metadata, so Runner spans nest under the
  caller's span (decision 2).

### 3. `@auv-js/sdk`

Records only what is unique to it, and never writes a store or moves pixels.

- Needs no code change to be recorded: the daemon gives every call routed to a
  Run that Run's trace context. A call without a Run is not recorded.
- May later carry its own span in `auv-trace-span-id` and offer a step
  boundary (provisional name, for example `withStep(name, fn)`) so an agent
  harness can group several RPCs under one tool call.
  `TODO(sdk-trace-step)`: deferred; nothing records the step span itself yet,
  so its name and time would be unknown to a reader.
- Model messages and token usage stay with the eval harness and are merged at
  export time by `run_id` / `span_id`.

### 4. Daemon (`auv-daemon`, `auv-api-server`): mostly logs

- Trace: Run start and end only, as the outer frame of a trace Run.
- Logs: Runner spawn, exit code, crash and restart; device discovery and
  pairing; Run binding and routing failures; authentication failures without
  credentials (keep the dependency-level filter from `cli.rs:186-188`);
  REST and WebSocket tunnel lifecycle.

### 5. Runner process: logs

- Driver call failures, missing permissions, selected platform backend,
  fallback reasons (for example EIS versus XTest), capture backend.
- Written to its own log file, tagged with the Runner id.

### 6. Drivers (`auv-driver*`): no `auv-tracing` dependency

- Everything a caller needs to record goes into the typed result
  (`InputActionResult.attempts` and fallback reasons already work this way).
- Internal detail (AX traversal, retries, settle waits) uses tokio `tracing`
  at `debug` / `trace` level only, and produces no run records.

## Decisions

1. **SDK-path traces are recorded on the Runner side.**
2. **Command spans and Runner RPC spans nest**, through trace-context
   propagation over gRPC metadata under keys separate from the `auv-run-id`
   routing header.
3. **Operational logs go to a per-user directory and expire automatically.**
   The owner accepted `~/.auv/` or the XDG cache directory; the existing AUV
   user state root was used instead (see below), because XDG puts logs under
   `XDG_STATE_HOME` and a cache directory may be cleared at any time.

## Implementation

### Record vocabulary

```text
auv.invoke.command span      command_id, execution (local | runner), dry_run
  auv.invoke.command_started   target, protocol inputs (values over 4 KiB cut to a prefix + length)
  auv.invoke.command_finished  status, direct result (<= 32 KiB) or failure
auv.runner.rpc span          rpc.service, rpc.method; remote parent = caller's span
  auv.runner.rpc_request       first request message, protobuf JSON
  auv.runner.rpc_finished      grpc_status, grpc_message, completed, first response message
artifact                     occurred_at (new); captures add capture.bounds.*,
                             capture.origin.*, capture.backend, capture.ref_id (Runner)
```

A command span without `command_finished` was dropped before it finished
(MCP cancellation). Runner messages are decoded generically from the
descriptor set, so new RPCs are recorded without handler code; messages over
64 KiB (pixels) are recorded by byte length, and strings over 2048 characters
by length.

### Producers and contracts

- `auv-tracing`: `TraceRecord::Artifact.occurred_at`; `records.jsonl`
  envelope version 2 (version 1 lines are rejected per line,
  `TODO(trace-envelope-v1-read)`); `FileTracingStore::read_records` and
  `artifact_path_in`; propagation keys `auv-trace-version`,
  `auv-trace-run-id`, `auv-trace-span-id`; `RemoteContext::new` / `inject`;
  `RunId::from_daemon_run_id`.
- `auv-cli-invoke` `trace`: the command span wraps `InvokeCommand::invoke` and
  `runner::invoke`, so CLI, MCP, and library callers share it.
- `auv-api-client`: the routed-call interceptor injects the caller's context.
- `auv-api-server` `server::trace_context`: the daemon proxy and the WebSocket
  tunnel apply the routed-Run policy (inject, accept the same Run, reject
  another Run or malformed fields, strip without a Run).
- `auv-cli` `runner::recording`: Runner store at
  `<daemon store root>/runner-traces/<runner id>/` (one store per Runner
  process, so concurrent Runners never share a file), flushed after every call
  and at shutdown; capture evidence is encoded on the blocking pool.
- `auv-trace-atif` (standalone tool crate, `publish = false`, not part of the
  `auv` CLI, because ATIF belongs to its consumers): reads one or more stores
  and their `runner-traces/*`, merges any number of Runs, and writes ATIF v1.8
  with copied images and `tool_calls[].extra.pointer` (and `start` for drags)
  in screenshot pixels. It depends only on `auv-tracing` and reads the record
  names as a contract (`NOTICE(atif-record-names)`).

### Operational logs

- Directory: `ProjectDirs("ai", "moeru", "auv")` state dir (else data-local
  dir) + `logs/`, mode 0700; `AUV_LOG_DIR` overrides.
- Files: `daemon.<date>.log`, `runner-<runner id>.<date>.log`, written
  synchronously; filter `off,auv=info`, replaced by `AUV_LOG`.
- Expiry: the daemon removes `*.log` files not modified for
  `AUV_LOG_RETENTION_DAYS` (default 7) at start and every 6 hours.
- Daemon logs Run create/stop, Runner start/stop/idle stop/unexpected exit
  (with exit status), discovery publication, pairing, bearer rejection, and
  refused routed calls. The Runner logs start/stop and every failed RPC.

## Acceptance

Run on macOS (2026-10-21) against an isolated `auv serve --no-register`, with
background input only. Harbor's validator
(`python -m harbor.utils.trajectory_validator`, image check on) accepted every
exported trajectory.

- Local CLI path: six commands, including failures, exported as six steps.
- Daemon Run: four CLI commands routed to the Runner. Every Runner RPC span
  nested under its command span and fell inside its time range.
- SDK-shaped calls (grpcurl with the SDK's routing headers): recorded as
  top-level Runner steps. Calls without a Run were not recorded. A trace context
  naming another Run, or a malformed one, was refused with `InvalidArgument`
  and logged.
- `kill -9` of the Runner: the call before the kill was already on disk. The
  daemon logged the exit (signal 9) and started a new Runner with its own
  store, and the export merged both stores.
- Daemon SIGINT: the Runner logged a clean stop. On restart, a 10-day-old `.log`
  was expired and a non-log file was kept.
- 20 concurrent `auv invoke` processes on one store: 100 lines, none corrupt.
- Background A→B on a TextEdit window: the two clicks mapped to the
  expected pixels of the window capture, which was checked visually. The typed
  text landed at the start of the document, not at click B. The clicks were
  delivered but not consumed (`verified: false`), and the after-capture shows it.

Linux (neko-gpu-1, GNOME 48 Wayland with portal input, 2560×1440 at 1×):

- A real-pointer trajectory in one daemon Run: capture, move to A
  (400,600), move to B (1600,900), drag A→B, click B, type, capture. All steps
  were delivered. The export (13 steps) passed Harbor's validator. Pointer A,
  pointer B, and the drag start mapped to the expected screenshot pixels,
  checked visually. The after-capture shows the typed text at the end of the
  document, where the click at B put the caret.
- `input.pointerPosition` is `Unimplemented` on the Wayland portal backend. It is
  recorded as a failed step and logged as a Runner warning, so a move cannot
  be verified by reading the pointer back there.

## Findings fixed on the way

- **Records could not be read back.** `TraceRecord` is internally tagged, and
  `JsonPayload` and `AttributeValue` deserialized through `RawValue`, which a
  buffered tagged enum cannot provide. Every event, and every span or artifact
  with attributes, failed to load (`NOTICE(json-payload-tagged-read)`).
- **Concurrent writers corrupted lines.** A record and its newline were
  separate writes, so a line from another process sharing the store could land
  between them. This reproduced in 2–14 of 400 lines
  (`NOTICE(trace-store-shared-append)`).
- **The trace and routing keys shared a name.** Both used `auv-run-id`, so trace
  context could never reach a Runner.
- **Capture artifacts lacked screen geometry**, so a pointer could not be placed
  on a screenshot.

## Known gaps and open questions

- `TODO(runner-side-capture-artifacts)`: on the remote CLI path, window,
  display, and region captures are recorded twice, once in each store. The
  export prefers the command's own copy.
- `TODO(runner-capture-evidence-coverage)`: captures inside text search,
  scroll-until, and recent frames are not recorded by the Runner.
- `TODO(atif-pointer-streams)`: `MoveMouse` and `StreamMouseMotion` record
  only their first response, so the export has no end point for them.
- `TODO(input-text-redaction)`: typed and pasted text is recorded verbatim, up
  to 4 KiB.
- Without an explicit Run, each `auv invoke` or MCP call is its own Run. Pass
  every Run to `auv-trace-atif`, or use `auv run create` with `--run`.
- `encode_args` drops non-finite numbers, so a rejected `nan` input is missing
  from the recorded inputs. This was not changed in this slice.
- A remote Device's Runner store is on that Device. The export reads only
  local stores.
- Cross-process ordering uses each host's wall clock.
- Tests that start `auv serve` write daemon logs to the real user log
  directory unless they set `AUV_LOG_DIR`.
- Whether trace stores should also expire, and whether the daemon should copy
  a guest store back into an OSWorld episode, are still open.
- `TODO(sdk-trace-step)`: there is no SDK step boundary yet.
- A real-pointer A→B run on Windows has not been done yet.
