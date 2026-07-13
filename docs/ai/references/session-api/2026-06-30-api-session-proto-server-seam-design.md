# 2026-06-30 AUV API-P4: Session proto server seam design

Date: 2026-06-30

Status: **docs-only seam design** — defines the minimum owned boundary for a
future `SessionService` server implementation over API-P2/P3. This note does
**not** approve or implement transport/runtime/server code.

## One-line summary

API-P4 defines the smallest honest server seam for `auv.api.session.v1`:
separate from the inspect server, backed by existing invoke/runtime/store
surfaces, with explicit ownership for session registry, invoke execution,
two-source operation summary, and event projection. It exists to prevent the
implementation slice from smearing handler code across `mcp`, `inspect_server`,
and `runtime`.

## Why API-P4 exists

API-P2 made the proto surface real.

API-P3 then froze the uncomfortable truth:

- `GetOperationResponse` is a **two-source projection**
- `CreateSession` has no current internal entrypoint
- `StreamSessionEvents` has no current internal producer
- `Invoke` today cannot explicitly stamp a caller-specified `session_id`

That is exactly the point where a team usually starts writing handlers “just to
get something working,” then accidentally invents a third runtime surface in
the glue.

API-P4 exists to block that.

## Current constraints from the repo

### 1. `SessionRuntime` exists, but it is not a network service

`src/session.rs` is an **in-process stateful observation substrate**:

- provider reuse
- observation resources
- verification resources
- event list

It is explicitly **not** a daemon transport and **not** a general API server.

### 2. `Runtime` can open sessions, but invoke does not currently use them

`src/runtime.rs` exposes:

- `open_session(SessionOptions) -> SessionRuntime`
- run reading / recorded operation helpers

But `auv_cli_invoke::invoke_recorded` currently starts runs with:

```rust
RunSpec::new(RunType::Command, "auv.command")
```

and does **not** accept an explicit `SessionId`.

So today, the invoke lane and the session substrate are adjacent, not unified.

### 3. MCP is not the server seam we want

`src/mcp.rs` already exposes an `invoke` tool, but it:

- builds its own `RunRecordingBackend`
- calls `auv_cli_invoke::invoke_recorded(...)` directly
- returns ad-hoc JSON

That is useful evidence, but it is **not** a reusable `SessionService`
boundary. Re-using MCP handlers as the proto server would be lazy glue, not an
owned surface.

### 4. Inspect server is viewer/storage-facing, not execute-facing

`src/inspect_server/mod.rs` is explicitly:

- HTML/HTTP/WebSocket inspection access
- run/artifact read API
- optional remote write sink for run updates/artifacts

It does **not** execute commands. API-P4 must not collapse `SessionService`
into `inspect_server`.

### 5. Run-update transport already exists

`auv_tracing_driver` already owns:

- `RunUpdate`
- `WireUpdate`
- `BroadcastRunRecorder`

That means a future `StreamSessionEvents` implementation does **not** need to
invent a second low-level event bus. It should project from existing run-update
delivery plus explicit server/session lifecycle signals.

## Server seam decision

API-P4 recommends that a future implementation own a **dedicated session API
server boundary** with four responsibilities:

1. **session registry**
2. **invoke execution adapter**
3. **operation summary source**
4. **event projector**

Nothing else should be in scope for the first implementation slice.

## Recommended boundary shape

The future implementation should live in a dedicated module or crate-owned
subtree, for example:

```text
src/api/session_service/
  mod.rs
  registry.rs
  invoke.rs
  summary.rs
  events.rs
```

The exact path is not frozen by API-P4. The important part is ownership:

- not in `src/mcp.rs`
- not in `src/inspect_server/`
- not inline inside `src/runtime.rs`

## Owned responsibilities

### A. Session registry

The server seam must own a registry keyed by proto `session_id`.

Minimum required responsibilities:

- create / look up a session handle
- remember creation time and device scope
- reject unknown `session_id` on `Invoke` / `StreamSessionEvents`

API-P4 does **not** require eager `SessionRuntime` creation for every session.

Recommended rule:

- for the first invoke-only server slice, the registry may be **lightweight**
  and store session metadata only
- future RPCs that actually need observation/provider state may lazily
  materialize `SessionRuntime`

That keeps `CreateSession` honest without pretending today's invoke lane is
already session-resource-aware.

### B. Invoke execution adapter

The server seam must own the bridge from proto `InvokeRequest` to the current
invoke host surface.

That adapter is responsible for:

- validating `session_id`
- decoding `json_payload`
- mapping into host `auv_cli_invoke::InvokeRequest`
- executing through the recorded invoke path
- returning `InvokeResponse`

**Critical seam requirement:** before implementation, there must be a way to
run invoke with an explicit session id.

Today that does not exist in `auv_cli_invoke::invoke_recorded`.

So the implementation slice must **first** introduce a narrow session-aware
invoke entry, such as:

- `invoke_recorded_with_spec(...)`, or
- `invoke_recorded_with_session(...)`, or
- a tiny wrapper that accepts `RunSpec`

API-P4 does **not** approve the exact function name. It only freezes the rule:

> the proto server must not fake session support while all invokes still stamp
> `default`.

### C. Operation summary source

The server seam must own one boundary for serving:

- `InvokeResponse`
- `GetOperationResponse`

API-P3 already showed why:

- `InvokeResult` owns `output_summary`, `signals`, `failure_message`
- `OperationResult` owns `operation_id`, `known_limits`

So API-P4 requires a **named summary-source seam** instead of hidden ad-hoc
joins in handlers.

The seam may later be implemented either as:

- an in-memory summary cache keyed by `run_id`, or
- a persisted projection, or
- a composed read path over both

But the choice must be explicit and isolated behind one owned module.

### D. Event projector

The server seam must own projection from internal events to proto
`SessionEvent`.

For the first implementation, the recommended source is:

- `BroadcastRunRecorder` / `RunUpdate` for invoke/run/artifact lifecycle
- explicit server-local session lifecycle (`session_created`, etc.)

The recommended source is **not**:

- raw `SessionRuntime::events()` alone

Reason:

- `SessionRuntime` events are observation/action-resource-oriented
- proto `SessionEvent` is invoke/run/artifact-oriented

They overlap only partially. Treating one as the other would be a category
error.

## Request-flow design

### `CreateSession`

Minimum honest flow:

```text
RPC request
→ allocate or normalize session_id
→ register lightweight session entry
→ emit session-created server event
→ return SessionRef
```

Do **not** pretend this already provisions a daemon-backed automation context.

### `Invoke`

Minimum honest flow:

```text
RPC request
→ validate session exists
→ decode json_payload envelope
→ build host InvokeRequest
→ execute through session-aware recorded invoke seam
→ capture InvokeResult
→ record/update operation summary source
→ project artifact refs
→ return InvokeResponse
```

Two rules matter here:

1. The invoke path must stamp the chosen `session_id` onto the run.
2. The invoke path must update whatever summary source `GetOperation` will read,
   instead of hoping the persisted `OperationResult` already contains enough.

### `GetOperation`

Minimum honest flow:

```text
RPC request
→ resolve run_id / operation handle
→ read persisted OperationResult
→ read summary source for InvokeResult-only fields
→ join explicitly
→ return GetOperationResponse
```

If the implementation cannot satisfy both sources, it must fail or explicitly
return a partial response per owner decision. It must not silently fabricate
empty strings as if they were authoritative data.

### `StreamSessionEvents`

Minimum honest flow:

```text
RPC request
→ validate session exists
→ subscribe to event projector
→ filter events by session_id
→ stream proto SessionEvent
```

The stream should stay coarse:

- session created
- invoke started
- invoke completed
- invoke failed
- artifact recorded

No controller loops. No observation flood. No semantic verdict multiplexing.

## First implementation boundaries

API-P4 recommends the first implementation slice stay inside this box:

| In scope | Out of scope |
| --- | --- |
| unary `CreateSession` | daemon/session persistence across process restarts |
| unary `Invoke` | planner/controller |
| unary `GetOperation` | action lease |
| streamed `StreamSessionEvents` | donor-specific proto expansion |
| local-process registry | multi-device routing |
| one local store / recorder path | inspect-server merger |

## Relationship to existing surfaces

### `runtime`

`runtime` remains the execution/storage owner. The session API server may call
into it or its adjacent invoke/run-recording seams, but must not absorb
runtime responsibilities into the transport layer.

### `inspect_server`

`inspect_server` remains:

- viewer-facing
- storage-facing
- write-sink-facing

The session API server may share:

- `LocalStore`
- `BroadcastRunRecorder`

But it should not share route ownership or get buried inside the inspect
viewer server.

### `mcp`

`mcp` remains a tool surface. It can later choose to call through the same
session API service boundary, but API-P4 does **not** require MCP and proto
server unification in the first implementation slice.

## Pre-implementation gates

Before code is written, the implementation slice should explicitly resolve:

1. **session-aware invoke seam**  
   How does the invoke path stamp `session_id` onto recorded runs?

2. **summary-source policy**  
   Is `GetOperation` backed by an in-memory summary cache, a persisted
   projection, or a hybrid?

3. **`operation_id` rule**  
   Is proto `operation_id` the command id, the persisted domain label, or a
   widened ref surface?

4. **event-source rule**  
   Which `RunUpdate` events map to which proto `SessionEvent` values?

5. **create-session materialization rule**  
   Does `CreateSession` allocate metadata only, or instantiate `SessionRuntime`
   eagerly?

These are not optional polish items. They are the actual seam.

## Recommended implementation handoff checklist

When this moves to Composer for implementation, the code slice should:

- [ ] add a dedicated session-api server module, not inline glue in `mcp` or
      `inspect_server`
- [ ] introduce one explicit session-aware invoke entrypoint
- [ ] isolate proto/host mapping from transport handler code
- [ ] isolate the operation summary source from RPC handler code
- [ ] isolate event projection from transport code
- [ ] add narrow tests for session stamping, summary join, and event mapping
- [ ] keep transport concerns out of runtime/storage owners

## Explicit non-goals

API-P4 does **not** approve:

- a gRPC server implementation
- a tonic/axum transport decision
- a daemon lifecycle model
- a session persistence model across restarts
- new persisted `OperationResult` fields
- `SessionEvent` schema expansion
- inspect-server replacement
- MCP unification
- auth / multi-tenant / remote device policy

Any of those needs a later owner-named slice.

## Final decision

API-P4 verdict:

- the next implementation slice may build a **dedicated** session proto server
  seam
- that seam must be **thin**, **session-aware**, and **explicitly two-source**
  for operation summaries
- it must **reuse** existing runtime/invoke/store/recorder owners
- it must **not** blur execute-facing API with inspect server or MCP glue

## Related

- API-P1 boundary review:
  [`2026-06-30-auv-api-p1-session-proto-boundary-review.md`](2026-06-30-session-api-closeout.md)
- API-P3 mapper boundary:
  [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-session-api-closeout.md)
- Proto surface: `proto/auv/api/v1/session.proto`
- Session substrate: `src/session.rs`
- Runtime boundary: `src/runtime.rs`
- MCP invoke surface: `src/mcp.rs`
- Inspect server boundary: `src/inspect_server/mod.rs`
- Recorded invoke path: `crates/auv-cli-invoke/src/recorded.rs`
- Run-update delivery: `crates/auv-tracing-driver/src/recording/`
