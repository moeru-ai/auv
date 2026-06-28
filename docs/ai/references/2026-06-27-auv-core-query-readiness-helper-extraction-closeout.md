# 2026-06-27 AUV Core query readiness helper extraction — post-extraction closeout

Date: 2026-06-27

Status: post-extraction hardening review complete. **Audit PASS.** No code logic
changes required.

Branch: `feat/osu-second-vertical-consumption-probe` @ `819e13c` (helper
extraction) + this closeout note.

Related:

- [`2026-06-27-auv-core-query-readiness-helper-extraction.md`](2026-06-27-auv-core-query-readiness-helper-extraction.md)
- [`2026-06-27-auv-core-a-query-readiness-graduation-review.md`](2026-06-27-auv-core-a-query-readiness-graduation-review.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

## 1. Public API scope

`crates/auv-query-readiness/src/lib.rs` exports **only**:

| Symbol | Role |
| --- | --- |
| `DerivedActionEligibility` | `NotConsumable | AnswerNonClickable | ClickReady` + `as_str` |
| `DerivedActionReadiness` | `{ eligibility, refusal_reason: Option<String> }` |
| `DerivedActionReadiness::{not_consumable, answer_non_clickable, click_ready}` | Constructors |
| `format_query_not_consumable_refusal` | Shared `status=… reason=…` formatter |

**Absent from helper (confirmed):** donor enums (`TrainingResult*`,
`VisualTruth*`), manifest types, traits, serde, geometry/point fields,
`derive_*`, dispatch/read-side imports, game-crate dependencies.

## 2. Adapter thinness

| Check | Minecraft (`training_result_spatial_query_action.rs`) | osu (`visual_truth_spatial_query_action.rs`) |
| --- | --- | --- |
| Type alias only for eligibility | `TrainingResultSpatialQueryActionEligibility` → `DerivedActionEligibility` | `VisualTruthSpatialQueryActionEligibility` → `DerivedActionEligibility` |
| Vertical point field stays local | `window_point: Option<WindowPoint>` on donor readiness struct | `pixel_point: Option<(f32, f32)>` on donor readiness struct |
| Manifest branching stays local | `status` / `visibility` / `projected_window_point` | `status` / `pixel_visibility` / capture bounds |
| Helper use limited to constructors + not-consumable formatter | yes | yes |
| Answer-non-clickable wording stays local | `answer_non_clickable_refusal_reason` + visibility labels | `pixel_visibility=…`, `inside_capture missing_pixel_point`, etc. |

No shared `derive_*` moved into core. No helper imports of game crates.

## 3. Dependency direction

```text
auv-query-readiness
  ├── auv-game-minecraft
  └── auv-game-osu
        └── auv-cli (run_read / inspect — vertical derive_* only)
```

Grep confirms `auv-query-readiness` appears only in:

- root `Cargo.toml` workspace members
- `crates/auv-game-minecraft/Cargo.toml`
- `crates/auv-game-osu/Cargo.toml`
- `Cargo.lock` (transitive resolution)

**Not** in root `[dependencies]` for `auv-cli`. `src/run_read.rs` and
`src/inspect.rs` have **no** `use auv_query_readiness`.

## 4. Three-doc alignment

| Document | Role | Stance after closeout |
| --- | --- | --- |
| [graduation review](2026-06-27-auv-core-a-query-readiness-graduation-review.md) | Admissibility language; default defer | Helper-only admissible **in review language** for query status triad + action readiness view; main matrix rows stay `candidate, not admissible yet`; **no** Core-B starter |
| [proof matrix](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md) | Main verdict authority | Probe-local recurrence **does not** lift extraction pressure; rows 66/68 verdict unchanged (`candidate, not admissible yet`¹) |
| [helper extraction note](2026-06-27-auv-core-query-readiness-helper-extraction.md) | Implementation handoff | "Shared helper extraction, nothing more"; explicitly does not change matrix verdict |

### Owner-override nuance (not a contradiction)

Graduation review §Recommended sequence item 2 listed **"Defer helper
extraction"** as the *default before owner action*. The owner then approved
Step 2 (helper-only slice). This closeout records:

- **One narrow helper landed** (`auv-query-readiness`).
- **Default defer remains** for all other matrix rows (query status triad enum
  graduation, stage status triad, provider comparison, SceneState/registry, etc.)
  and for any further extraction beyond this glue.

## 5. Verdict block

| Criterion | Result | Evidence |
| --- | --- | --- |
| No donor smuggling | **PASS** | Helper crate has zero game/manifest/dispatch symbols |
| No read-side reverse dep | **PASS** | `auv-cli` does not depend on helper; run_read/inspect call vertical `derive_*` only |
| No scope creep narrative | **PASS** | This note explicitly defers query status enum graduation, stage triad, provider compare, SceneState/registry |

**Overall closeout verdict: PASS**

## 6. Explicit non-goals (unchanged)

This branch is **not** Core-B and **not** next extraction pressure. Per proof
matrix L92–98 and helper note L124–131:

- No query status triad enum extraction
- No stage status triad graduation
- No provider comparison helper
- No SceneState / registry / blackboard / arbiter
- No dispatch or live-click consumption wiring
- Proof-matrix row verdicts unchanged

**Next after merge:** owner chooses falsifier-oriented review — **not** another
extraction slice.

## 7. Validation rerun (2026-06-28)

All commands run from repo root on `feat/osu-second-vertical-consumption-probe`:

| Command | Result |
| --- | --- |
| `cargo fmt --check` | pass (exit 0) |
| `cargo check` | pass (exit 0; pre-existing deprecation/dead_code warnings only) |
| `cargo test -p auv-query-readiness` | pass — 3 tests |
| `cargo test -p auv-game-minecraft training_result_spatial_query_action` | pass — 11 tests |
| `cargo test -p auv-game-osu visual_truth_spatial_query_action` | pass — 3 tests |
| `cargo test -p auv-cli osu_visual_truth` | pass — 3 tests |
| `git diff --check` | pass (exit 0) |

## 8. Merge prep checklist

No open PR for `feat/osu-second-vertical-consumption-probe` at closeout time.
When opening or merging:

- [ ] Validation green (§7 above)
- [ ] Closeout note linked from helper extraction note
- [ ] PR framing: **osu second-vertical probe + one bounded helper extraction**; explicitly **not** Core-B
- [ ] Proof-matrix verdict unchanged (`candidate, not admissible yet` for rows 66/68)
- [ ] Graduation review recommended-sequence language unchanged
- [ ] Post-merge: owner chooses falsifier review — not another extraction slice
