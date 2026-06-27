# MC-18: Closed-scene toy query provider seam

Date: 2026-06-27

## Verdict boundary

MC-18 opens the **second MC-12 query provider seam**, parallel to MC-15
`checkpoint_native`. It adds an in-repo **closed-scene / closed-label toy provider**
that answers the design question:

> Without `projection_reference` (`scene_packet + MinecraftProjector`), can we still
> produce the same MC-12 `TrainingResultSpatialQueryAnswer` wire shape using only
> bounded, pre-agreed scene knowledge?

MC-18 is **not**:

- real Gaussian inference or splat-quality judgment
- open-world / open-vocab / generalization claims
- action authority (no readiness changes, action lease, `CandidatePromotion`, or
  `ActionResolver` wiring — MC-14 remains a separate consumer-only slice)
- Core-B extraction (no provider registry, blackboard, arbiter, or generic scene-state
  runtime; see explicit deferral below)
- a new artifact role, persisted JSON file type, or second query-result schema
- holdout / witness / quality mixing (**MC-16** / **MC-17**)

v1 honest behavior: validate bounded closed-scene inputs, apply dumb closed-world
rules (fixture lookup, label lookup, limited projection approximation), emit the
existing MC-12 answer shape, and record an honest toy `basis_frame_id` that does
**not** masquerade as render or checkpoint evidence.

## Core question and seam placement

```text
MC-10 semantic manifest
        │
        ▼
MC-12 spatial query producer
        │
        ├── projection_reference  (always when semantic ready — unchanged)
        │
        └── provider seam (mutually exclusive, one at a time)
                ├── MC-15 checkpoint_native   (--query-provider checkpoint-native)
                └── MC-18 closed_scene_toy    (--query-provider closed-scene-toy)
```

MC-15 and MC-18 are **sibling provider adapters** in the same module seam. They
differ in what they claim to read and whether they may delegate projection math to
`projection_reference`:

| Seam | Primary read surface | Projection path | Honest basis witness |
| --- | --- | --- | --- |
| MC-15 `checkpoint_native` | normalized result (`config.yml`, checkpoints) | reuses `run_projection_reference_backend` math | `checkpoint:<relative_path>` |
| MC-18 `closed_scene_toy` | bounded closed-scene fixture / label table | **no** `MinecraftProjector` reference path | `closed_scene_toy:<fixture_id>:<frame_id>` |

MC-18 exists to prove **contract stability and honest degradation** when the answer
comes from controlled closed-world knowledge alone — not accuracy against a trained
splat.

## Relationship to MC-12 / MC-13 / MC-14 / MC-15 / MC-16 / MC-17

| Slice | Relationship to MC-18 |
| --- | --- |
| **MC-12** | Parent contract. MC-18 emits `TrainingResultSpatialQueryAnswer` only; manifest + inspect dual-backend fields unchanged. |
| **MC-13** | Read-side inspect consumer unchanged. MC-18 does not add inspect surfaces. |
| **MC-14** | Action-readiness consumer unchanged. MC-18 does **not** extend MC-14 and must not flip readiness or imply click authority. |
| **MC-15** | Sibling provider seam. Same selection / compare rules; different provider internals and honest limits. |
| **MC-16** | Holdout preview witness — out of scope. MC-18 must not consume or produce holdout manifests. |
| **MC-17** | Holdout render quality — out of scope. No photometric metrics or quality verdict mixing. |

## Closed-scene / closed-label semantics

### Closed scene

- **Bounded scope**: finite set of blocks, frames, and camera poses agreed before the
  run (fixture manifest or embedded fixture table keyed by `source_scene_packet_manifest_path`
  hash / run id).
- **Controlled inputs**: provider reads only the bounded fixture + MC-10 lineage pointers;
  no ad-hoc filesystem discovery beyond declared paths.
- **No open-world claims**: answers outside the fixture envelope must degrade to
  `blocked` or `failed` with explicit reasons — never silent extrapolation.

### Closed label

- **Finite pre-agreed label set**: e.g. block positions or semantic tags enumerated in
  the fixture (`labels: [{ id, block, face?, semantics }]`).
- **Lookup, not understanding**: matching `target_block` / `target_face` /
  `target_semantics` against the table is **closed-label lookup**, not model
  comprehension. `message` and `known_limits` must say so plainly.
- **No open-vocab**: unknown targets → `blocked` with
  `reason = target_block_absent_from_scene_packet` (reuse existing MC-12 reason when
  semantically correct) or a provider-local message clarifying "not in closed label
  set" without inventing a new reason enum in D1 unless owner approves.

### Toy projection approximation

Allowed v1 internals (pick the smallest subset that closes evidence):

1. **Fixture screen-point table** — precomputed `(label_id → screen_point, visibility)`
   for each basis frame.
2. **Limited analytic stub** — e.g. fixed viewport + pose from fixture, naive pinhole
   without full `MinecraftProjector` pipeline, only when the stub is documented in
   fixture metadata and `known_limits`.

Forbidden:

- Calling `run_projection_reference_backend` or `MinecraftProjector` as the provider
  answer path (that would collapse MC-18 into MC-15-style reference delegation).
- Emitting `basis_frame_id` values that look like `checkpoint:*` or scene-packet frame
  paths unless the fixture explicitly records that witness and `known_limits` states it
  is fixture metadata, not live render evidence.

## Adapter contract (future implementation)

Module (natural landing): `crates/auv-game-minecraft/src/training_result_spatial_query_provider.rs`

Proposed types (names provisional until implementation review):

- `ClosedSceneToyProviderInputs` — derived from `TrainingResultSpatialQueryRequest` +
  MC-10 semantic manifest + resolved fixture handle
- `ClosedSceneToyProviderOutcome` — maps to `TrainingResultSpatialQueryAnswer`
- `run_closed_scene_toy_provider_backend` — in-repo provider entrypoint parallel to
  `run_checkpoint_native_provider_backend`

Backend enum extension in `training_result_spatial_query.rs`:

- `closed_scene_toy` (`TrainingResultSpatialQueryBackend::ClosedSceneToy`)

Selection, `comparison_verdict`, and inspect dual-backend fields remain **MC-12
unchanged**:

- provider `answered` > reference `answered` > most specific `blocked` / `failed`

## Wire contract

### Input lineage (unchanged)

MC-18 consumes the same MC-12 entry as MC-15:

```sh
auv-cli minecraft query-3dgs-training-result \
  --training-result-semantic-manifest <minecraft-3dgs-training-result-semantic.json> \
  --target-block <x,y,z> \
  [--target-face <up|down|north|south|east|west>] \
  [--target-semantics hit_face_center|block_center] \
  [--query-provider closed-scene-toy] \
  [--closed-scene-fixture <path>] \
  --output-dir <dir>
```

Input boundary:

- MC-10 semantic manifest is authoritative for lineage and `semantic_status` gate.
- Optional `--closed-scene-fixture` points at a bounded fixture JSON; if omitted, v1
  may use a built-in test fixture only in unit tests (live closure must pass explicit
  fixture path — owner decision at implementation).

`--query-provider closed-scene-toy` and `--query-command` remain **mutually exclusive**.
`--query-provider` values `checkpoint-native` and `closed-scene-toy` are **mutually
exclusive** (one provider backend per invocation).

### Output shape (MC-12 exact)

Provider returns `TrainingResultSpatialQueryAnswer` only:

| Field | MC-18 policy |
| --- | --- |
| `status` | `answered` \| `blocked` \| `failed` |
| `reason` | Reuse MC-12 reasons where applicable; defer new enum variants unless a gap is proven in implementation |
| `message` | Human-readable; must state closed-scene toy path, not inference |
| `visibility` | Honest `ProjectionVisibility` from fixture / stub |
| `screen_point` | Present when `answered` and visibility allows coordinates; absent when honestly unknown |
| `match_radius_px` | Optional; fixture-supplied or omitted in v1 |
| `confidence` | Omitted or fixture-static in v1; **not** a model confidence score |
| `basis_frame_id` | **Required** when `answered`; see convention below |

Manifest / inspect `known_limits` must include at minimum:

```text
MC-18 v1 closed-scene toy provider answers from bounded fixture/label lookup only;
closed-scene and closed-label only; not Gaussian inference; not action authority
```

Plus provider-specific limit constant, e.g.:

```text
MC-18 v1 closed-scene toy provider does not use projection_reference or
MinecraftProjector; answers are fixture-derived closed-world lookup only
```

### `basis_frame_id` convention

Prefix distinguishes toy witness from MC-15 checkpoint witness:

```text
closed_scene_toy:<fixture_id>:<basis_frame_id>
```

Examples:

- `closed_scene_toy:mc18-smoke-v1:frame-0003`
- `closed_scene_toy:label-table-alpha:pose-north-door`

Rules:

- `fixture_id` — stable id from fixture manifest `fixture_id` field
- `basis_frame_id` — frame or pose row within that fixture used for the answer
- Never use bare `checkpoint:*` or scene-packet relative paths unless paired with
  `closed_scene_toy:` prefix and explained in `known_limits`

### Proposed fixture schema (D2, synthetic)

```json
{
  "schema_version": 1,
  "fixture_id": "mc18-smoke-v1",
  "generated_at": "2026-06-27T00:00:00Z",
  "labels": [
    {
      "id": "door-north",
      "block": { "x": 511, "y": 73, "z": 728 },
      "face": "north",
      "semantics": "hit_face_center"
    }
  ],
  "frames": [
    {
      "basis_frame_id": "frame-0003",
      "answers": [
        {
          "label_id": "door-north",
          "visibility": "visible",
          "screen_point": { "x": 640.0, "y": 360.0 }
        }
      ]
    }
  ]
}
```

## Future implementation sub-slices (design only)

| Slice | Scope | Done when |
| --- | --- | --- |
| **D1** | Provider module skeleton | **closed** (`0c3319b`) |
| **D2** | Bounded fixture schema + resolver | **closed** |
| **D3** | CLI + `query_3dgs_training_result` dispatch | **closed** |
| **D4** | Unit/integration tests — three evidence classes | **closed** |
| **D5** | Live closure note | **closed** — `docs/ai/references/2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md` |

## Acceptance criteria (design-level)

Future live closure must capture **three evidence classes**. Validation focus is
contract stability, lineage clarity, MC-12/15 shape parity, and honest failures —
**not** accuracy vs reference or checkpoint.

### 1. Answered + visible

- Provider `status = answered`, `visibility = visible`, `screen_point` present
- `basis_frame_id` uses `closed_scene_toy:` prefix
- `comparison_verdict` may be `provider_only`, `match`, or `divergent` vs reference;
  divergent is acceptable and must not be treated as failure
- MC-14 derived view on **selected** answer remains downstream-only; MC-18 does not
  set action authority

### 2. Answered + non-clickable visibility

- Provider `status = answered`, `visibility = outside_window` (or `behind_camera` /
  `out_of_frustum`)
- `screen_point` policy follows MC-12 conventions for non-visible answers
- Proves same-shape answer without implying click readiness

### 3. Blocked / failed honest degradation

Examples:

- MC-10 `semantic_status != ready` → `blocked`, `reason = semantic_source_not_ready`
- Target not in closed label set → `blocked` with honest message
- Missing / invalid fixture → `blocked` or `failed` with `provider_output_invalid` or
  `provider_command_failed` as appropriate
- Corrupt fixture JSON → `failed`

## Explicit non-goals

- **Core-B**: do not implement provider registry, blackboard, arbiter, or generic
  scene-state runtime. Do not edit or extend
  `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
  Core-B direction (~line 469) as part of MC-18.
- **Generic provider API platform** — no shared trait layer across verticals in this
  slice.
- **Quality verdict** — no pass/fail usefulness gate on answers.
- **MC-16 / MC-17 mixing** — no holdout preview manifests, render commands, or
  photometric metrics in the toy provider path.
- **Overselling lookup** — docs, `message`, and `known_limits` must not describe
  closed-label table matching as "model understanding" or "Gaussian-native inference".
- **MC-14 extension** — no new action eligibility rules, persisted readiness artifacts,
  or live-click wiring.
- **New query result schema** — forbidden; any new fields belong in fixture sidecars,
  not `TrainingResultSpatialQueryAnswer`.

## Live-closure placeholder (future D5)

When implementation lands, record a note parallel to MC-15 live closure:

```sh
# Primary gate — visible target, closed-scene toy provider
cargo run --quiet -- minecraft query-3dgs-training-result \
  --training-result-semantic-manifest <semantic.json> \
  --target-block <x,y,z> \
  --target-face <face> \
  --target-semantics hit_face_center \
  --query-provider closed-scene-toy \
  --closed-scene-fixture <fixture.json> \
  --output-dir .tmp/mc18-live/query-closed-scene-visible

# Non-clickable visibility gate
# ... fixture row with outside_window / behind_camera ...

# Negative control — absent label / not ready semantic
# ... expect blocked or failed with honest reason ...
```

Gates to check in artifacts:

- `selected_backend = closed_scene_toy` when provider answers and wins selection
- `provider_status`, `reference_status`, `comparison_verdict` populated per MC-12
- `known_limits` contains MC-18 honest-limit strings
- `basis_frame_id` prefix is `closed_scene_toy:`

No live run required for this design slice.

## Live closure

`docs/ai/references/2026-06-27-minecraft-mc18-closed-scene-toy-provider-live-closure.md`

## Related slices

- MC-12 spatial query contract:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
- MC-15 checkpoint-native provider seam (sibling):
  `docs/ai/references/2026-06-27-minecraft-mc15-checkpoint-native-query-provider-seam-design.md`
- MC-14 action-facing consumer (downstream only; not extended):
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
- MC-10 semantic gate:
  `docs/ai/references/2026-06-27-minecraft-mc10-result-semantic-validation-design.md`

## Deferred

- MC-18+ richer fixtures shared across CI and live closure
- Cross-provider compare matrices (MC-15 vs MC-18) beyond MC-12 dual-backend fields
- True Gaussian render inference (remains MC-15+ / MC-17 lineage, not MC-18)
- Core-B extraction after a second vertical consumer exists (owner decision, separate slice)
