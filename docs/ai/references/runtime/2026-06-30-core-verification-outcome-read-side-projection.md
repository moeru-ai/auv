# 2026-06-30 AUV Core-C3 D2: verification_outcome read-side projection

Date: 2026-06-30

Status: **landed (read-side only)**. Core-C3 **paused** after D2 — no D3 runtime,
schema, controller, or lease work unless owner reopens with a named slice.

## One-line summary

Core-C3 D2 adds neutral Layer 3 read-side summaries (`verification_outcome`,
`verification_source`, `verification_reason`) on MC-19 and osu query wired live
action paths, projecting from existing `OperationResult` donors and honestly marking
`absent` / `not_attempted` where no post-action verification evidence exists.

## Upstream boundary

- D1 boundary review:
  [`2026-06-30-auv-core-c3-post-action-verification-outcome-boundary.md`](2026-06-30-core-post-action-verification-outcome-boundary.md)
- Core-C1 admission layers:
  [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md)
- Parallel read-side pattern (Core-C2):
  [`2026-06-30-auv-core-c2-prep-admission-dispatch-read-side-vocabulary-alignment.md`](2026-06-30-core-prep-admission-dispatch-read-side-vocabulary-alignment.md)

## Scope landed

**In scope (D2 only):**

| Surface | Change |
| --- | --- |
| `src/run_read.rs` | `MinecraftQueryWiredLiveActionSummary` / `OsuQueryWiredLiveActionSummary` + private mapper `resolve_query_wired_live_action_verification_projection` |
| `src/inspect.rs` | MC-19 / osu wired live action text lines |
| `src/inspect_server_viewer.html` | MC-19 query wired live action card (viewer parity with Rust read path) |

**Explicit non-goals (unchanged from D1):**

- No runtime / producer changes (`minecraft_query_live_action.rs`, `osu_query_live_action.rs`)
- No `OperationResult` or persisted artifact schema changes
- No `source_action_ref` field (deferred — see §Deferred)
- No candidate-action execution lineage projection (archived AX donor only in D1)
- No Minecraft `live_click` dedicated read summary (generic `list_verifications` unchanged)
- No planner, skill controller, action lease, generic verifier trait

## Read-side fields

| Field | Type | Role |
| --- | --- | --- |
| `verification_outcome` | `String` (always set) | Neutral Layer 3 summary label |
| `verification_source` | `Option<String>` | Provenance pointer (`key=value` pairs, same discipline as `source_readiness_ref`) |
| `verification_reason` | `Option<String>` | Human-readable inspect hint; not a substitute for full `VerificationResult` JSON |

### `verification_outcome` vocabulary (provisional)

| Value | When projected |
| --- | --- |
| `not_attempted` | `attempted=false` (Layer 1 refusal; Layer 3 N/A) |
| `absent` | `attempted=true` but no `VerificationResult` on linked `operation-result` (MC-19 / osu wired by design) |
| `passed` | Semantic claim with `semantic_matched=true` and no contradicting failure |
| `failed` | `semantic_matched=false` or `failure_layer` in `{semantic_mismatch, state_changed_no_match}` |
| `unreliable` | `failure_layer=verification_unreliable` |
| `inconclusive` | `state_changed=true` and `semantic_matched` unset |
| `activation_only` | Only `Custom { name: "activation_only" }` claims (Layer 2 boundary marker, not semantic pass) |

**Discipline:** `dispatch_outcome=failed` does **not** map to `verification_outcome=failed`.
Dispatch/driver failure stays Layer 2 (Core-C1).

### `verification_source` formats landed

| Situation | Example |
| --- | --- |
| Layer 1 no dispatch | `kind=layer1_no_dispatch` |
| Layer 3 anchor (operation-result present) | `kind=operation_result artifact_id=… run_id=…` |

D1's separate `source_action_ref` name was **not** introduced; `verification_source`
with `kind=operation_result` covers the wired-path anchor for D2.

### `verification_reason` sources

| Situation | Source |
| --- | --- |
| `not_attempted` | `refusal_reason` from outcome event, or default N/A message |
| `absent` (wired paths) | First `operation-result.known_limits` entry (e.g. MC-19 D4 / osu wired known limit) |
| Claims present | `observed_label` or `failure_layer` snippet from `VerificationResult` |

## Projection logic (private mapper)

Location: `src/run_read.rs` — `NOTICE(core-c3-d2)` block.

```text
attempted=false
  → verification_outcome=not_attempted
  → verification_source=kind=layer1_no_dispatch

attempted=true, operation-result missing on read path
  → verification_outcome=absent (no verification_source)

attempted=true, operation-result present, verifications empty
  → verification_outcome=absent
  → verification_source=kind=operation_result …
  → verification_reason=known_limits[0] or generic absent message

attempted=true, verifications non-empty
  → project from VerificationResult claims (prefer non-activation_only claims)
```

Claims are read from `OperationResult.verifications` first, then legacy
`OperationOutput::Verification` (same policy as `extract_verifications`).

## Donor behavior after D2

| Path | `verification_outcome` (typical) | Notes |
| --- | --- | --- |
| MC-19 query wired live action | `absent` when `attempted=true` | `verifications: Vec::new()` + known limit preserved in `verification_reason` |
| osu query wired live action | `absent` when `attempted=true` | Same |
| MC-19 / osu refusal gates | `not_attempted` | `attempted=false` |
| Minecraft `live_click` (separate CLI path) | Not on wired summary | Would project `passed`/`failed` if wired summary were extended; generic `Verifications:` inspect panel unchanged |
| candidate-action execution (archived) | Not in D2 scope | `CandidateActionExecutionLineage` unchanged |

## Inspect / viewer surfaces

Text inspect (`src/inspect.rs`) appends after `source_readiness_ref`:

```text
verification_outcome=… verification_source=… verification_reason=…
```

MC-19 viewer card (`deriveQueryWiredLiveActionCard`) mirrors Rust projection for
client-side inspect when artifact JSON is cached.

## Validation

Commands run for this slice:

- `cargo fmt --check`
- `cargo check`
- `cargo test -p auv-cli --lib`
- `git diff --check`

Focused regression coverage:

- `query_wired_live_action_verification_projection_maps_semantic_pass_and_absent`
- Existing MC-19 / osu wired summary tests (absent / not_attempted assertions on gates)
- `render_run_text_renders_query_wired_live_action_three_gates`
- `render_run_text_renders_osu_query_wired_live_action_three_gates`

## D1 open questions — D2 resolution

| D1 question | D2 resolution |
| --- | --- |
| MC-19 是否接入 world diff？ | **No** — stay wiring-only; read-side marks `verification_outcome=absent` with known_limit in `verification_reason` |
| `source_action_ref` 指向哪里？ | **Deferred** — D2 uses `verification_source` + `kind=operation_result` only; no separate `source_action_ref` field |
| archived AX verification 纳入 D2？ | **No** — archived donor only; candidate-action lineage not extended |

## Core-C3 pause boundary

Core-C3 stops after D1 (boundary) + D2 (read-side projection). Do **not** continue
into:

- D3+ runtime backfill (e.g. MC-19 gameplay verification on wired path)
- Persisted `verification_outcome` on `OperationResult`
- `source_action_ref` or cross-vertical unified Layer 3 inspect section
- Generic verifier trait / shared crate (Core-B pause)
- Controller, planner, action lease (Core-A7 / MC-20 pause)

### Reopen triggers (owner-named slice only)

| Slice | Trigger |
| --- | --- |
| MC-19+ verification on wired path | Owner names vertical slice; likely reuses `live_click` world diff producer |
| `source_action_ref` read-side field | Owner approves provenance shape distinct from `verification_source` |
| candidate-action / live_click summary cards | Owner names read-side extension beyond wired summaries |
| Persisted `verification_outcome` | Repetition pain + explicit schema approval |
| Core-C3 D3+ | Owner reopens Core-C3 after reviewing D1 + this D2 handoff |

## Related code markers

- `NOTICE(core-c3-d2)` in `src/run_read.rs`, `src/inspect_server_viewer.html`
- `NOTICE(core-c2-d2)` for `source_readiness_ref` (adjacent provenance, separate concern)

## One-sentence closure

Core-C3 D2 closes the read-side Layer 3 vocabulary gap for query wired live action
summaries without claiming verification where donors intentionally omit it; **Core-C3
is paused** until an owner names a follow-up slice from the reopen table above.
