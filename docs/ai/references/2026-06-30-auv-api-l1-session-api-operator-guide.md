# API-L1: Session API Operator Guide

**Date:** 2026-06-30  
**Status:** operator reference — live entry, RPC boundaries, debugging  
**Slice:** docs-only

**Pause context:** [API-P14](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)  
**Smoke:** [API-P13](2026-06-30-auv-api-p13-external-client-smoke-handoff.md), [API-S1](2026-06-30-auv-api-s1-subprocess-smoke-handoff.md)  
**Freeze pointers:** [R2](2026-06-30-auv-api-r2-invoke-operation-result-handoff.md), [R2b-A](2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md), [R2c-A](2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md)

## One-line summary

This guide is the **human operator entry** for the unary session API: how to start
`auv session serve`, call `CreateSession` / `Invoke` / `GetOperation`, what each
RPC can and cannot do today, and how to debug failures without misreading frozen
P14 / R2 / R2b / R2c boundaries.

## Live entry

### Start the server

```sh
auv session serve [--host <host>] [--port <port>] [--store-root <path>]
```

| Flag | Default | Notes |
| --- | --- | --- |
| `--host` | `127.0.0.1` | Loopback only; non-loopback hosts are rejected |
| `--port` | `9847` | Use `0` for OS-assigned port (subprocess smoke, local dev) |
| `--store-root` | project `.auv` | Run and artifact persistence root |

On startup the server prints a single readiness line to stdout:

```text
session API: grpc://127.0.0.1:<port>
```

Use that line to discover the listen address when `--port 0` is set. Constants
live in [`transport.rs`](../../src/api/session_service/transport.rs):
`DEFAULT_SESSION_API_HOST`, `DEFAULT_SESSION_API_PORT`.

### Proto surface

Service: `auv.api.session.v1.SessionService` — see
[`session.proto`](../../proto/auv/api/v1/session.proto).

**Experimental / unstable** — not a long-term public compatibility promise.

## RPC can / cannot matrix

| RPC | Can (today) | Cannot / deferred |
| --- | --- | --- |
| `CreateSession` | Register lightweight `session_id` with optional `client_label` | Materialize full `SessionRuntime`; stream subscription |
| `Invoke` | Blocking catalog invoke with `session_id` stamp on the recorded run; persists `operation-summary` (P11) + synthetic `operation-result` on happy path (R2); returns `InvokeResponse` with `operation` ref | Fire-and-forget / async job model; MCP/CLI catalog invoke parity (R2b-A frozen) |
| `GetOperation` | Two-source join when persisted skeleton exists; wire `operation_id` = invoke `command_id` (P12); happy path after session `Invoke` (R2) | Semantic verification from synthetic marker alone; success without persisted skeleton |
| `StreamSessionEvents` | — | **UNIMPLEMENTED** — returns `UNIMPLEMENTED` / `NotWired` (P10 deferred) |

## Recommended happy path

```text
CreateSession(client_label)
  → session_id

Invoke(session, command_id="fixture.observe", json_payload=[])
  → InvokeResponse { operation: { run_id, operation_id }, status, known_limits, ... }

GetOperation(operation from InvokeResponse)
  → GetOperationResponse { output_summary, status, known_limits, ... }
```

Expected for `fixture.observe` on the happy path:

- `InvokeResponse.status == "completed"`
- `GetOperationResponse.status == "completed"`
- `output_summary == "fixture observed"`
- `known_limits` includes `auv.api.session.invoke_synthetic_operation_result`

Automated proof: P13 Journey B in
[`client_smoke.rs`](../../src/api/session_service/client_smoke.rs);
subprocess proof: API-S1 integration test.

## What Invoke records

On the session API path only:

| Artifact | When | Consumer |
| --- | --- | --- |
| Recorded run + trace | Always on invoke | `auv inspect`, trace viewers |
| `operation-summary` | Happy-path persist (P11) | `GetOperation` join |
| Synthetic `operation-result` | Happy-path persist (R2) | `GetOperation` join |

**Not** persisted on MCP/CLI catalog invoke (R2b-A freeze). Those surfaces use
trace + `auv inspect` read-back.

## Debugging playbook

### 1. Pick an explicit `store_root`

```sh
auv session serve --store-root /tmp/auv-session-debug
```

All runs and artifacts for that server instance land under this directory.
Reusing the same root across restarts preserves `GetOperation` read-back.

### 2. Inspect the run

After `Invoke`, note `operation.run_id` from the response:

```sh
auv inspect <run_id>
```

Use inspect for span events, command resolution, and driver notes. Command
`known_limits` from catalog output appear as trace span events (`command.known_limit`)
— **not** on `InvokeResponse` (R2c-A).

### 3. `InvokeResponse.known_limits`

Durability-only on the session RPC path, for example:

- `auv.api.session.operation_summary_persist_failed`
- `auv.api.session.operation_result_persist_failed`

These mean invoke **succeeded** but a persist step failed. They are **not**
command honesty limits.

### 4. `GetOperation` returns `PersistedOperationRequired`

| Cause | What to check |
| --- | --- |
| No prior `Invoke` for this `run_id` | Call `Invoke` first, or use a pre-seeded store (P13 Journey C) |
| Durability write failed after invoke | `InvokeResponse.known_limits`; inspect run artifacts on disk |
| Wrong `operation_id` on request | P12: wire `operation_id` must equal invoke `command_id` |

This is **not** a P12 identity regression when the skeleton is genuinely missing
(P14 errata).

### 5. `StreamSessionEvents` errors

Expected. Streaming is P10 deferred — not a unary regression.

### 6. Manual grpcurl (no server reflection)

```sh
grpcurl -plaintext \
  -import-path proto \
  -proto auv/api/v1/session.proto \
  -d '{"client_label":"smoke"}' \
  127.0.0.1:9847 \
  auv.api.session.v1.SessionService/CreateSession
```

See [P13 handoff](2026-06-30-auv-api-p13-external-client-smoke-handoff.md) for the
full manual smoke recipe.

## Frozen boundaries (do not misread)

1. **P14 pause** — unary lane + P13 in-process smoke + API-S1 subprocess smoke
   are closed for named scope; P10 stream and R2b-impl are **not** implied next steps.

2. **R2b-A** — synthetic summary + operation-result write-through stays
   `session_service`-only. MCP/CLI catalog invoke does not gain join artifacts
   without owner Package B + named consumer.

3. **R2c-A** — command `known_limits` stay trace-only; persisted synthetic
   `OperationResult.known_limits` carries the honesty marker only.

4. **Synthetic ≠ typed** — runs with `invoke_synthetic_operation_result` are
   skeleton join artifacts, not full semantic verification records.

5. **Loopback only** — session API server refuses non-loopback bind (P9).

## Operator validation commands

```sh
cargo run --quiet -- session serve --help
cargo test session_api_smoke
cargo test --test session_api_subprocess_smoke
git diff --check
```

## Related

- [P4 server seam](2026-06-30-auv-api-p4-session-proto-server-seam-design.md)
- [P12 identity closeout](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)
- Implementation: `src/api/session_service/`
