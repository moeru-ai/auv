# API-P13: First External Client Smoke

**Date:** 2026-06-30  
**Status:** Implemented  
**Slice:** Test-first / smoke-only owner-approved feature

**Predecessor:** [API-P12 identity / role closeout](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)

## Problem

Before P13, session API correctness was proven mostly from **inside** the crate:
handler unit tests, mapper tests, and two transport-level gRPC tests in
`transport.rs`. No dedicated module exercised the **external client** path —
a real `SessionServiceClient` connecting over loopback TCP — with explicit
journeys and documented gaps.

P12 locked wire `operation_id` = invoke `command_id`. P13 validates that
contract from the client perspective without widening into runtime persistence
work.

## Root cause: Invoke → GetOperation precondition gap

```text
CreateSession → Invoke(fixture.observe)
  → handler records run + operation-summary (P11)
  → NO operation-result artifact written

GetOperation(same OperationRef)
  → run_read::read_operation_result returns None
  → PersistedOperationRequired → gRPC FAILED_PRECONDITION
```

This is a **design deferral**, not a P12 regression. See handler NOTICE in
`src/api/session_service/handler.rs` and internal proof in
`transport.rs::grpc_invoke_and_get_operation_failed_precondition`.

## Smoke journeys

All journeys live in `src/api/session_service/client_smoke.rs` and use a real
`SessionServiceClient` against `bind_session_api` + `serve_on_listener` on
`127.0.0.1:0`.

### Journey A — Unary invoke (green)

1. Fresh temp `store_root`
2. `CreateSession` → non-empty `session_id`
3. `Invoke(session, "fixture.observe", empty json_payload)`
4. Assert: `status == "completed"`, non-empty `run_id`, `operation_id == "fixture.observe"`

### Journey B — Honest precondition gap (required)

1. Continue Journey A with same client and `OperationRef` from invoke
2. `GetOperation(operation)`
3. Assert: `Code::FailedPrecondition`, message contains `no persisted operation result`

**Purpose:** external client documents the known deferral so future failures are
not mistaken for transport or identity regressions.

### Journey C — P12 wire identity on GetOperation (green)

Uses a **pre-seeded store** via `test_fixtures::persist_operation_result_and_summary_run`:

- `operation-result` with domain `operation_id = "music.search.results"`
- `operation-summary` with `command_id = "music.search"`

Flow:

1. Start server on pre-seeded `store_root`
2. `CreateSession` (session independent of run)
3. `GetOperation(OperationRef { run_id, operation_id: "music.search" })`
4. Assert: `status == "completed"`, `operation.operation_id == "music.search"`,
   `output_summary == "did the thing"`

**Not asserted in P13:** `ArtifactRef.role` — covered by P12 `mapper.rs` unit tests.

## Manual smoke recipe

Automated smoke uses in-process TCP (same binary, no subprocess). For manual
exploration with the CLI server:

```sh
# Terminal 1 — loopback session API (default port 9847)
cargo run --quiet -- session serve --store-root /tmp/auv-smoke

# Terminal 2 — run automated external-client smoke
cargo test session_api_smoke
```

Optional grpcurl (no server reflection in P13; requires local proto import path):

```sh
grpcurl -plaintext \
  -import-path proto \
  -proto auv/api/v1/session.proto \
  -d '{"client_label":"smoke"}' \
  127.0.0.1:9847 \
  auv.api.session.v1.SessionService/CreateSession
```

## Implementation map

| Piece | Location |
| --- | --- |
| External client smoke tests | `session_service/client_smoke.rs` |
| Module wiring | `session_service/mod.rs` (`#[cfg(test)] mod client_smoke`) |
| Journey C fixtures | existing `test_fixtures::persist_operation_result_and_summary_run` |
| Server boot (reused, not refactored) | `transport::bind_session_api`, `serve_on_listener` |

`transport.rs` changed only at visibility: `serve_on_listener` is now `pub(crate)` so
the sibling smoke module can boot the same loopback server path without copying
transport internals.

## Non-goals (P13)

- Not refactoring or deduplicating `transport.rs` gRPC tests
- Not asserting `ArtifactRef.role` in smoke journeys
- Not persisting `OperationResult` on `Invoke` (future owner-named slice)
- Not adding tonic gRPC reflection
- Not subprocess / `auv session serve` CI smoke
- Not `StreamSessionEvents` (P10)
- Not MCP / inspect-server unification

## Deferred follow-ups

| Item | Trigger |
| --- | --- |
| Journey D: fresh-store Invoke → GetOperation green | Owner slice wires `operation-result` on Invoke |
| Subprocess `auv session serve` smoke | P13b or manual-only until port discovery is stable |
| Optional 10–20 line server-boot helper share | Only if byte-identical duplication remains after smoke lands |

## Validation

```sh
cargo fmt --check
cargo check
cargo test session_api_smoke
git diff --check
```
