# Osu visual truth query wired live action design

Date: 2026-06-28

Status: **depth-C owner slice** — osu **Core-C1 donor proof** (second vertical after
MC-19). Proves admission + one honest dispatch/refusal path; **not** MC-19 parity,
**not** Core-B graduation, **not** proof-matrix verdict change.

## One-line summary

Osu closes one narrow wiring evidence chain from existing visual-truth spatial
query + derived action readiness to **one honest, recorded, refusable live click
attempt** via playfield re-projection and live window mapping — without core
extraction, gameplay verification, or treating capture-space readiness as global
click authority.

## Upstream chain

```text
Visual truth semantic manifest (osu probe semantic gate)
        │
        ▼
Visual truth spatial query manifest + inspect   [persisted query truth]
        │
        ▼ derive_visual_truth_spatial_query_action_readiness   [derived only]
Osu action-readiness view (pixel_point in capture space)
        │
        ▼ this slice — admission wiring
Live playfield → window_point OR explicit refusal
        │
        ▼
operation result / run record with lineage
```

**Fixture inputs:** reuse
`crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe/` (`visual_truth_manifest.json`,
`projection.json`) through semantic validation → spatial query → wired live action.

## Three eligibility paths (mirror MC-19 / MC-14)

| Derived `action_eligibility` | This slice behavior |
| --- | --- |
| `click_ready` | Resolve **playfield** from `VisualTruthFrame` (same `object_index` / `capture_phase` as query); map via **live** `PlayfieldProjection::for_window` → `to_window_point`; perform **one** live click attempt |
| `answer_non_clickable` | **Do not dispatch**; record MC-14-style `refusal_reason` (e.g. `pixel_visibility=outside_capture`) |
| `not_consumable` | **Do not dispatch**; record refusal preserving query `status` / `reason` lineage |

## Core-C1 field mapping

| Core-C1 concept | Osu donor field |
| --- | --- |
| `readiness_class` | `action_eligibility` (`click_ready` / `answer_non_clickable` / `not_consumable`) |
| `attempted` | `VisualTruthQueryActionWiringOutcome.attempted` |
| `action_point` | `pixel_point` (readiness, capture space) + `window_point` (live dispatch) |
| Layer 1 refusal | `not_consumable` / `answer_non_clickable` before dispatch |
| Layer 2 failure | executor `Err` or invoke failure when `attempted=true` (honest overload on outcome message) |
| Layer 3 verification | **Out of scope** — no gameplay / hit verification |

## Coordinate / authority disclaimers

- Readiness `click_ready` remains **capture-space consumability** from persisted
  query (`pixel_x` / `pixel_y` witness). It is **not** window-click authority.
- Live dispatch **must not** click manifest `pixel_x/y` directly. Re-resolve
  `expected_playfield_x/y` from `VisualTruthFrame` and map through **live**
  `PlayfieldProjection` (same path as `run_typed_dispatch` in `benchmark.rs`).
- Inspect and `known_limits` carry both `pixel_point` and `window_point` plus
  `osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification`.

## D1 / D3 / D4 slice table (depth C)

| Slice | Deliverable | Closure boundary |
| --- | --- | --- |
| **D1** | `visual_truth_spatial_query_action_wiring.rs` in `auv-game-osu` | Unit tests: three gates + defensive missing live `window_point` |
| **D3** | `run_osu_query_wired_live_action` in `src/osu.rs`, `osu_query_live_action.rs`, run_read + inspect | Library run recording + read-side summary; integration tests with stub executor |
| **D4** | `examples/osu_query_wired_live_action.rs` + live closure evidence doc | macOS non-stub `input.clickWindowPoint`; honest attempt/refusal only |

## Explicit non-goals

- Core-C2 helper extraction, core trait unification, CLI subcommand
- Gameplay / hit verification (Layer 3)
- Proof-matrix row 66/68 verdict upgrade
- MC-20 planner/controller, provider registry, persisted admission artifact role
- Dual-backend compare (truth vs detection)

Does **not** change Core-A proof-matrix verdicts. See
[`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
for generic admission vocabulary; this slice is vertical donor evidence only.

## Risk watch

| Risk | Mitigation |
| --- | --- |
| `click_ready` label conflation | Inspect shows `pixel_point` vs `window_point`; known_limits + disclaimers |
| Stale pixel used for live click | Dispatch uses live playfield projection, not manifest pixels |
| Layer 2 outcome overload | Record honestly in outcome event; defer field split to Core-C2+ |

## Platform note

`#[cfg(target_os = "macos")]` for real window projection and
`InvokeWindowPointClickExecutor` in the default library path. Non-macOS builds
use stub executor + capture-derived projection in unit/integration tests only.

## Cross-links

- Osu second-vertical probe:
  [`2026-06-27-auv-second-vertical-consumption-probe-osu-design.md`](2026-06-27-auv-second-vertical-consumption-probe-osu-design.md)
- OSU-WQ1 witness + quality:
  [`2026-06-28-osu-wq1-witness-quality-evidence-design.md`](2026-06-28-osu-wq1-witness-quality-evidence-design.md)
- Core-C1 admission design:
  [`2026-06-28-auv-core-c1-action-attempt-admission-design.md`](2026-06-28-auv-core-c1-action-attempt-admission-design.md)
- MC-19 wiring template:
  [`2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md`](2026-06-27-minecraft-mc19-query-to-live-click-wiring-design.md)
