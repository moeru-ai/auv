# 2026-06-30 AUV Core-A5b-query D1 query backend label contract

Date: 2026-06-30

Status: **docs-only contract** for proof-matrix row **70a (query backend label
discipline)**. Records the shared label vocabulary and wire discipline that
three donors already satisfy locally. **No code extraction approved** — see
[Core-A6](2026-06-30-query-readiness-closeout.md).

## Scope boundary

**In scope:**

- Row **70a** only — query backend label discipline
- Three donor enums and their persisted manifest / inspect wire shapes
- Ownership boundary for label vocabulary vs donor-local serde

**Out of scope (explicit non-goals):**

- Rows **70b** (render backend) and **70c** (quality backend)
- `HoldoutRenderQualityBackend`, `CardDetectionQualityBackend`, osu witness strings
- Manifest field renames (`selected_backend` → `query_backend` or vice versa)
- `inspect.rs` / `run_read.rs` changes
- Core-B, fourth donor scouting, CLI catalog changes
- Any shared crate or trait extraction (D3 hypothesis only below)

**Primary inputs:**

- [Core-A5b-prep backend label split review](2026-06-30-query-readiness-closeout.md)
- [Core-A6 row 70 split owner decision](2026-06-30-query-readiness-closeout.md)
- [Core-A4 quality/backend falsifier gate](2026-06-30-query-readiness-closeout.md)
- `crates/auv-game-minecraft/src/training_result_spatial_query.rs`
- `crates/auv-game-osu/src/visual_truth_spatial_query.rs`
- `crates/auv-game-balatro/src/card_detection_spatial_query.rs`

---

## Discipline rule (three invariants)

Row 70a donors are judged against these invariants (unchanged from row 70 /
A5b-prep / Core-A6):

1. **Stable label in manifest** — query-backend provenance is persisted as a
   stable `snake_case` enum wire label in the spatial-query manifest (or an
   owner-accepted equivalent). Transient runtime selection details do not
   substitute for the label.
2. **No raw command text** — provider shell commands, CLI invocation strings,
   and other transient runtime command text are **not** persisted in query
   manifests. MC dual-backend paths carry enum labels only.
3. **Inspect exposes label** — read-side inspect (or equivalent summary)
   prints the stable backend label where the query surface exists, so
   provenance is inspectable without opening raw JSON.

Trainer/job lineage strings (`trainer_backend`, `job_backend`) and capture
fixture `backend` strings remain **out of 70a scope**.

---

## Donor wire inventory

All three enums use `#[serde(rename_all = "snake_case")]` and a local
`as_str(self) -> &'static str` that mirrors the serde wire labels.

### Minecraft (`auv-game-minecraft`)

| Item | Value |
| --- | --- |
| **Enum** | `TrainingResultSpatialQueryBackend` |
| **Variants** | `CommandProvider`, `CheckpointNative`, `ClosedSceneToy`, `ProjectionReference` |
| **`as_str` / wire labels** | `command_provider`, `checkpoint_native`, `closed_scene_toy`, `projection_reference` |
| **Manifest field** | `selected_backend: Option<TrainingResultSpatialQueryBackend>` on `TrainingResultSpatialQueryManifest` and inspect report |
| **Raw command excluded** | **Yes** — provider command path persists enum label only |
| **Inspect** | `selected_backend=` on MC-12 manifest line (`src/inspect.rs`) |

### osu! (`auv-game-osu`)

| Item | Value |
| --- | --- |
| **Enum** | `VisualTruthSpatialQueryBackend` |
| **Variants** | `PlayfieldProjectionReference` (v1 single variant) |
| **`as_str` / wire label** | `playfield_projection_reference` |
| **Manifest field** | `query_backend: VisualTruthSpatialQueryBackend` on manifest and inspect report |
| **Raw command excluded** | **Yes** — v1 reference-only; dual-backend compare deferred |
| **Inspect** | `query_backend=` on query artifact line (`src/inspect.rs`) |

### Balatro (`auv-game-balatro`)

| Item | Value |
| --- | --- |
| **Enum** | `CardDetectionSpatialQueryBackend` |
| **Variants** | `DetectionBundleReference` (v1 single variant) |
| **`as_str` / wire label** | `detection_bundle_reference` |
| **Manifest field** | `query_backend: CardDetectionSpatialQueryBackend` on manifest and inspect report |
| **Raw command excluded** | **Yes** — detection bundle reference only |
| **Inspect** | `query_backend=` on query artifact line (`src/inspect.rs`) |

---

## Read-side asymmetry (accepted divergence)

| Donor | Manifest field | Inspect key |
| --- | --- | --- |
| MC | `selected_backend` (`Option`) | `selected_backend=` |
| osu | `query_backend` | `query_backend=` |
| Balatro | `query_backend` | `query_backend=` |

This naming split is **accepted divergence** for 70a. Core-A5b-query D1 does
**not** harmonize field names. Convergence (if ever desired) is a separate
owner slice and is **not** a blocker for the discipline contract or for
helper-only admissible review language.

MC additionally wraps the label in `Option<…>` because some blocked/failed paths
may omit a selected backend; osu and Balatro require the field on all persisted
manifests. That shape difference is donor-local policy, not a contract violation.

---

## Cardinality (not a blocker)

| Donor | Variant count (v1) |
| --- | --- |
| MC | 4 |
| osu | 1 |
| Balatro | 1 |

Variant cardinality differs across donors. Row 70a judges **discipline rule
recurrence**, not enum mergeability. MC's multi-backend compare seam needs more
labels; osu and Balatro v1 probes intentionally ship single-reference backends.
Cardinality mismatch is a **contract note** and convergence backlog item — it
does **not** block 70a probe recurrence or this D1 contract.

---

## Ownership boundary (write dead)

This D1 contract owns:

- **Label vocabulary discipline** — stable `snake_case` wire labels for
  query-backend provenance
- **`as_str` wire discipline** — persisted JSON label matches the donor's
  `as_str()` return for each variant

This D1 contract does **not** own:

- Donor enum definitions or variant sets
- `Serialize` / `Deserialize` derives on donor enums
- Manifest struct fields (`selected_backend`, `query_backend`)
- Inspect formatting in `auv-cli`

**Explicit non-claim:** a future shared helper crate must **not** be described
as centralizing serde for query-backend enums. Each donor retains local
`#[serde(rename_all = "snake_case")]` and serde roundtrip tests. Any D3
evaluation that treats “move serde tests into shared crate” as contract
ownership is **out of scope** for 70a.

Contrast with Core-A3 (`auv-stage-status`): stage status graduated because
donors shared an **identical three-variant enum** and could type-alias to one
serde-owning `StageStatus`. Query-backend enums have **different variant sets**
and cannot merge that way — see D2 comparison.

---

## D3 extraction hypothesis (not approved)

If an owner later approves a **Core-A5b-query D3** slice **after** D2 records
extraction candidacy and explicitly lifts Core-A6's implementation gate, the
narrowest **hypothesized** shape is:

```rust
// HYPOTHESIS ONLY — not approved, not implemented.
pub trait QueryBackendLabel: Copy {
  fn as_str(self) -> &'static str;
}
```

Donors would `impl QueryBackendLabel` on their local enums while keeping serde,
manifest fields, and inspect keys unchanged.

**No extraction approved.** [Core-A6](2026-06-30-query-readiness-closeout.md)
states: **“No code extraction approved.”** D3 is an independent owner gate,
not a consequence of D1 or D2. D2 must adjudicate F4 and F-nominal-abstraction
before any candidacy is recorded.

---

## Relationship to prior notes

| Note | Relationship |
| --- | --- |
| [A5b-prep](2026-06-30-query-readiness-closeout.md) | Analytical three-way split; 70a query half evidence |
| [Core-A6](2026-06-30-query-readiness-closeout.md) | Owner row split; 70a helper-only admissible review language |
| [Core-A4](2026-06-30-query-readiness-closeout.md) | Pre-split F2/F3 blockers; F4 open on shared consumer |
| [Proof matrix row 70a](2026-06-27-core-spatial-result-consumption-proof-matrix.md) | Footnote **¹³** → this contract |

## One-sentence summary

Core-A5b-query D1 freezes **70a query-backend label discipline** — stable
`snake_case` labels, no raw command text, inspect exposes label — across three
donor-local enums, with serde and manifest fields staying donor-owned and **no**
code extraction approved.
