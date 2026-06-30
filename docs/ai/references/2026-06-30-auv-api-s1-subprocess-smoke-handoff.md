# API-S1: Subprocess Session API Smoke

**Date:** 2026-06-30  
**Status:** Implemented  
**Slice:** test-only owner-approved feature

**Predecessor:** [API-P13](2026-06-30-auv-api-p13-external-client-smoke-handoff.md) (in-process external client)  
**Operator entry:** [API-L1](2026-06-30-auv-api-l1-session-api-operator-guide.md)

## Problem

P13 proves session API correctness from a real `SessionServiceClient` over loopback
TCP, but boots the server **in-process** (`bind_session_api` + `serve_on_listener`).
That does not exercise the operator path `auv session serve` as a separate process.

API-S1 closes that gap with a hermetic integration test that spawns the built
`auv` binary via `CARGO_BIN_EXE_auv`.

## Hard constraint

`std::env::current_exe()` inside `src/` unit tests resolves to the **test harness**,
not the `auv` CLI. Subprocess smoke **must** live in `tests/*.rs` integration tests.

## Smoke journey (thin closed loop)

1. Temp `store_root`
2. Spawn: `auv session serve --host 127.0.0.1 --port 0 --store-root <tmp>`
3. Parse stdout line `session API: grpc://127.0.0.1:<port>`
4. `CreateSession` → `Invoke(fixture.observe)` → `GetOperation`
5. Assert P13 Journey B subset (status, summary, synthetic `known_limits` marker)

## Implementation map

| Piece | Location |
| --- | --- |
| Integration test | [`tests/session_api_subprocess_smoke.rs`](../../tests/session_api_subprocess_smoke.rs) |
| Binary spawn | `env!("CARGO_BIN_EXE_auv")` |
| Synthetic marker assertion | Frozen literal in test (matches `operation_result_store` constant) |
| Server readiness line | [`transport.rs`](../../src/api/session_service/transport.rs) `serve_on_listener` (stdout flushed post-print) |

## P13 vs S1

| | P13 | API-S1 |
| --- | --- | --- |
| Server boot | In-process | Subprocess `auv session serve` |
| Binary | Test harness | `CARGO_BIN_EXE_auv` |
| Journeys | A + B + C | B only |

## Non-goals (S1)

- Not `StreamSessionEvents` (P10)
- Not MCP/CLI parity (R2b-A)
- Not grpcurl / reflection CI
- Not `cargo run` inside the test
- Not duplicating P13 Journey C

## Validation

```sh
cargo fmt --check
cargo check
cargo test --test session_api_subprocess_smoke
git diff --check
```
