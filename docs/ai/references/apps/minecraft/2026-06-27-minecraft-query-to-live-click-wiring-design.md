# Minecraft MC-19: Query-to-live-click minimal wiring design

Date: 2026-06-27

Status: **D4 live closure recorded; D5 inspect polish closed**.
MC-19 D1 adds a runtime wiring seam with injectable executor,
readiness-gated dispatch/refusal core, and unit-tested three-path
attempt/refusal semantics in
`training_result_spatial_query_action_wiring.rs`. MC-19 D3 adds library-only
run recording via `run_minecraft_query_wired_live_action` in `src/minecraft.rs`,
staging existing `operation-result` artifacts plus MC-12 query lineage. D4
live closure is recorded in `2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`.

## One-line summary

MC-19 closes **one narrow wiring evidence chain**: from existing MC-12 query +
MC-14 derived action readiness to **one honest, recorded, refusable live click
attempt** — without introducing a new provider, Core-B runtime, or generic
controller platform.

## What MC-19 is not

| Not this | Why |
| --- | --- |
| New spatial-query provider | MC-15 / MC-18 already closed provider seams |
| Core-B runtime | No registry, blackboard, arbiter, action lease, scene-state platform |
| MC-16 / MC-17 hot path | Holdout witness / render quality stay offline evidence in v1 |
| Archived `candidate-action` product expansion | No TextEdit-style vertical polish or new action classes |
| Gameplay planner / multi-action orchestration | v1 is **click only**, single attempt |

## Core question

When MC-14 says a query answer is **`click_ready`**, can AUV initiate **one**
controlled live click attempt that is:

- honest about refusal when not ready
- recorded in the existing run-store / operation-result model
- inspectable with query + readiness lineage intact

When MC-14 says **`answer_non_clickable`** or **`not_consumable`**, can AUV
**refuse without dispatch** and preserve upstream semantics (no fake partial
success)?

## Upstream inputs (reuse only)

```text
MC-10 semantic manifest (lineage)
        │
        ▼
MC-12 spatial query manifest + inspect   (persisted query truth)
        │
        ▼ derive_action_readiness(manifest)     [MC-14 — already live-closed]
MC-14 action-readiness view               (derived only — still not a new artifact role)
        │
        ▼ MC-19 v1 wiring boundary
Live click attempt OR explicit refusal
        │
        ▼
operation result / run record with lineage
```

**Provider sources:** MC-15 checkpoint-native and MC-18 closed-scene-toy are
valid upstream query backends only. MC-19 does not add or change provider
behavior.

**Out of hot path for v1:** MC-16 holdout preview witness, MC-17 render quality
measurement. They may exist in the same repo/run store but must **not** gate
action admission in MC-19 v1.

## MC-19 v1 scope

### Single action class

- **Click only** — reuse existing window-targeted input delivery
  (`input.clickWindowPoint` / macOS driver path already used by
  `minecraft live-click`).

### Three eligibility paths (mirror MC-14)

| MC-14 `action_eligibility` | MC-19 v1 behavior |
| --- | --- |
| `click_ready` | Perform **one** live click attempt at derived `window_point` |
| `answer_non_clickable` | **Do not dispatch**; record explicit refusal with MC-14 `refusal_reason` |
| `not_consumable` | **Do not dispatch**; record refusal preserving MC-12 `status` / `reason` lineage |

### Minimum execution record (conceptual fields)

MC-19 v1 must make the following inspectable on the run, without inventing a
parallel action-result schema beside existing AUV operation / trace surfaces:

- whether an action was attempted (`attempted: true | false`)
- why executed or why refused (MC-14 refusal or upstream MC-12 reason — no
  re-labeling)
- lineage pointers: query manifest path or artifact ref, readiness summary
  fields (`action_eligibility`, `window_point` when present)
- execution outcome when attempted: driver `OperationResult` / existing
  `minecraft live-click`-style operation result artifact, or structured refusal
  before dispatch

Prefer **extending or wrapping** existing `auv.minecraft.live_click` /
`OperationResult` recording rather than a Minecraft-only action platform.

## Reuse targets (implement slice should anchor here)

| Area | Existing surface | MC-19 use |
| --- | --- | --- |
| Readiness derivation | `derive_action_readiness` in `training_result_spatial_query_action.rs` | Gate before dispatch |
| Live click execution | `run_minecraft_live_click` in `src/main.rs` | Donor for click attempt + operation result persistence |
| Input delivery | `input.clickWindowPoint` via invoke registry | Same backend as current live-click |
| Run recording | `runtime.run_recorded_operation`, trace artifacts | One run per wired attempt |
| MC-14 inspect | `MC-14 Training Result Spatial Query Action Readiness:` in `src/inspect.rs` | Pre/post lineage; no fourth artifact role |

## Acceptance evidence (three live paths)

Validation focus: **wiring honesty and record completeness**, not gameplay
success or projection accuracy.

### 1. Executable — `click_ready`

- Upstream: MC-12 answered + visible + MC-14 `click_ready`
- Outcome: one real action attempt recorded
- Evidence: run id, operation result (or equivalent), query/readiness lineage
  fields present in inspect or terminal summary

### 2. Answered but not clickable — `answer_non_clickable`

- Upstream: e.g. MC-12 answered + `outside_window` (MC-14 live closure run
  `run_1782543551237_21825_0` class)
- Outcome: **no dispatch**
- Evidence: refusal reason matches MC-14 (`visibility=...`), no pretend click
  success

### 3. Not consumable — `not_consumable`

- Upstream: MC-12 blocked/failed (e.g. absent label / absent target class from
  MC-12/MC-18 negative gates)
- Outcome: **no dispatch**
- Evidence: preserves MC-12 `status` / `reason`; MC-19 does not invent a new
  failure taxonomy

## Explicit non-goals

- Provider registry / trait unification
- Blackboard, arbiter, action lease, generic controller runtime
- Multi-step action plans or gameplay planner
- Quality threshold gate using MC-17 metrics
- New persisted MC-12 / MC-14 schema or fourth query artifact role
- `CandidatePromotion` / archived AX copilot vertical expansion
- Core-B enum graduation or shared action-readiness contract move

## Relationship to MC-14 deferred slice

MC-14 design intentionally stopped at derived read-side consumption and listed
**MC-14+ live-click-from-query** as deferred. **MC-19 is that slice**, named
explicitly so it does not smuggle runtime platform work under “MC-14 cleanup”.

Update boundary when MC-19 implements:

- MC-14 remains **derived-only** (no new persisted readiness artifact)
- MC-19 owns **execution wiring + execution evidence** only

## Suggested implement sub-slices (design-level)

| Slice | Scope | Done when |
| --- | --- | --- |
| **D1** | Runtime wiring seam + executor injection + readiness-gated dispatch/refusal core (`wire_query_manifest_to_action`, `QueryLiveClickExecutor`) | Implemented with three-path unit tests; not live closure |
| **D2** | **Retired** — original phase scope already landed inside D1 | No separate implementation slice remains |
| **D3** | Library run recording + `OperationResult` wiring (**no CLI changes**) | **Implemented** — `run_minecraft_query_wired_live_action` + integration tests; no CLI |
| **D4** | Three live closure gates | **Closed** — live closure note with run ids |
| **D5** | Inspect / terminal consumer polish | **Closed** — `inspect_run` MC-19 section, viewer MC-19 card on `operation-result`, wired-action rows on query manifest |

D1 implementation notes:

- Module: `crates/auv-game-minecraft/src/training_result_spatial_query_action_wiring.rs`
- Thin library helper: `wire_spatial_query_manifest_to_action` in `src/minecraft.rs`
- Phase bookkeeping: the originally written D2 scope (“readiness-gated
  dispatch/refusal core”) is already covered by landed D1 code. Do not
  re-implement that layer under a new slice name.
- Full `run_minecraft_live_click` integration is **deferred**; D3 should stay
  library-only, reuse the `input.clickWindowPoint` invoke pattern, and avoid
  importing the telemetry + screenshot + `assess_bound_projection` pipeline
  from `run_minecraft_live_click`.

D3 implementation notes:

- Library entry: `run_minecraft_query_wired_live_action` / `run_minecraft_query_wired_live_action_with_executor` in `src/minecraft.rs`.
- Shared click helper: `invoke_click_at_window_point` in `src/minecraft_query_live_action.rs` (also used by `run_minecraft_live_click` in `src/main.rs`).
- D4: `input.clickWindowPoint` is implemented in `crates/auv-cli-invoke/src/commands/input.rs` (offset/relative point inputs, window resolve, driver click).
- Known limit: `MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT` (replaces the D3 limit to avoid stacked semantics).
- Live closure: `docs/ai/references/2026-06-27-minecraft-mc19-query-to-live-click-wiring-live-closure.md`.

D5 implementation notes:

- Derived read-side summary: `derive_minecraft_query_wired_live_action_summary` / `list_minecraft_query_wired_live_action_summaries` in `src/run_read.rs`.
- Terminal inspect section: `MC-19 Query Wired Live Action:` in `src/inspect.rs` (`cargo run -- inspect <run-id>`).
- Viewer: MC-19 summary card on `operation-result` JSON preview and `wired_action_*` rows on spatial query manifest in `src/inspect_server_viewer.html`.

Implement must **not** expand D3 into a new CLI surface unless the owner names
that slice explicitly.

## Honest limits

- MC-19 v1 proves **consumption → minimal action attempt**, not semantic
  gameplay success
- Click success in Minecraft is not required for slice closure; honest refusal
  and record completeness are
- MC-16/17 may remain in the same workspace but are not action gates in v1

## Related references

- Core-C1 admission vocabulary (generic boundary; MC-19 proves wiring honesty):
  `docs/ai/references/2026-06-28-auv-core-c1-action-attempt-admission-design.md`
- MC-12 contract:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
- MC-14 action-facing design:
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
- MC-14 live closure (three eligibility classes):
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-live-closure.md`
- Core spatial consumption pattern (action readiness vocabulary):
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
- Admission table (runtime wiring explicitly deferred until owner slice):
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-admission-table.md`
