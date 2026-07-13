# 2026-06-30 AUV Core-C3 D1: Post-action verification outcome boundary review

Date: 2026-06-30

Status: **docs-only boundary review (D1)** — substrate research and donor inventory.
**D2 landed:** read-side projection in
[`2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md`](2026-06-30-core-verification-outcome-read-side-projection.md).
**Core-C3 paused** after D2; see that handoff for reopen triggers.

No runtime changes in D1; D2 was read-side only (no schema/runtime in either slice).

## One-line summary

Core-C3 D1 maps the **Layer 3 boundary** (post-action semantic verification) against
existing donors and read-side surfaces, separates it from Core-C1 admission Layers 1–2,
and inventories where `VerificationResult` evidence already exists versus where
`verification_outcome` remains an intentionally empty conceptual slot.

## Scope boundary

**In scope:**

- Three-layer failure separation (pre-dispatch / driver / post-action)
- Provisional `verification_outcome` vocabulary candidates (read-side only)
- Provenance field discussion relative to Core-C2 `source_readiness_ref`
- Donor inventory with file paths and artifact roles
- D2 scope recommendation

**Out of scope (explicit non-goals — see also §Non-goals):**

- Implementation, runtime extraction, persisted schema changes
- Generic verifier trait, planner, skill controller, action lease
- Backfilling gameplay verification into MC-19 / osu wired paths without owner slice

## Primary inputs

- [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md)
- [`2026-06-30-auv-core-c2-prep-admission-dispatch-read-side-vocabulary-alignment.md`](2026-06-30-core-prep-admission-dispatch-read-side-vocabulary-alignment.md)
- [`docs/TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md) — Verification Method, Action Resolver
- [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](../apps/minecraft/2026-06-27-minecraft-probe-19-reference.md)
- [`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`](../apps/osu/2026-06-28-osu-visual-truth-query-wired-live-action-design.md)
- Code: `src/contract.rs`, `src/run_read.rs`, `src/inspect.rs`, `src/candidate_action_decision.rs`,
 `src/main.rs`, `src/minecraft_query_live_action.rs`, `src/osu_query_live_action.rs`,
 `crates/auv-game-minecraft/src/verify.rs`, `crates/auv-game-osu/src/detection_eval_witness.rs`

---

## 1. Three-layer separation (三层分离)

Core-C1 already defines the separation this review inherits. Core-C3 D1 confirms
repo evidence aligns with that model and clarifies what each layer **owns**.

```text
Layer 1 — Pre-dispatch refusal (semantic / readiness gate)
 readiness_class ∈ { non_actionable, not_consumable }
 OR defensive refusal while nominally ready
 → attempted=false, refusal_reason set
 → NO dispatch_outcome, NO verification_outcome

Layer 2 — Dispatch / driver failure after attempt
 readiness_class=ready (or equivalent), attempted=true
 → dispatch_outcome records invoke/driver error
 → input may have been attempted; semantic world state NOT verified
 → NOT conflated with Layer 1 refusal

Layer 3 — Post-action verification failure (or success)
 attempted=true, dispatch may succeed at input-delivery layer
 → semantic / gameplay assertion about world or UI state AFTER action
 → verification_outcome (conceptual) or VerificationResult (persisted contract)
 → NOT conflated with Layer 1 or Layer 2
```

### Layer 1 — Pre-dispatch refusal

| Evidence | Role |
| --- | --- |
| MC-14 `action_eligibility` → Core-C2 `readiness_class` mapping | `src/run_read.rs` `map_action_eligibility_to_readiness_class` |
| MC-19 / osu wired: `attempted=false`, `refusal_reason` from wiring | `crates/auv-game-minecraft/src/training_result_spatial_query_action_wiring.rs`, `crates/auv-game-osu/src/visual_truth_spatial_query_action_wiring.rs` |
| Minecraft `MismatchRefusal` / `evaluate_mismatch_refusal` | **Pre-dispatch** projection/refusal, not post-action — `crates/auv-game-minecraft/src/verify.rs` |
| candidate-action readiness block | `src/candidate_action_decision.rs` — blocked before `execute()` when readiness fails |

**Discipline:** Layer 1 answers "should we dispatch?" not "did the world change as expected?"

### Layer 2 — Driver / dispatch failure

| Evidence | Role |
| --- | --- |
| MC-19 `dispatch_outcome` derived from trace events | `src/run_read.rs` `derive_dispatch_evidence_from_events` |
| MC-19 executor `Err` when `attempted=true` | wiring outcome + operation-result message |
| `InputActionResult` attempts with `succeeded=false` | `auv_driver::InputActionResult` consumed by candidate-action |
| `activation_only` VerificationResult with `FailureLayer::ControlFailed` | `src/candidate_action_decision.rs` — records input delivery, explicitly **not** semantic success |

**Discipline:** Layer 2 answers "did input delivery / invoke succeed?" A passing click,
AX press, or overlay animation is **not** semantic success without Layer 3 evidence
(see `docs/TERMS_AND_CONCEPTS.md` Action Resolver section).

### Layer 3 — Post-action verification

| Evidence | Role |
| --- | --- |
| `VerificationResult` in `src/contract.rs` | Core persisted assertion shape on `OperationResult.verifications` |
| Minecraft `live_click`: pre/post frame → `evaluate_world_diff` → `VerificationResult` | `src/main.rs` `build_minecraft_world_diff_verification` |
| candidate-action `post_action_verifications` after successful input | `src/candidate_action_decision.rs` `verify_after_execution` |
| MC-19 / osu wired operation-result | **`verifications: Vec::new()`** — Layer 3 intentionally absent |
| `run_read::extract_verifications` | Read-side aggregator from operation-result artifacts |

**Discipline:** Layer 3 answers "given the action ran, is the world/UI in the expected state,
with evidence?" It is recorded **separately** from admission fields per Core-C1.

---

## 2. `verification_outcome` vocabulary candidates (provisional)

**Status: provisional read-side vocabulary only.** Not stabilized in code. No field
named `verification_outcome` exists in persisted artifacts or read-side summaries today
(grep confirms zero code references; only Core-C1/C2-prep docs).

These candidates are **summary labels** for inspect/run_read, not replacements for
`VerificationResult` or vertical verdict structs.

| Candidate | Meaning | When to use (read-side projection) |
| --- | --- | --- |
| `absent` | No post-action verification evidence on the run path | MC-19, osu wired live action (known_limits declare no gameplay verification) |
| `not_attempted` | Action never dispatched; Layer 3 N/A | Layer 1 refusal (`attempted=false`) |
| `activation_only` | Input delivery recorded; no semantic assertion | candidate-action without post_action_probe, or activation-only verification |
| `passed` | At least one verification with `semantic_matched=true` and no contradicting failure_layer | Minecraft live_click world diff success; candidate-action post_action semantic pass |
| `failed` | Verification executed with `semantic_matched=false` or terminal `failure_layer` | World diff `SemanticMismatch` / `StateChangedNoMatch`; AX post-action probe fail |
| `unreliable` | Verification attempted but evidence insufficient | World diff `VerificationUnreliable`; post-action observation errors |
| `inconclusive` | Mixed or partial signals (state_changed but semantic_matched=None) | World diff block removed without expected per `WorldDiffVerdict` |
| `deferred` | Vertical explicitly documents verification as future work | MC-19 D4 known limit; osu wired known limit |

**Mapping notes (provisional):**

- Prefer projecting from existing `VerificationResult.semantic_matched`, `failure_layer`,
 and `method` rather than inventing parallel enums in runtime.
- Vertical quality/witness verdicts (`pass`/`fail`/`blocked` from MC-17 D3 or osu WQ1)
 are **not** automatically Layer 3 — see donor inventory.
- Do **not** pattern-match on `VerificationMethod::Custom { name }` for safety-critical
 decisions (`src/contract.rs` documents this carve-out).

---

## 3. Provenance fields discussion

### Existing pattern: `source_readiness_ref` (Core-C2 D2)

Implemented read-side only in `src/run_read.rs`:

- Format: space-delimited `key=value` pairs (`kind=query_manifest artifact_id=… run_id=…`)
- Resolved for MC-19 / osu wired summaries via `resolve_query_wired_live_action_source_readiness_ref`
- Exposed on `MinecraftQueryWiredLiveActionSummary` and `OsuQueryWiredLiveActionSummary`
- Rendered in `src/inspect.rs` and MC-19 viewer cards

**Role:** lineage pointer to **readiness derivation** provenance, not a new persisted artifact.

### Proposed fields (provisional — **not in codebase**)

| Field | Proposed role | Relationship to existing patterns |
| --- | --- | --- |
| `source_action_ref` | Pointer to the **dispatch attempt** that Layer 3 would verify | Analogous to `source_readiness_ref` but for Layer 2 anchor: likely `kind=operation_result artifact_id=… run_id=…` or `kind=outcome_event event=minecraft.query_wired_live_action.outcome` |
| `verification_source` | Which donor produced Layer 3 evidence | e.g. `kind=operation_result_verification`, `kind=candidate_action_execution`, `kind=world_diff` — mirrors `source_readiness_ref` `kind=` discipline |
| `verification_reason` | Human-readable summary for inspect | Project from `VerificationResult.observed_label`, `failure_layer`, or vertical reason string; **not** a substitute for full `VerificationResult` JSON |

**Design constraints (inherit from Core-C2):**

- Reader-side provenance only unless owner names persisted schema slice
- Preserve donor payload text; do not normalize into fake core taxonomy in D1/D2
- `source_action_ref` should not duplicate `source_readiness_ref`; readiness points
 upstream to query manifest, action ref points to dispatch/operation-result lineage

**Gap (post-D2):** `source_action_ref` remains unimplemented. `verification_outcome`,
`verification_source`, and `verification_reason` are landed on wired live action
summaries only — see D2 handoff.

---

## 4. Donor inventory

| Donor | Layer | Location | Artifact role / surface | Post-action semantic verification? |
| --- | --- | --- | --- | --- |
| **Core contract** `VerificationResult` | L3 shape | `src/contract.rs` | `OperationResult.verifications[]`, legacy `OperationOutput::Verification` | Defines assertion schema |
| **Read-side extractor** | L3 read | `src/run_read.rs` `extract_verifications` / `list_verifications` | Aggregates all operation-result verifications | Aggregator only |
| **Inspect verification panel** | L3 read | `src/inspect.rs` `Verifications:` section | Renders method, executed, semantic_matched, failure_layer | Display only |
| **MC-19 query wired live action** | L1/L2 wiring | `src/minecraft_query_live_action.rs`, `crates/auv-game-minecraft/src/training_result_spatial_query_action_wiring.rs` | `operation-result` role; outcome events | **No** — `verifications: Vec::new()`; `MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` |
| **MC-19 read-side summary** | L1/L2 read | `src/run_read.rs` `MinecraftQueryWiredLiveActionSummary` | Derived inspect/viewer fields | **No** `verification_outcome` field |
| **osu query wired live action** | L1/L2 wiring | `src/osu_query_live_action.rs`, `crates/auv-game-osu/src/visual_truth_spatial_query_action_wiring.rs` | `operation-result` | **No** — `verifications: Vec::new()`; `OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` |
| **osu wired read-side summary** | L1/L2 read | `src/run_read.rs` `OsuQueryWiredLiveActionSummary` | Derived inspect fields | **No** verification field |
| **Minecraft live_click (CLI path)** | L2+L3 | `src/main.rs` | `operation-result` `operation_id=auv.minecraft.live_click` | **Yes** — `evaluate_world_diff` → `VerificationResult` method `SemanticMatch` |
| **Minecraft world diff engine** | L3 vertical | `crates/auv-game-minecraft/src/verify.rs` | `WorldDiffVerdict`, `WorldDiffFailure` | Yes (library); mapped to core in main.rs only |
| **Minecraft mismatch refusal** | L1 pre-dispatch | `crates/auv-game-minecraft/src/verify.rs` `evaluate_mismatch_refusal` | Projection artifacts | **No** — refusal before dispatch |
| **candidate-action execution (archived AX)** | L2+L3 | `src/candidate_action_decision.rs` | `candidate-action-execution` artifact; embedded `operation_result.verifications` | **Yes** — `post_action_verifications`, `verify_after_execution`, `FocusedAxNodeReobserved` probe |
| **candidate-action read-side lineage** | L3 read | `src/run_read.rs` `CandidateActionExecutionLineage` | `verification`, `semantic_matched` fields | Projects from execution artifact |
| **osu detection eval witness (WQ1)** | measurement | `crates/auv-game-osu/src/detection_eval_witness.rs` | `osu-detection-eval-witness.json` | **Explicitly not** action verification (`OSU_WQ1_V1_WITNESS_KNOWN_LIMIT`) |
| **osu detection eval quality (WQ1)** | measurement | `crates/auv-game-osu/src/detection_eval_quality.rs` | `osu-detection-eval-quality.json` | **Explicitly not** gameplay success (`OSU_WQ1_V1_QUALITY_KNOWN_LIMIT`) |
| **MC-17 holdout render quality** | quality metrics | `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs` | `minecraft-3dgs-holdout-render-quality.json` | **No** — photometric evidence, not post-action |
| **MC-17 D3 quality verdict** | derived read | `src/run_read.rs` quality verdict summaries | Derived from MC-12/16/17 evidence | **No** — quality gate, not action verification |
| **window.verifyText** | invoke stub | `crates/auv-cli-invoke/src/commands/window.rs` | N/A | **Stub** — returns error; no `VerificationResult` emission |
| **session.verify_with_result** | runtime session | `src/session.rs` | Session verification resource | **Unknown depth** — typed API exists; not wired into MC-19/osu paths reviewed |

### Investigation line notes

**MC-19:** After dispatch, operation-result carries wiring message and `known_limits`
but **zero** `VerificationResult` entries. Inspect MC-19 section shows admission/dispatch
fields only (`attempted`, `dispatch_outcome`, `readiness_class`, `source_readiness_ref`).
Explicit verification evidence: **absent by design**.

**Minecraft:** Post-action verification exists on the **separate** `auv.minecraft.live_click`
path in `src/main.rs`, not on MC-19 wired path. World diff + inventory delta logic lives
in `crates/auv-game-minecraft/src/verify.rs` as vertical library; only main.rs maps it
into core `VerificationResult` today.

**osu:** Post-action **semantic** verification does **not** exist on wired live action.
Witness/quality chain is detection measurement with explicit known_limits disclaimers.

**Native UI / AX:** Archived copilot path (`candidate_action_decision`) has the richest
Layer 3 implementation: activation-only (Layer 2 boundary marker) plus optional
`post_action_verifications` with AX re-observation probes. `candidate_promotion` itself
does not emit verifications (grep: no matches). Status: **archived vertical donor**,
not active product lane per `AGENTS.md`.

**Read-side:** `run_read.rs` exposes:

- Generic `list_verifications` / `extract_verifications` (Layer 3 from operation-results)
- Wired live action summaries: readiness + dispatch only (Core-C2 fields)
- candidate-action execution lineage: `verification`, `semantic_matched`
- No unified cross-vertical `verification_outcome` summary yet

---

## 5. Non-goals

This D1 review and any D2 slice it enables explicitly **exclude**:

| Excluded | Rationale |
| --- | --- |
| Planner / skill controller | Core-C1/C2 non-goals; no orchestration |
| Action lease | Owner-deferred (Core-A7 pause) |
| Generic verifier trait | No extraction pressure; vertical-local executors remain |
| Runtime / schema changes | No `OperationResult` field additions for `verification_outcome` |
| New persisted artifact roles | Layer 3 stays on existing `VerificationResult` + operation-result |
| Backfilling MC-19/osu wired with gameplay verification | Requires separate owner-named vertical slice |
| MC-20 / controller wiring | Out of active roadmap pause |
| Normalizing donor refusal/verification text into forced core enums | Core-C2-prep stance preserved |

---

## 6. Conclusion

### Should D2 be read-side only?

**Yes.** Evidence supports the same posture as Core-C2:

- Layer 3 evidence already persists (where it exists) as `VerificationResult` on
 `operation-result` — no schema gap for the core contract itself.
- The **gap** is reader vocabulary: wired live action summaries lack any
 `verification_outcome` projection; cross-vertical inspect has no unified Layer 3
 slot adjacent to Core-C1 admission fields.
- MC-19/osu intentionally document absence via `known_limits`; D2 should project
 `verification_outcome=absent` honestly rather than imply verification from dispatch success.

**Minimum honest D2 scope:**

1. Read-side `verification_outcome` projection on wired live action summaries (and
 optionally candidate-action / live_click lineage)
2. Optional provenance formatters: `source_action_ref`, `verification_source`,
 `verification_reason` (reader-only, following `source_readiness_ref` pattern)
3. Inspect / viewer rendering of Layer 3 adjacent to existing admission fields
4. Preserve donor fields and `VerificationResult` detail panel unchanged

### Core vocabulary vs vertical donor semantics

| Concept | Verdict |
| --- | --- |
| `VerificationResult` | **Core vocabulary** — stable contract in `src/contract.rs`; producers/consumers should use this for persisted Layer 3 claims |
| `verification_outcome` | **Core read-side summary slot** — provisional neutral labels (`absent`, `passed`, `failed`, …); must not fork a parallel persisted schema |
| `WorldDiffVerdict`, osu witness status, MC-17 quality verdict | **Vertical donor semantics** — remain in owning crates; D2 maps to summary labels only |
| `activation_only` | **Boundary marker** between Layer 2 and Layer 3 — not semantic verification |

**Do not** promote vertical verdict enums into core runtime in D2. **Do** document
the projection table from donor → `verification_outcome` summary.

### Intentional deferrals (reopen triggers)

| Deferral | Trigger to reopen |
| --- | --- |
| MC-19 gameplay verification | Owner names MC-19+ verification slice; likely wires `live_click` world diff into wired path |
| Persisted `verification_outcome` field on OperationResult | Concrete repetition pain + owner approves schema slice |
| Generic verifier trait / shared crate | Core-B or controller reopen — currently paused |
| `window.verifyText` structured emission | Owner names invoke verification slice |

### Open questions for owner

1. MC-19 是否应接入 `live_click` 式 world diff，还是保持 wiring-only 并只在 read-side 标 `verification_outcome=absent`？
2. `source_action_ref` 是否指向 `operation-result` artifact，还是 wired live action outcome event？
3. archived AX copilot 的 verification 是否纳入 Core-C3 D2，还是标记为 archived donor only？

---

## Related references

- Core-C1 design: [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-core-action-attempt-admission-design.md)
- Core-C2 prep: [`2026-06-30-auv-core-c2-prep-admission-dispatch-read-side-vocabulary-alignment.md`](2026-06-30-core-prep-admission-dispatch-read-side-vocabulary-alignment.md)
- Core-C3 D2 handoff: [`2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md`](2026-06-30-core-verification-outcome-read-side-projection.md)
- MC-19 wiring: [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](../apps/minecraft/2026-06-27-minecraft-probe-19-reference.md)
- MC-19 live closure: [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`](../apps/minecraft/2026-06-27-minecraft-probe-19-reference.md)
- osu wired closure: [`2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md`](../apps/osu/2026-06-28-osu-visual-truth-query-wired-live-action-live-closure.md)
- Core lane roadmap: [`2026-06-13-auv-core-lane-roadmap.md`](2026-06-13-core-roadmap.md)
- Terms: [`docs/TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md)

## One-sentence summary

Core-C3 D1 mapped Layer 3 boundaries and donors; **D2 landed** read-side
`verification_outcome` projection on MC-19 / osu wired summaries (see D2 handoff).
**Core-C3 is paused** after D2.
