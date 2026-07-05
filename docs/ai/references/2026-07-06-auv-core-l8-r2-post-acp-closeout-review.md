# AUV Core L8-R2 Post-ACP Closeout Review

**Date:** 2026-07-06  
**Lane:** docs-only hard audit (no default code changes)  
**Status:** closeout complete — **verdict below**

**Prerequisites:** [L8 closeout (R1)](2026-07-05-auv-core-l8-closeout-review.md), [L9 inspect surface](2026-07-05-auv-core-l9-inspect-surface-handoff.md), [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md), [ACP-1](2026-07-05-auv-netease-music-acp-1-handoff.md) / [ACP-2](2026-07-05-auv-netease-music-acp-2-handoff.md) handoffs

**Compat baseline (pinned):** forced-answer table and status/issue mapping in [2026-07-05-auv-core-l8-closeout-review.md](2026-07-05-auv-core-l8-closeout-review.md). R2 judges drift **only** against that document.

---

## 1. Audit baseline

| Item | Value |
|------|-------|
| R2 audit HEAD | `80874bbf` — `feat(inspect): unified netease proof run-detail hint (ACP-2c)` |
| Branch | `cursor/acp-2c-unified-netease-proof-hint` (1 commit ahead of `main`) |
| `main` at audit time | `95af4d62` — `feat(auv-netease-music): add sidebar scan proof invoke` (ACP-2b) |
| L8 R1 evidence commits | `3745c419` docs → `ac4e4e0b` L8b → `9483f2d6` read-side |
| Delta since L8 R1 | L9 viewer ATL panel landed; ACP-1/2 hermetic packs; ACP-2c unified `#netease-proof-hint` |

**Scope:** Re-validate **L8b candidate-action seam** (`decision → driver_result → verification → artifact → inspect`). ACP pack is **orthogonal evidence only**.

---

## 2. Forced-answer matrix

### 2.1 Producer (L8b only)

| Check | Result | Evidence |
|-------|--------|----------|
| Canonical execution artifact coexists `action_resolver_decision` + `input_action_result` | **Pass** | `build_candidate_action_execution_artifact` (`src/candidate_action_decision.rs:500+`) still calls `reconcile_effective_decision` before persist |
| `plan_delivery_mismatch` in `known_limits`, not silent pass | **Pass** | `cargo test -p auv-cli --lib plan_delivery_mismatch` — 3 passed |
| `activation_only` vs semantic verification boundary | **Pass** | No third schema introduced; producer path unchanged since R1 |
| **ACP boundary — `*.rs` code references** | **Pass** | `rg 'action_resolver\|candidate.action' crates/auv-netease-music --glob '*.rs'` → **0 matches** |
| **ACP boundary — non-code** | **Pass (informational)** | `rg ... --glob '!*.rs'` → **0 matches** in crate tree |

### 2.2 Read-model (projection only)

| Check | Result | Evidence |
|-------|--------|----------|
| ATL fields from artifacts, no synthesis | **Pass** | `action_transition_lineage_entry` (`run_read.rs:6185+`); no `InputActionResult::default` / fake success in read path |
| `planned_decision` comparator-only | **Pass** | Join via `read_candidate_action_decision_artifact`; unchanged from [ATL read handoff](2026-07-05-auv-core-action-transition-lineage-read-handoff.md) |
| `GET /runs/{id}` builds ATL via `extract_action_transition_lineage` only | **Pass** | `inspect_server/mod.rs:383-385` |
| inspect_server `*.rs` does not `persist_` run facts on read path | **Pass** | `rg 'persist_' src/inspect_server/ --glob '*.rs'` → 0 on read assembly; `write_updates` is **live ingestion**, not ATL fabrication |
| L9 ATL viewer renders API JSON only | **Pass** | `viewer_renders_action_transition_lineage_hooks` — 1 passed |
| ACP-2c `#netease-proof-hint` | **Pass (non-seam)** | Heuristic on root span + artifact role; does not write run facts or populate ATL; `viewer_renders_netease_proof_hint_hooks` — 1 passed |

### 2.3 Compatibility (vs R1 baseline doc)

| Scenario | Expected (R1) | Result | Evidence |
|----------|---------------|--------|----------|
| Canonical L8b + plan/delivery mismatch | `partial` + `plan_delivery_mismatch` | **Pass** | `action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b` |
| Legacy missing `action_resolver_decision` | `partial` + `missing_action_resolver_decision` | **Pass** | `action_transition_lineage_marks_legacy_missing_decision_as_partial` |
| Legacy missing `input_action_result` only | `partial` + `missing_input_action_result` | **Pass (code)** / **Gap (test)** | `classify_action_transition_lineage` + `legacy_action_transition_lineage_entry` (`run_read.rs:6175-6179`, `6294`); **no isolated unit test** — see drift register |
| Malformed JSON / non-JSON mime | `malformed` + issue | **Pass** | `malformed_action_transition_lineage` path unchanged |

### 2.4 Post-ACP boundary (fourth dimension)

| Question | Result | Notes |
|----------|--------|-------|
| ACP gate misused as L8 re-validation? | **Pass** | Gate requires prior L8 **verdict exists**; ACP completion does not upgrade seam verdict |
| NetEase proof runs imply full action seam closure? | **Documented risk** | Shared RunSpec/artifact roles; viewer hint is UX-only — **not** ATL |
| ACP-2c hint causes seam hallucination? | **Pass (non-seam)** | Labels from span name + artifact role; no decision/driver injection |
| ACP orthogonal green (narrow filter) | **Pass** | `select_proof` 8 passed; `sidebar_scan_proof` 8 passed — **does not count toward seam pass** |

---

## 3. Drift register (relative to L8 R1)

| ID | Kind | Finding | Seam impact |
|----|------|---------|-------------|
| D1 | test coverage | No dedicated regression for legacy **missing `input_action_result` only** (R1 doc names it; only combined legacy fixture tested) | None observed — code path unchanged |
| D2 | docs/viewer | ACP handoffs list L8 closeout as prerequisite; easy to misread pack graduation as seam re-proof | Misread risk only |
| D3 | viewer | `#netease-proof-hint` cannot distinguish invoke proof vs product `playlist ls --store-root` | Known non-seam limit (ACP-2 handoff) |
| D4 | surface | L9 closed R1 viewer gap; ATL panel now renders mismatch/partial/malformed | Positive — no seam regression |

**No producer/read/compat behavior regression** detected at `80874bbf`.

---

## 4. Validation commands (recorded)

Audit HEAD: `80874bbf` on `cursor/acp-2c-unified-netease-proof-hint`.

```sh
git rev-parse HEAD
# 80874bbf6019432a83332e41f977e272cdedb5cb

cargo test -p auv-cli --lib action_transition_lineage -- --nocapture
# 3 passed (includes ATL viewer hook + 2 run_read ATL tests)

cargo test -p auv-cli --lib viewer_renders_action_transition -- --nocapture
# 1 passed

cargo test -p auv-cli --lib viewer_renders_netease_proof_hint_hooks -- --nocapture
# 1 passed

cargo test -p auv-cli --lib inspect -- --nocapture
# 101 passed

cargo test -p auv-cli --lib plan_delivery_mismatch -- --nocapture
# 3 passed

cargo test -p auv-netease-music select_proof -- --nocapture
# 8 passed (orthogonal)

cargo test -p auv-netease-music sidebar_scan_proof -- --nocapture
# 8 passed (orthogonal)

rg -n "action_resolver|candidate.action" crates/auv-netease-music --glob '*.rs'
# 0 matches

rg -n "action_resolver|candidate-action" crates/auv-netease-music --glob '!*.rs'
# 0 matches

rg -n "LocalStore::write|persist_" src/inspect_server/ --glob '*.rs'
# 0 matches on read projection path

rg 'action_resolver_decision\.clone\(\)' src/candidate_action_decision.rs
# 0 matches (auxiliary, same as R1)
```

**Note:** Full-workspace `cargo test` may flake on network-coupled `candidate_action` OpenAI fixture tests (502). R2 seam verdict uses **narrow hermetic filters above** only.

---

## 5. Forced answers (R2)

1. **L8b artifact 主路径是否仍真实共存 `decision + driver_result`?**  
   **Yes.** Producer code and `plan_delivery_mismatch` tests unchanged in substance.

2. **read-side 是否仍只读投影、未偷生产 seam 事实?**  
   **Yes.** ATL assembly is `extract_*` only on `GET /runs`; viewer renders JSON.

3. **old artifact 缺字段是否仍稳定 `partial` / `malformed`（core 层）?**  
   **Yes** for tested paths; `missing_input_action_result`-only lacks isolated test (D1).

4. **ACP-1/2 成功是否证明 core seam 毕业?**  
   **No.** Orthogonal pack lane; zero seam references in NetEase `*.rs`.

5. **相对 L8 R1，seam 是否漂移?**  
   **No behavior drift.** L9 + ACP-2c are surface/pack layers outside L8b producer.

---

## 6. Verdict

### Selected: `close_with_documented_gaps`

**Rationale:** Producer, read-model, and compatibility **core** layers pass at `80874bbf`. No `reopen_*` trigger. Post-ACP **misread risk** remains (D2, D3): a reader can still equate ACP pack success with seam closure despite orthogonal design.

**Not selected:**

| Verdict | Why not |
|---------|---------|
| `close_confirmed` | D2 misread risk is material enough to require named follow-up docs hygiene |
| `reopen_l8b_producer` | No failing producer evidence |
| `reopen_read_projection` | No synthesis or read-path store mutation on seam fields |
| `reopen_compat` | Tested compat paths match R1; D1 is coverage gap not behavior drift |

### Relationship to L8 R1

| R1 verdict | R2 outcome |
|------------|------------|
| `close_for_core_seam_surface_gap_only` | **Superseded for operations** by L9 viewer closure (D4) |
| Core seam producer/read/compat | **Re-confirmed** — still tight post-ACP |
| Operational next step | Address **documented gaps** below; do not reopen L8b without new failing evidence |

---

## 7. Follow-up slices (named only — not implemented in R2)

| Slice | Kind | Trigger |
|-------|------|---------|
| `docs-acp-orthogonality-callout` | docs-only | Owner wants ACP handoffs + gate to carry an explicit “pack pass ≠ seam re-proof” callout block |
| `test-only: action_transition_lineage_legacy_missing_driver` | test-only | Owner wants D1 closed with isolated `missing_input_action_result` fixture |
| `L9-R1 netease-proof-hint-disambiguation` | owner-approved feature | Owner wants viewer to distinguish invoke proof vs product `playlist ls` (needs schema or metadata — out of R2) |

---

## 8. Explicit non-goals

- S / Surface Memory lane
- ACP-3 or second-app packaging
- L8b producer changes without new failing evidence
- Runtime execution semantics changes
- Full `cargo test -p auv-netease-music` as R2 seam evidence (forbidden per audit plan)

---

## 9. Next gate

Proceed on **app packaging** slices only with explicit owner naming and cite **this R2 doc** plus [L8 R1](2026-07-05-auv-core-l8-closeout-review.md) for seam claims — **not** ACP handoffs alone.

**ACP-B (second app):** see [qqmusic ACP-B1 gate/handoff](2026-07-06-auv-qqmusic-acp-b1-gate-handoff.md). Second-app pack pass remains orthogonal to seam re-proof per [ACP gate](2026-07-05-auv-core-app-command-pack-gate.md#orthogonality-callout-mandatory-in-every-acp-handoff).
