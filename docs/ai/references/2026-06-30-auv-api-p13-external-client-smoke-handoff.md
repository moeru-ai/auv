# API-P13: First External Client Smoke

**Date:** 2026-06-30  
**Status:** Implemented  
**Slice:** Test-first / smoke-only owner-approved feature

**Predecessor:** [API-P12 identity / role closeout](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)

## Current repository note

The dedicated `src/api/session_service/client_smoke.rs` module described below
has since been removed as redundant. Equivalent loopback gRPC coverage remains in
`src/api/session_service/transport.rs`, and the stronger real-binary path is
covered by `tests/session_api_subprocess_smoke.rs`.

## Problem

Before P13, session API correctness was proven mostly from **inside** the crate:
handler unit tests, mapper tests, and two transport-level gRPC tests in
`transport.rs`. No dedicated module exercised the **external client** path —
a real `SessionServiceClient` connecting over loopback TCP — with explicit
journeys and documented gaps.

P12 locked wire `operation_id` = invoke `command_id`. P13 validates that
contract from the client perspective without widening into runtime persistence
work.

## Historical gap (closed by API-R2)

Before [API-R2](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md), fresh
`Invoke` wrote `operation-summary` (P11) but not `operation-result`, so
`GetOperation` returned `PersistedOperationRequired`. Journey B originally documented
that deferral; it now asserts the happy-path round-trip after R2 landed.

## Smoke journeys

These journeys originally lived in `src/api/session_service/client_smoke.rs` and
used a real `SessionServiceClient` against `bind_session_api` +
`serve_on_listener` on `127.0.0.1:0`. That dedicated module was later retired in
favor of the existing transport-level gRPC tests plus the subprocess smoke.

### Journey A — Unary invoke (green)

1. Fresh temp `store_root`
2. `CreateSession` → non-empty `session_id`
3. `Invoke(session, "fixture.observe", empty json_payload)`
4. Assert: `status == "completed"`, non-empty `run_id`, `operation_id == "fixture.observe"`

### Journey B — Invoke → GetOperation round-trip (green, post-R2)

1. Continue Journey A with same client and `OperationRef` from invoke
2. `GetOperation(operation)`
3. Assert: `status == "completed"`, `output_summary == "fixture observed"`,
   `operation.operation_id == "fixture.observe"`, and
   `known_limits` includes `auv.api.session.invoke_synthetic_operation_result`

**Purpose:** external client proves the API-R2 happy path end-to-end over loopback TCP.

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
cargo test --test session_api_subprocess_smoke
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
| External client smoke tests | Retired; covered by `transport.rs` gRPC tests and `tests/session_api_subprocess_smoke.rs` |
| Module wiring | Retired |
| Journey C fixtures | existing `test_fixtures::persist_operation_result_and_summary_run` |
| Server boot (reused, not refactored) | `transport::bind_session_api`, `serve_on_listener` |

`transport.rs` changed only at visibility: `serve_on_listener` is now `pub(crate)` so
the sibling smoke module can boot the same loopback server path without copying
transport internals.

## Non-goals (P13)

- Not refactoring or deduplicating `transport.rs` gRPC tests
- Not asserting `ArtifactRef.role` in smoke journeys
- Not persisting typed `OperationResult` on non-session invoke paths (API-R2b)
- Not adding tonic gRPC reflection
- Not subprocess `auv session serve` smoke (landed separately in [API-S1](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md))
- Not `StreamSessionEvents` (P10)
- Not MCP / inspect-server unification

## Follow-ups (historical / now landed where noted)

| Item | Trigger |
| --- | --- |
| Journey D: fresh-store Invoke → GetOperation green | **landed in API-R2** |
| Subprocess `auv session serve` smoke | **landed in API-S1** |
| Optional 10–20 line server-boot helper share | Only if byte-identical duplication remains after smoke lands |

## Validation

```sh
cargo fmt --check
cargo check
cargo test --test session_api_subprocess_smoke
git diff --check
```
