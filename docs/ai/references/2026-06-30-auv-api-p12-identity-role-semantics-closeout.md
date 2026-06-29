# API-P12: Identity / Role Semantics Closeout

**Date:** 2026-06-30  
**Status:** Implemented  
**Slice:** Owner-approved feature — close API-P3 open decisions 2 and 4

## Problem

After API-P11, the unary `GetOperation` summary path is durable, but two contract
cracks remain on the API wire:

1. **OD2 — `operation_id` fork:** `InvokeResponse` maps `OperationRef.operation_id`
   to `InvokeRequest.command_id`, while `GetOperationResponse` mapped it to the
   persisted domain label `OperationResult.operation_id` (e.g.
   `music.search.results`). Same run, different meanings.
2. **OD4 — `ArtifactRef.role` hollow on read:** `GetOperation` evidence refs came
   from slim `contract::ArtifactRef` values with no `role`; the mapper returned
   empty strings instead of joining the run artifact catalog.

P13 external client smoke and P10 `StreamSessionEvents` depend on stable
`OperationRef` / `ArtifactRef` semantics. This slice closes both gaps.

## Locked decisions

### Decision 1 — `OperationRef.operation_id` on the API wire

**Rule:** proto `operation_id` = **invoke `command_id`** on both `InvokeResponse`
and `GetOperationResponse`.

| Layer | Field | Meaning |
| --- | --- | --- |
| API wire | `OperationRef.operation_id` | invoke command identity |
| Internal persisted | `OperationResult.operation_id` | domain / typed-operation label (unchanged) |
| Proto request | `InvokeRequest.command_id` | same as wire `operation_id` |

**Rejected:**

- domain label on wire — contradicts API-P1/P2 proto comment
- widened `OperationRef` with two id fields — unnecessary once wire identity is `command_id`

**`command_id` resolution on `GetOperation` (priority):**

1. `command_id` on persisted `operation-summary` artifact (`OperationSummaryRecord`, API-P11 extension, version `v1alpha2`)
2. `auv.command.id` span attribute on `auv.command.invoke`
3. Gap: known_limit `auv.api.session.command_id_unavailable`, empty wire `operation_id` — **no** fallback to domain `OperationResult.operation_id`

**Request validation:** non-empty `GetOperationRequest.operation.operation_id` must
match the resolved wire `command_id`, else `OperationIdMismatch`.

### Decision 2 — proto `ArtifactRef.role` authoritative source

**Rule:** the run artifact catalog (`artifacts.jsonl` / `ArtifactRecordV1Alpha1.role`)
is authoritative on all API projections.

| Path | Source |
| --- | --- |
| `Invoke` | `InvokeResult.artifacts` records |
| `GetOperation` | catalog lookup by `(run_id, artifact_id)` for each evidence ref |

**Rejected:** extending `contract::ArtifactRef` with `role`.

**Gap policy:** empty `role` plus known_limit `auv.api.session.artifact_role_unavailable`
when the catalog has no matching `artifact_id`.

## Implementation map

| Piece | Location |
| --- | --- |
| Wire `command_id` on summary record | `OperationSummaryRecord.command_id`, `OPERATION_SUMMARY_API_VERSION` bump |
| Span fallback | `run_read::read_invoke_command_id` |
| Role catalog | `run_read::artifact_role_catalog` |
| Join + resolve | `session_service::summary` |
| Proto mapping | `session_service::mapper` |
| Handler validation | `session_service::handler` |

## Test matrix (Phase B)

- Invoke + GetOperation both emit `command_id` when domain `operation_id` differs
- GetOperation fills `role` from catalog for evidence artifacts
- Missing catalog role → `artifact_role_unavailable` known_limit
- Missing `command_id` sources → `command_id_unavailable` known_limit
- Non-empty request `operation_id` mismatch → `OperationIdMismatch`
- `operation-summary` v1alpha1 records without `command_id` still read (span fallback)

## Deferred (not this slice)

- `OperationResult` persistence on session `Invoke`
- `StreamSessionEvents` (P10)
- External client smoke (P13)
- `json_payload` envelope (API-P3 OD5)
- First-artifact-wins stale `command_id` on invoke retry (documented; same as P11 summary read policy)
- `contract::ArtifactRef` shape change

## Related

- API-P3 mapper boundary (OD2/OD4 resolved):
  [`2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md`](2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md)
- API-P11 summary durability:
  [`2026-06-30-auv-api-p11-summary-durability-handoff.md`](2026-06-30-auv-api-p11-summary-durability-handoff.md)
- Proto: `proto/auv/api/v1/session.proto`

## Validation

```sh
cargo fmt --check
cargo check
cargo test session_service
git diff --check
```
