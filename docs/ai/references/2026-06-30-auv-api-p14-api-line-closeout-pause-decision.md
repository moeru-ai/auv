# API-P14: API Line Closeout / Pause Decision

**Date:** 2026-06-30  
**Status:** **final closeout + pause decision** — the approved session API unary lane
and external client smoke coverage are closed for their named scope. This note records
what landed from P1 through P13, which gaps are intentional deferrals, and which
owner-named triggers would reopen work. **No new implementation is approved by this
note.**

## One-line summary

The session API **unary** surface (`CreateSession` / `Invoke` / `GetOperation`) and
external client smoke coverage are landed and test-backed. **`StreamSessionEvents`**
remains unwired (P10 deferred). Session **Invoke → `OperationResult` persistence**
is **closed by API-R2**; MCP/CLI catalog invoke join-artifact divergence is
**frozen session-only by API-R2b-A**. Further API lane work requires an explicit
owner-named slice; pause does not imply P10 or R2b-impl is "next."

## Owner freeze block

```text
unary 已有：CreateSession / Invoke / GetOperation
external smoke 已有：transport gRPC + API-S1 subprocess
stream 仍未启用：P10 defer
session Invoke -> OperationResult：closed by API-R2
MCP/CLI invoke -> join artifacts：frozen open by API-R2b-A
```

## Post-R2 errata (2026-06-30)

```text
session Invoke -> GetOperation happy path：closed by API-R2
MCP/CLI invoke -> join artifacts：intentional boundary, frozen by API-R2b-A
known_limits plumbing：frozen by API-R2c Package A
P14 pause boundary unchanged
```

Pointers: [R2 handoff](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md),
[R2b review](2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md),
[R2c review](2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md).

### English expansion (for reviewers)

| Statement | Meaning | Evidence |
| --- | --- | --- |
| Unary landed | All three unary RPCs wired through handler + loopback gRPC transport | [`handler.rs`](../../src/api/session_service/handler.rs), [`transport.rs`](../../src/api/session_service/transport.rs) |
| External smoke landed | Real `SessionServiceClient` over loopback TCP | `transport.rs` gRPC tests and [API-S1 subprocess smoke](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md) |
| Stream not enabled | `StreamSessionEvents` returns `UNIMPLEMENTED` / `NotWired` | [`transport.rs` L206–212](../../src/api/session_service/transport.rs), [`handler.rs` L208–225](../../src/api/session_service/handler.rs) |
| Session Invoke → OperationResult | Happy path persists synthetic `operation-result` on session `Invoke` (API-R2) | [R2 handoff](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md), `transport::grpc_invoke_and_get_operation_round_trips`, `session_api_subprocess_smoke_invoke_then_get_operation_round_trips` |

## Scope boundary

**In scope for this note:**

- Inventory of P1–P13 landings
- Frozen capability matrix and anti-misread rules
- Pause boundary before P10 / R2b-impl / MCP merge
- Reopen triggers for future owner-named slices

**Out of scope:**

- New Rust, proto, or transport code
- Implementing P10 `StreamSessionEvents`
- Reopening session `Invoke` operation-result wiring (landed in API-R2)
- MCP / inspect-server unification
- Controller / planner / lease / archived AX copilot lanes

This is a **boundary record**, not a proposal to continue implementation.

## Closed phases (P1–P13)

"Closed" means the slice reached its intended endpoint for the named scope. It does
not mean every proto RPC is fully featured.

| Phase | Closure type | Pointer | What "closed" means |
| --- | --- | --- | --- |
| **P1** | Boundary review | [`2026-06-30-auv-api-p1-session-proto-boundary-review.md`](2026-06-30-auv-api-p1-session-proto-boundary-review.md) | Proto surface vocabulary and experimental stability language frozen |
| **P2** | Proto + crate | `proto/auv/api/v1/session.proto`, `crates/auv-api-proto` | Generated types available to server and clients |
| **P3** | Mapper boundary (docs) | [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md) | Two-source `GetOperation` join documented; open decisions catalogued |
| **P4** | Server seam design + modules | [`2026-06-30-auv-api-p4-session-proto-server-seam-design.md`](2026-06-30-auv-api-p4-session-proto-server-seam-design.md), `src/api/session_service/` | Dedicated session API boundary; not MCP/inspect glue |
| **P5** | Session-aware invoke | `handler.rs` `invoke_recorded_with_session` | Runs stamp explicit `session_id` on recorded runs |
| **P6** | Summary cache | `OperationSummaryCache` in handler | Process-local runtime summary for same-handler `GetOperation` |
| **P7** | Two-source join | `summary.rs` | Explicit join policy for `GetOperation` |
| **P8** | Handler skeleton | `handler.rs` | Transport-agnostic unary RPC wiring |
| **P9** | Loopback gRPC | `transport.rs` | `CreateSession`, `Invoke`, `GetOperation` over tonic |
| **P11** | Summary durability | [`2026-06-30-auv-api-p11-summary-durability-handoff.md`](2026-06-30-auv-api-p11-summary-durability-handoff.md) | `operation-summary` artifact persisted on `Invoke` |
| **P12** | Identity / role closeout | [`2026-06-30-auv-api-p12-identity-role-semantics-closeout.md`](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md) | Wire `operation_id` = `command_id`; `ArtifactRef.role` from catalog |
| **P13** | External client smoke | [`2026-06-30-auv-api-p13-external-client-smoke-handoff.md`](2026-06-30-auv-api-p13-external-client-smoke-handoff.md) | Historical dedicated smoke module; later retired as redundant with transport gRPC tests and API-S1 |
| **S1** | Subprocess loopback smoke | [`2026-06-30-auv-api-s1-subprocess-smoke-handoff.md`](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md) | `auv session serve` subprocess proof via `CARGO_BIN_EXE_auv` |

## Frozen capability matrix

| Capability | Status | Notes |
| --- | --- | --- |
| `CreateSession` | **landed** | Lightweight registry; no `SessionRuntime` materialization |
| `Invoke` (blocking unary) | **landed** | Records run + `operation-summary` + synthetic `operation-result` (R2); returns `InvokeResponse` |
| `GetOperation` (with persisted skeleton) | **landed** | Two-source join when `operation-result` artifact exists |
| `GetOperation` after fresh `Invoke` (happy path) | **landed (R2)** | Round-trip succeeds when persist succeeds; `PersistedOperationRequired` on durability failure / missing skeleton |
| External client smoke | **landed** | `cargo test grpc_invoke_and_get_operation_round_trips`; `cargo test --test session_api_subprocess_smoke` |
| Subprocess loopback smoke (S1) | **landed** | `cargo test --test session_api_subprocess_smoke` |
| `StreamSessionEvents` (P10) | **deferred** | Transport `UNIMPLEMENTED`; handler `NotWired` |
| `json_payload` envelope (P3 OD5) | **deferred** | Provisional decoder only; owner-named envelope slice required |

## Unary path invariant (frozen)

```text
CreateSession → register session_id
Invoke(session, command_id) → recorded run + operation-summary + operation-result (R2) + InvokeResponse
GetOperation(run_id) → join operation-result + summary when skeleton exists (happy path)
```

`GetOperation` after `Invoke` is part of the happy-path invariant post-R2.
`PersistedOperationRequired` remains for durability-write failure or when no
skeleton exists (no prior invoke / pre-seeded fixture only).

## Anti-misread rules

These rules are part of the pause boundary.

### 1. Invoke → GetOperation happy path is landed; edge failures are not regressions

Session `Invoke` persists synthetic `OperationResult` on the happy path (API-R2).
The transport gRPC and API-S1 subprocess smokes assert invoke→GetOperation round-trip success post-R2.
`PersistedOperationRequired` still applies when durability writes fail or no
skeleton exists. Treating those edge cases as P12 identity regressions is **wrong**.

### 2. Stream UNIMPLEMENTED is not a transport regression

`StreamSessionEvents` was never wired. P10 is the named future slice. Absence of
streaming is a **documented deferral**, not missing polish on unary RPCs.

### 3. Two different `SessionEvent` types

`src/session.rs::SessionEvent` is observation/action-resource oriented.
`auv.api.session.v1.SessionEvent` is invoke/run/artifact oriented. Mapping one to
the other is a category error (see API-P4 §D).

### 4. P13 smoke ≠ production external API

The in-process transport tests use loopback TCP and hermetic fixtures. They do
not certify remote access, TLS, or gRPC reflection. The subprocess `auv session
serve` loopback path is covered separately by API-S1.

### 5. Pause does not unlock adjacent lanes

P14 closeout does **not** approve MCP/proto server unification, inspect-server
merger, controller, planner, or action lease work.

## Anti-misread rule (main point)

> **API-P14 closeout means "the approved unary session API lane + transport/API-S1 smoke are
> done for their named scope."** It does **not** mean "P10 stream or
> R2b-impl is the obvious next implementation."

### Forbidden misreads

- "P13 smoke is green, so every GetOperation path must work without invoke or fixtures." (Happy path works post-R2; Journey C pre-seeded fixtures remain valid.)
- "Stream is in the proto, so unary closeout should have included P10."
- "P12 fixed identity, so all GetOperation precondition failures are bugs." (Durability-failure and missing-skeleton preconditions remain intentional.)
- "Session API pause means we should merge execute API into inspect_server or MCP."

## Explicit non-goals (P14)

API-P14 does **not** approve:

- implementing P10 `StreamSessionEvents` in this slice
- re-landing session `Invoke` synthetic `OperationResult` persist (API-R2)
- expanding `session.proto` or adding gRPC reflection
- grpcurl / TLS / remote production CI gates for session API (subprocess loopback smoke landed in API-S1)
- MCP / inspect-server route unification
- `json_payload` envelope standardization (P3 OD5)
- reopening controller / planner / lease / archived `candidate-action` lanes

## Reopen triggers

A paused lane does not reopen because it "feels next." It reopens only when the
owner names the trigger **and** the exact slice.

| Trigger | Unlocks (candidate only) | Does **not** auto-unlock |
| --- | --- | --- |
| Owner names **P10** | `StreamSessionEvents` v0 (handler-emitted hub) | RunUpdate projector, `invoke_started`, proto expansion |
| Owner names **API-R2b-impl** (Package B) | MCP/CLI catalog invoke join-artifact parity on shared `store_root` | Stream, MCP merge, R2c-impl |
| Owner names **P3 OD5 envelope** | Versioned `json_payload` decoder | Stream, operation-result |
| Owner names **P10b** | RunUpdate / BroadcastRunRecorder projection | Unary changes |

**Trigger met ≠ implement.** A reopened lane still needs a named slice and fresh
scope review against `CONTRIBUTING.local.md`.

## Validation (re-check state)

Readers verifying this pause record against the repo:

```sh
cargo test session_service
cargo test grpc_invoke_and_get_operation_round_trips
cargo test --test session_api_subprocess_smoke
git diff --check
```

Expected: `session_service` includes handler, mapper, summary, and transport
tests; the transport gRPC round-trip covers in-process client/server behavior;
`session_api_subprocess_smoke` runs the subprocess loopback proof.

## Related

- API-P1 boundary review:
  [`2026-06-30-auv-api-p1-session-proto-boundary-review.md`](2026-06-30-auv-api-p1-session-proto-boundary-review.md)
- API-P3 mapper boundary:
  [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md)
- API-P4 server seam:
  [`2026-06-30-auv-api-p4-session-proto-server-seam-design.md`](2026-06-30-auv-api-p4-session-proto-server-seam-design.md)
- API-P11 summary durability:
  [`2026-06-30-auv-api-p11-summary-durability-handoff.md`](2026-06-30-auv-api-p11-summary-durability-handoff.md)
- API-P12 identity closeout:
  [`2026-06-30-auv-api-p12-identity-role-semantics-closeout.md`](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)
- API-P13 external smoke:
  [`2026-06-30-auv-api-p13-external-client-smoke-handoff.md`](2026-06-30-auv-api-p13-external-client-smoke-handoff.md)
- API-L1 operator guide:
  [`2026-06-30-auv-api-l1-session-api-operator-guide.md`](2026-06-30-auv-api-l1-session-api-operator-guide.md)
- API-S1 subprocess smoke:
  [`2026-06-30-auv-api-s1-subprocess-smoke-handoff.md`](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md)
- MC-20 pause template (pattern reference):
  [`2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`](2026-06-30-minecraft-mc20-final-closeout-pause-decision.md)
- Proto: `proto/auv/api/v1/session.proto`
- API-R2 invoke operation-result handoff:
  [`2026-06-30-auv-api-r2-invoke-operation-result-handoff.md`](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md)
- API-R2b invoke-surface parity freeze:
  [`2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md`](2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md)
- API-R2c known_limits freeze:
  [`2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md`](2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md)
- Implementation: `src/api/session_service/`
