# 2026-06-27 osu! second-vertical consumption probe design

Date: 2026-06-27

Status: implemented probe slice (MC-14-analog derived consumption only;
query→readiness shape recurrence, not full MC-14 parity). No core extraction,
no witness/quality/live-click wiring.

## Classification

`substrate research` — owner-approved second vertical for Core-A proof-matrix
evidence without graduating types into core.

## Why osu!

| Requirement | osu! evidence |
| --- | --- |
| Non-Minecraft producer artifacts | `visual_truth_manifest.json`, `projection.json` from `osu-benchmark` / `osu-eval-detections` |
| Semantic → query without MC donor | Playfield truth + `EvalProjection::PlayfieldToPixels` in `visual_eval.rs`; gate in `projection.rs` |
| Positive + negative paths | Frozen fixture `crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe/`; detection eval dir for spatial scoring |
| Derived readiness | Pixel-space answer from projection; `click_ready` vs `answer_non_clickable` without live dispatch |
| Avoid witness/quality/live click | Eval report is out of scope; no window click wiring in v1 |

**Falsifiers:** vertical without persisted manifests; projection that cannot gate
honestly; readiness that requires live dispatch to demonstrate.

## Target chain (MC-14-analog at derived shape only)

```text
benchmark artifacts (visual_truth + projection)
  → semantic gate (ready / blocked / failed)
  → spatial query (answered / blocked / failed)
  → read-side inspect (run_read + inspect text)
  → derived action readiness (click_ready / answer_non_clickable / not_consumable)
```

This is **not** full MC-14 parity. MC-14 readiness is closer to window action
reachability (`visible` / `outside_window`, projected window point). osu probe
readiness is **capture-space consumability** only: benchmark capture pixels
inside/outside bounds. The shared part is a **derived-only three-class readiness
view** over persisted query truth, not dispatch-safe or authority-bearing
readiness.

## Artifact roles

| Role | File | Persisted |
| --- | --- | --- |
| `osu-visual-truth-semantic` | `osu-visual-truth-semantic.json` | yes |
| `osu-visual-truth-semantic-inspect` | `osu-visual-truth-semantic-inspect.json` | yes |
| `osu-visual-truth-spatial-query` | `osu-visual-truth-spatial-query.json` | yes |
| `osu-visual-truth-spatial-query-inspect` | `osu-visual-truth-spatial-query-inspect.json` | yes |
| action readiness | _(derived only)_ | no |

## Status models (osu-local)

### Semantic gate (`VisualTruthSemanticStatus`)

- `ready` — manifest + projection parse; `to_eval_projection()` succeeds; at least one frame
- `blocked` — missing manifest/projection path, empty frames, symlink paths
- `failed` — corrupt JSON, non-finite projection calibration

### Spatial query (`VisualTruthSpatialQueryStatus`)

- `answered` — semantic ready; target frame found; projection applied (includes outside-capture answers)
- `blocked` — semantic not `ready`
- `failed` — target absent, projection unavailable at query time

Distinct from semantic `ready`: query `answered` means a target-conditioned pixel answer was produced.

### Derived action readiness (`VisualTruthSpatialQueryActionEligibility`)

- `click_ready` — query `answered`, pixel inside capture bounds (**capture-space
  consumability label; not window-click authority**)
- `answer_non_clickable` — query `answered` but outside bounds or missing pixel witness
- `not_consumable` — query not `answered`

**Known limit:** coordinates are source-image pixels from benchmark capture, not
window-click authority (same honesty as `osu_detection_session_provider`). Do not
treat `click_ready` as dispatch-safe readiness evidence.

## Lineage fields

Semantic manifest carries:

- `source_run_artifact_dir`
- `source_visual_truth_manifest_path`
- `source_projection_path`
- `frame_count`, `beatmap_path`
- `semantic_status`, `semantic_reason`

Spatial query manifest carries semantic manifest path plus query target
(`object_index`, `capture_phase`, optional `object_kind`) and
`query_backend = playfield_projection_reference` (v1 single backend).

## Fixture matrix

| Case | Fixture | Expected |
| --- | --- | --- |
| Positive semantic | `tests/fixtures/osu_visual_truth_probe/` | `ready` |
| Negative semantic (unit) | temp dir missing projection | `blocked` |
| Positive query | semantic `ready` + target object 0 `before_dispatch` | `answered`, `inside_capture` |
| Negative query semantic | semantic `blocked` fixture | query `blocked` |
| Negative query target | valid semantic, unknown object index | `failed` |
| Outside capture | answered pixel outside bounds (unit) | `answer_non_clickable` |

Frozen positive root replaces fragile `.tmp-osu-dispatch-p4ab-closeout` when absent.

## MC pattern mapping (donor recurrence, not independent ontology)

This table records **donor-chain recurrence** along the Minecraft MC-10→MC-14
line. It is evidence that a second vertical can walk a similar consumption
shape; it is **not** proof of an independent second-vertical ontology or of
cross-vertical abstraction maturity.

| Core-A stage | Minecraft donor | osu probe |
| --- | --- | --- |
| Semantic gate | MC-10 `TrainingResultSemanticStatus` | `VisualTruthSemanticStatus` |
| Spatial query | MC-12 `TrainingResultSpatialQueryStatus` | `VisualTruthSpatialQueryStatus` |
| Read-side inspect | MC-13 run_read + inspect | `OsuVisualTruth*` extract/list |
| Action readiness | MC-14 `derive_action_readiness` | `derive_osu_visual_truth_spatial_query_action_readiness` |

## Explicit non-goals (v1)

**Footnote (2026-06-28):** live admission/dispatch opened as a **separate owner slice** (`2026-06-28-osu-visual-truth-query-wired-live-action-design.md`); probe v1 historical scope below is unchanged.


- Core enum/helper extraction
- MC-20 planner/controller
- Witness (MC-16), quality (MC-17), live click (MC-19)
- Dual-backend compare (truth vs detection) — deferred osu slice
- Provider registry / blackboard / arbiter
- Cross-vertical inspect viewer card unification

## Live closure

Recorded runs stage artifacts under `.tmp/` run store via
`run_osu_visual_truth_semantic_validation` and `run_osu_visual_truth_spatial_query`
in `src/osu.rs`. Inspect text sections:

- `Osu Visual Truth Semantic:`
- `Osu Visual Truth Spatial Query:`
- `Osu Visual Truth Spatial Query Action Readiness:`

Proof-matrix row assessment: see companion
`docs/ai/references/apps/osu/2026-06-27-osu-consumption-probe-evidence.md`.
