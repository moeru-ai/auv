# API-R2: Invoke → OperationResult Persistence Handoff

**Date:** 2026-06-30  
**Status:** Implemented  
**Slice:** Owner-approved feature — session invoke write-through for synthetic `operation-result`

**Decision review:** [API-R1](2026-06-30-auv-api-r1-invoke-operation-result-persistence-decision-review.md)

## Problem resolved

After API-P11, `Invoke` persisted the runtime summary half (`operation-summary`) but
not the persisted skeleton required by API-P7's two-source join. Fresh
`GetOperation` after `Invoke` returned `PersistedOperationRequired` even when the
command completed successfully.

API-R2 closes that gap on the **session invoke write path** only (owner package D2-A).

## Invariant (happy path)

```text
Invoke (completed or failed)
  → run + operation-summary (P11) + operation-result (synthetic, R2)
  → InvokeResponse

GetOperation(run_id)
  → join succeeds
  → known_limits includes auv.api.session.invoke_synthetic_operation_result
```

When **operation-result persist fails** after invoke, `Invoke` still succeeds;
`InvokeResponse.known_limits` includes
`auv.api.session.operation_result_persist_failed`, and `GetOperation` may still
return `PersistedOperationRequired`. That is the partial-success edge, not the
normal case.

## Design

| Piece | Owner | Notes |
| --- | --- | --- |
| Write module | `session_service::operation_result_store` | Three functions + two constants only |
| Builder | `synthetic_operation_result_from_invoke` (private) | Session invoke synthetic skeleton |
| Persist | `persist_operation_result` | `read_run` + `stage_artifact_bytes` + `replace_run_snapshot` |
| Wrapper | `record_invoke_operation_result` | P11-style partial success → `known_limits` |
| Wire | `handler::finish_invoke_response` | After `record_invoke_summary`; merge limits |
| Read | `run_read::read_operation_result` | Unchanged; first JSON `operation-result` artifact |

### Known-limit constants

| Constant | Surface |
| --- | --- |
| `auv.api.session.invoke_synthetic_operation_result` | Persisted `OperationResult.known_limits` (surfaces on `GetOperation` via join) |
| `auv.api.session.operation_result_persist_failed` | `InvokeResponse.known_limits` when append fails |

### Synthetic skeleton policy

| Field | Value |
| --- | --- |
| `api_version` | `OPERATION_RESULT_API_VERSION` |
| `run_id` | `InvokeResult.run_id` |
| `operation_id` | invoke `command_id` (**session invoke synthetic only** — see below) |
| `status` | `RunStatus::Completed` → `Completed`; `Failed` → `Failed` |
| `output` | `Acknowledged { message: Some(output_summary) }` |
| `verifications` | `[]` |
| `evidence_artifacts` | Mapped inline from `InvokeResult.artifacts` |
| `freshness_basis` | `None` |
| `known_limits` | Always includes `invoke_synthetic_operation_result` |

### Session-invoke-local `operation_id` policy

For **session invoke synthetic records only**, internal
`OperationResult.operation_id` equals the invoke `command_id` (e.g.
`fixture.observe`). Typed producers elsewhere in the runtime may still use richer
domain labels (e.g. `music.search.results`). This does **not** change API-P12 wire
semantics: proto `OperationRef.operation_id` remains the invoke `command_id` from
the summary half, not the domain label stored in typed artifacts.

## Partial success (P11 parity)

Invoke execution finishes before artifact append. Persist failure must **not**
surface as an invoke error (non-idempotent blind retry risk). Durability gaps go to
`InvokeResponse.known_limits` only.

Summary and operation-result persist independently: either, both, or neither may
fail. The process-local summary cache is populated only when **all** durability
limits are empty (same gate as pre-R2 summary-only behavior, now covering both
artifacts).

## Duplicate artifacts

Invoke retries may append multiple `operation-result` artifacts. The read path takes
the **first** match, mirroring `read_operation_result` and P11 summary behavior.

## Test matrix

| Test | Location | Asserts |
| --- | --- | --- |
| Synthetic round-trip | `operation_result_store` | Append + `read_operation_result`; marker in `known_limits` |
| Failed invoke status | `operation_result_store` | `RunStatus::Failed` → `OperationStatus::Failed` |
| Persist failure | `operation_result_store` (unix) | `operation_result_persist_failed` limit |
| Handler round-trip | `handler::invoke_then_get_operation_round_trips` | Fresh invoke → get_operation success |
| gRPC round-trip | `transport::grpc_invoke_and_get_operation_round_trips` | Same over tonic |
| External smoke | `session_api_subprocess_smoke_invoke_then_get_operation_round_trips` | API-S1 real-binary gRPC path |
| Dual persist failure | `handler::get_operation_reports_runtime_summary_unavailable_after_summary_persist_failure` | Both summary + operation-result persist limits on invoke |

## Non-goals (R2)

- `auv-cli-invoke::recorded` / MCP / CLI parity — frozen at
  [API-R2b Package A](2026-06-30-auv-api-r2b-invoke-surface-parity-decision-review.md)
- P10 proto changes / D4-A `InvokeCommandOutput.known_limits` plumbing — frozen
  at [API-R2c Package A](2026-06-30-auv-api-r2c-known-limits-plumbing-decision-review.md)
- `run_read` / inspect behavior changes
- Combined summary+operation policy module beyond separate store peers

## Verification

```sh
cargo fmt --check
cargo check
cargo test session_service
cargo test --test session_api_subprocess_smoke
git diff --check
```

## References

- `src/api/session_service/operation_result_store.rs`
- `src/api/session_service/handler.rs` (`finish_invoke_response`)
- [API-P11 summary durability](2026-06-30-auv-api-p11-summary-durability-handoff.md)
- [API-P13 external smoke](2026-06-30-auv-api-p13-external-client-smoke-handoff.md) (Journey B updated post-R2)
