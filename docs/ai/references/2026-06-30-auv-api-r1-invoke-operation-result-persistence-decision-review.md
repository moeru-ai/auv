# API-R1: Invoke → OperationResult Persistence Decision Review

**Date:** 2026-06-30  
**Status:** **docs-only decision review** — records evidence, owner decisions, and
candidate R2 boundaries. **Does not approve implementation.** P14 API lane pause
remains in force until owner accepts this review and names an **API-R2**
implementation slice.

## One-line summary

Fresh session `Invoke` persists `operation-summary` (API-P11) but not
`operation-result`, so `GetOperation` returns `PersistedOperationRequired`. This
review decides **whether** to close that gap, **where** persistence should live,
and **how honest** a synthesized `OperationResult` may be for catalog invoke
commands — before any Rust changes.

## Slice classification

| Item | Value |
| --- | --- |
| This note (API-R1) | **docs-only** |
| Follow-on code (API-R2, if approved) | **owner-approved feature** |
| Not | bug fix, test-only, narrow refactor |

## Problem statement

### User-visible gap

```text
CreateSession → Invoke(fixture.observe) → GetOperation(run_id)
                                              ↓
                         FAILED_PRECONDITION / PersistedOperationRequired
```

P13 Journey B documents this from an external `SessionServiceClient`. It is a
**known gap**, not a P12 identity regression ([API-P14](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)).

### Frozen unary invariant (today)

```text
Invoke → recorded run + operation-summary artifact + InvokeResponse
GetOperation → requires persisted operation-result skeleton + summary join
```

API-P11 closed the **InvokeResult half** (`output_summary`, `signals`,
`failure_message`). API-R1 addresses the **OperationResult half**
(`operation_id` domain label, `known_limits`, `evidence_artifacts`, `output`,
`verifications`).

## Evidence (current code)

| Fact | Location |
| --- | --- |
| Handler NOTICE: no `OperationResult` on invoke | [`handler.rs` L11–15](../../src/api/session_service/handler.rs) |
| `finish_invoke_response` writes summary only | [`handler.rs` L124–144](../../src/api/session_service/handler.rs), [`summary_store.rs`](../../src/api/session_service/summary_store.rs) |
| `GetOperation` requires persisted skeleton | [`summary.rs` `load_joined_operation_summary`](../../src/api/session_service/summary.rs) |
| Two-source join policy frozen | API-P3, API-P7, API-P12 |
| `InvokeResult` has no `known_limits` field | [`auv-cli-invoke/src/model.rs`](../../crates/auv-cli-invoke/src/model.rs) |
| `InvokeCommandOutput.known_limits` dropped at invoke boundary | [`recorded.rs` L176–243](../../crates/auv-cli-invoke/src/recorded.rs) — limits recorded as span events only |
| Production `operation-result` staging exists for typed/runtime paths | vertical probes, `run_recorded_operation`, tests in [`test_fixtures.rs`](../../src/api/session_service/test_fixtures.rs) |
| `auv-cli-invoke` does **not** depend on `contract::OperationResult` | [`crates/auv-cli-invoke/Cargo.toml`](../../crates/auv-cli-invoke/Cargo.toml) |
| Stale name in handler NOTICE | cites `Runtime::record_operation`; actual seam is `run_recorded_operation` / typed producers ([`contract.rs` L119–120](../../src/contract.rs)) |

## Why this was deferred

1. **API-P11 scope** was explicitly summary durability only
   ([handoff deferred list](2026-06-30-auv-api-p11-summary-durability-handoff.md)).
2. **`OperationResult` is the typed command outcome contract** — produced by
   driver/runtime recorded operations with semantic `output` / `verifications`,
   not by the lightweight catalog invoke handler return shape.
3. **Synthesizing `OperationResult` from `InvokeResult` risks a second-quality
   record** that inspect and `run_read` would treat as authoritative unless
   explicitly labeled.
4. **P14 pause** — gap documented; reopen requires owner trigger, not “feels next.”

## Owner decisions required (API-R1 output)

Answer **before** API-R2 implementation.

### D1 — Close the gap?

| Option | Meaning |
| --- | --- |
| **D1-A Accept** | Fresh session `Invoke` should enable `GetOperation` without test fixtures |
| **D1-B Defer** | Keep `PersistedOperationRequired`; document only; revisit with typed-command migration |

**Reviewer recommendation:** **D1-A Accept** — unary API is incomplete for the
`Invoke → GetOperation` journey clients expect. Accept only with explicit
synthetic/honesty policy (D3).

### D2 — Owning write boundary

| Option | Scope | Pros | Cons |
| --- | --- | --- | --- |
| **D2-A Session handler** | `session_service` only, mirror P11 `record_invoke_summary` | Narrow; no `auv-cli-invoke` dependency change; matches API lane ownership | CLI/MCP invoke still lack `operation-result`; two invoke durability models |
| **D2-B Invoke recorded seam** | `auv-cli-invoke::recorded` after successful invoke | One path for session API, MCP, CLI | Requires new dependency or moving `OperationResult` wire type; wider slice |
| **D2-C Typed producers only** | Each command emits real `OperationResult` | Architecturally pure | Long tail; does not land `fixture.observe` quickly |

**Reviewer recommendation:** **D2-A for API-R2 v1**, with **D2-B** as a named
follow-up (**API-R2b**) if owner wants invoke-surface parity before session API
ships. Do **not** start with D2-C alone — catalog commands are not typed
`OperationResult` producers today.

### D3 — Synthetic skeleton honesty policy

If D1-A: what may be written for catalog invoke commands?

| Field | Proposed v1 policy | Risk if wrong |
| --- | --- | --- |
| `operation_id` (domain, internal) | `command_id` for catalog invoke (e.g. `fixture.observe`) | Collides with typed ops that use richer domain labels (`music.search.results`) |
| `status` | Map `RunStatus` → `OperationStatus` | Low |
| `output` | `OperationOutput::Acknowledged { message: Some(output_summary) }` | Inspect may over-read semantic success |
| `verifications` | Empty | Correct for observe/capture commands |
| `evidence_artifacts` | Map `InvokeResult.artifacts` → `contract::ArtifactRef` | Must match run catalog |
| `known_limits` | Merge: durability limits (P11-style) + **marker** `auv.api.session.invoke_synthetic_operation_result` | Without marker, synthetic records look typed |
| Wire `operation_id` (API) | Unchanged P12: `command_id` from summary record | — |

**Rejected without owner call:**

- Fabricating `Candidates` / `Verification` from invoke summary text
- Silent empty `OperationResult` with no synthetic marker
- Using domain label on wire (P12 regression)

### D4 — `known_limits` plumbing prerequisite

`InvokeCommandOutput.known_limits` never reaches `InvokeResult`. Today only
API-P11 durability limits appear on `InvokeResponse.known_limits`.

| Option | Slice impact |
| --- | --- |
| **D4-A R2 includes plumbing** | Extend `InvokeResult` or invoke→persist mapping to carry command limits into persisted `OperationResult.known_limits` |
| **D4-B R2 defers** | Synthetic `OperationResult` carries durability + synthetic marker only; command limits stay event-only |

**Reviewer recommendation:** **D4-B for R2 v1** — smaller slice; document gap;
**D4-A** as **API-R2c** if inspect parity matters.

## Recommended decision package (for owner sign-off)

```text
D1-A  Accept closing the gap
D2-A  Session handler write-through (API-R2 v1)
D3    Synthetic skeleton with invoke_synthetic_operation_result marker
D4-B  Defer command known_limits plumbing
```

This reopens the P14 trigger **operation-result on Invoke** for a **bounded**
implementation slice only — not P10, not MCP merge.

## Candidate API-R2 slice (not approved here)

**If** owner accepts the package above:

| Piece | Owner | Notes |
| --- | --- | --- |
| Build synthetic `OperationResult` | `session_service` mapper or small builder module | From `InvokeResult` + `command_id` + D3 policy |
| Persist artifact | `operation_result_store.rs` (new) or extend `summary_store.rs` | Mirror P11 partial-success: invoke already finished; persist failure → `known_limits` on `InvokeResponse`, not invoke error |
| Wire into handler | `finish_invoke_response` | After `record_invoke_summary` |
| Promote staging helper | Lift `persist_operation_result_on_store` from `test_fixtures` | Production path; tests keep using fixtures |
| Regression tests | `handler.rs`, update P13 Journey B | `invoke` → `get_operation` succeeds for `fixture.observe` |
| Handler NOTICE | Update stale `Runtime::record_operation` reference | Point to R2 seam |

### API-R2 non-goals

- `StreamSessionEvents` (P10)
- Proto / MCP surface changes
- `json_payload` envelope (P3 OD5)
- Full typed `OperationResult` for every catalog command (D2-C)
- MCP/CLI invoke parity (defer **API-R2b** unless owner widens)
- Changing `GetOperation` join semantics or P12 wire identity
- `run_read` / inspect HTTP behavior changes beyond reading new artifact

### API-R2 validation floor

```sh
cargo fmt --check
cargo check
cargo test session_service
cargo test --test session_api_subprocess_smoke
git diff --check
```

Expected test shifts:

- `get_operation_without_persisted_record_requires_persisted_operation_result` →
  **delete or invert** (becomes success path test)
- P13 Journey B → **replace** with invoke→get_operation success journey (or add
  Journey D; owner chooses — do not duplicate)

## Anti-misread rules (API-R1)

1. **Synthetic `OperationResult` ≠ typed runtime record** — inspect consumers
   must not treat `invoke_synthetic_operation_result` runs as full semantic
   verification evidence.
2. **P12 wire `operation_id` stays `command_id`** — internal domain label in JSON
   artifact may equal `command_id` for catalog invoke without changing wire rules.
3. **P11 partial-success policy applies** — persist failure after successful
   invoke must not fail the invoke RPC.
4. **API-R1 approval ≠ API-R2 auto-start** — owner must name R2 explicitly.
5. **Session-only R2 does not fix CLI `GetOperation`** — there is no CLI
   `GetOperation` today; gap is session API specific until R2b.

## Alternatives considered

### A. GetOperation without `OperationResult` (summary-only mode)

Join from `operation-summary` alone when no `operation-result` exists.

**Rejected:** Violates API-P7/P4 explicit policy — persisted skeleton is required;
would fork read semantics and confuse inspect alignment.

### B. Block GetOperation until typed migration

Keep precondition; improve error docs only.

**Rejected for product path:** Leaves unary API half-functional; P13 smoke encodes
the embarrassment.

### C. Duplicate fields into `operation-summary` artifact

Stuff `operation_id` / `known_limits` into summary JSON.

**Rejected:** Conflates two artifacts; breaks `run_read::read_operation_result`
contract.

## Open questions for owner (blocking R2)

1. Accept synthetic marker string `auv.api.session.invoke_synthetic_operation_result` or prefer a version bump on `OPERATION_RESULT_API_VERSION`?
2. P13 Journey B: **flip** to success or **add** parallel journey and keep B as historical negative test?
3. R2 v1 session-only OK, or require **API-R2b** invoke-crate persist in same PR?
4. Failed invoke (`RunStatus::Failed`): persist failed `OperationResult` skeleton or skip persist (GetOperation still fails with same precondition)?

**Reviewer default:** persist failed skeleton with `OperationStatus::Failed` so
`GetOperation` can answer failed runs consistently with summary join.

## Relationship to P14 pause

| P14 statement | API-R1 effect |
| --- | --- |
| Reopen trigger: **operation-result on Invoke** | This review satisfies “decision before slice” |
| Pause remains until owner names R2 | R1 alone does not land code |
| Does not auto-unlock P10 / MCP | Unchanged |

Update [API-P14](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md) only
after owner accepts D1–D4 and names API-R2.

## Related

- [API-P14 pause / reopen triggers](2026-06-30-auv-api-p14-api-line-closeout-pause-decision.md)
- [API-P11 summary durability](2026-06-30-auv-api-p11-summary-durability-handoff.md)
- [API-P3 two-source projection](2026-06-30-auv-api-p3-session-proto-mapper-boundary-handoff.md)
- [API-P12 wire identity](2026-06-30-auv-api-p12-identity-role-semantics-closeout.md)
- [API-P13 external smoke](2026-06-30-auv-api-p13-external-client-smoke-handoff.md)
- [`contract::OperationResult`](../../src/contract.rs)
- [`session_service` implementation](../../src/api/session_service/)
