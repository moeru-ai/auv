# 2026-06-29 AUV Core-X1 third-vertical admissibility MVP (Balatro)

Date: 2026-06-29

Status: design-only MVP proof contract. Implementation deferred to Core-X2.
**Balatro only** — framed as **path to donor**, not existing donor.

Parent scouting design:
[`2026-06-29-auv-core-x1-third-vertical-scouting-design.md`](2026-06-29-auv-core-x1-third-vertical-scouting-design.md)

## Framing

Balatro is the **most donor-like scout candidate** in the repository. It is
**not** the third consumption donor today. Unlike Minecraft MC-10..17 and osu,
Balatro has **no** closed chain over:

```text
semantic gate → spatial query → witness → quality evidence
```

What Balatro **does** have today (`crates/auv-game-balatro/`):

- `detector` + `observation` + `model` + optional `card_corner`
- object-oriented CLI with `--verify` on mutating commands
- setup manifest and Hugging Face–resolved ONNX assets
- image-backed and live macOS observation tests

What Balatro **does not** have:

- persisted semantic or spatial-query manifests in run-store roles
- witness or quality measurement artifacts
- real operation wire — [`operation.rs`](../../../crates/auv-game-balatro/src/operation.rs)
  holds **opaque placeholders** until an owner-approved operation slice

This document defines the **minimal admissible proof** Core-X2 must implement
before any proof-matrix graduation talk.

## Minimum chain slice

Aligned with
[`2026-06-27-auv-core-spatial-result-consumption-pattern.md`](2026-06-27-auv-core-spatial-result-consumption-pattern.md):

```text
Producer / fixture artifacts (committed)
  → Semantic gate (ready / blocked / failed)           [required]
  → Spatial query (answered / blocked / failed)        [required]
  → At least ONE of:
       - Witness seam (detection / projection alignment evidence)
       - Quality evidence + derived verdict
         (measured_only | metric_partial | blocked | failed)
       - Persisted quality_backend or render_backend enum
  → Action readiness OR live admission                 [optional — default defer]
```

**Proposed Balatro producer entrypoint (Core-X2):** committed detection or card-read
fixture package (image + detection manifest lineage) consumed by semantic gate —
not live `SKILL.md` play policy.

Observe-only framing: the MVP proves **consumption-pattern closure** on persisted
artifacts, not gameplay usefulness.

## Rows designed to pressure-test

| Proof-matrix row | MVP intent | How Balatro pressures it |
| --- | --- | --- |
| **69** Quality measurement verdict | Third `metric_partial` **semantic** datapoint | Define witness-bound card/detection eval with explicit partial-metric policy distinct from MC (omits metrics) and osu (retains metrics) |
| **70** Persisted backend label discipline | Render/quality `backend` enum on second non-MC donor | Persist `quality_backend` (or equivalent) enum on quality manifest — not free `detector_model_id` strings alone |
| **Core-C1** Live admission coincidence | Optional third live-admission substrate | **Default defer** — not required for 69/70 unlock |
| **67** Provider comparison | Bonus only | Not in MVP scope |

Core-A4 triggered falsifiers F2/F3 on row 70 remain the design target: osu
satisfies query-backend half only; Balatro MVP must add **enum discipline** on
the quality path analogous to MC-17 `render_backend`.

## Live admission

| Surface | MVP stance | Rationale |
| --- | --- | --- |
| Semantic + query manifests | **Required** | Fixture- or observation-backed persisted artifacts |
| Witness + quality | **Required** (at least one seam) | Row 69/70 pressure |
| Derived action readiness | **Optional** | Can mirror osu probe-local recurrence without dispatch |
| Core-C1 wired live action | **Defer** | macOS window targeting exists in crate ecosystem but not required to unlock rows 69/70 |
| `operation.rs` real wire | **Defer** | Placeholders stay until owner names live admission slice |

## Positive and negative paths (required before graduation talk)

### Semantic gate

| Path | Required evidence |
| --- | --- |
| **Positive** | Committed fixture with valid detection manifest → `semantic_status=ready` with lineage to producer artifact |
| **Negative** | Incompatible or missing manifest → `blocked` or `failed` with honest reason; no silent upgrade to ready |

### Spatial query

| Path | Required evidence |
| --- | --- |
| **Positive** | Target-conditioned answer (e.g. card slot / UI region query) → `answered` with coordinates or explicit answer payload |
| **Negative** | Missing target, stale observation, or blocked phase → `blocked` or `failed`; `answered` must not collapse into semantic `ready` |

### Witness or quality (at least one)

| Path | Required evidence |
| --- | --- |
| **Positive** | Witness manifest `ready` + quality verdict derived with documented `metric_partial` rule when metrics incomplete |
| **Negative** | Alignment failure or missing witness → `blocked` / `failed`; no model-success claims |

### Lineage and honesty

- Every stage manifest carries pointers to upstream artifact IDs / paths
- `known_limits` documents what the probe does **not** prove (gameplay success, autoplay, full MC-17 render parity)
- Inspect / run_read surfaces show persisted backend enum where row 70 is targeted

## Must prove in MVP

- Positive **and** negative path for semantic gate and spatial query
- At least one witness **or** quality **or** persisted `quality_backend` enum path
- Third-vertical **semantic** contribution to `metric_partial` — not label collision alone (X-F5)
- Committed fixture regression path (X-F4)

## Must not prove in MVP

- Gameplay success, autoplay usefulness, or `SKILL.md` policy quality
- Core-B enum graduation or shared helper extraction
- Full parity with MC-17 holdout render pipeline
- Core-A5a/A5b helper admissibility — evidence only
- Controller / planner / MC-20 center

## Build gap inventory (exists vs Core-X2)

| Component | Exists today | Core-X2 must add |
| --- | --- | --- |
| Detection / observation | `detector.rs`, `observation.rs`, tests | Wire as **producer artifact** with run-store role |
| CLI `--verify` | Mutating command verification evidence | Map to consumption manifests, not only stdout |
| Setup manifest | `auv-game-balatro setup` | Lineage fields for semantic gate input |
| Semantic gate manifest | — | New persisted stage + positive/negative fixtures |
| Spatial query manifest | — | Target-conditioned query over card/UI regions |
| Witness manifest | — | Detection/projection alignment evidence |
| Quality manifest + verdict | — | Four-label verdict + explicit `metric_partial` policy |
| `quality_backend` enum | — | Stable persisted label (row 70); inspect line |
| Action readiness derive | — | Optional; defer dispatch |
| Live admission / `operation.rs` | Placeholder stubs only | Replace **only** if owner adds Core-C1 to Core-X2 scope |
| Consumption fixtures | Image tests in crate | Committed semantic/query/witness/quality fixture set |

## Falsifier alignment (parent X-F1..X-F6)

| Falsifier | MVP design response |
| --- | --- |
| X-F1 | Observe/fixture-centered; no controller required |
| X-F2 | Third game domain — not MC/osu variant |
| X-F3 | Targets `metric_partial` semantics + quality backend enum gap |
| X-F4 | Committed fixture path required in Core-X2 deliverable |
| X-F5 | Documented partial-metric policy required in quality derive |
| X-F6 | This doc states path-to-donor; Balatro not donor #3 today |

## Owner trigger for Core-X2

Open **`Core-X2-balatro-consumption-probe`** when:

1. Owner accepts this MVP contract
2. Owner names Balatro as implementation candidate
3. X1 falsifier gate (parent doc) remains passing

**Still defer:** Core-A5a/A5b extraction until third-donor evidence exists and
owner names a separate slice.

## One-sentence summary

The Balatro MVP is an **observe-only, fixture-backed** semantic→query→(witness|quality|backend-enum) chain that closes honestly toward **donor #3** without claiming donor status today, deferring live admission and `operation.rs` unless owner expands scope.
