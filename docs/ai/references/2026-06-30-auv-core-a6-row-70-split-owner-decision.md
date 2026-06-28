# 2026-06-30 AUV Core-A6 row 70 split owner decision

Date: 2026-06-30

Status: **owner decision recorded** — proof-matrix row **70 (monolithic)** retired;
replaced by **70a / 70b / 70c**. **No code extraction approved.**

## Scope boundary

**In scope:**

- Owner decision: split vs keep bound for backend label discipline
- New matrix rows, blockers, and admissibility language per surface
- Rename map for follow-on slices (`Core-A5b-*` scoped to one surface each)

**Out of scope:**

- Core-A5b / Core-A5b-query / Core-A5b-quality **implementation**
- Core-B, MC-20, controller/registry/arbiter work
- Donor enum renames or new backend variants
- Row 69 quality verdict (Core-A5a track)

**Primary inputs:**

- [`2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md`](2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md)
- [`2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md`](2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md)
- [`2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md`](2026-06-29-auv-core-a4-quality-backend-helper-falsifier-gate.md)
- [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

---

## Owner decision

**Split row 70.** Do **not** keep query, render, and quality backend discipline
bound in one matrix row.

### Why not keep bound

Monolithic row 70 mixed three contracts with different donor coverage:

| Surface | Donor coverage (post-X5) | Effect while bound |
| --- | --- | --- |
| **Query backend** | MC + osu + Balatro — discipline recurs | Graduation blocked by weaker halves |
| **Render backend** | MC only | Full row stays `not admissible` forever |
| **Quality backend** | MC (`render_backend`) + Balatro (`quality_backend`) + osu (strings) | Semantic split hidden inside “almost there” |

Core-A4 **F2 (full-row scope)** was triggered because admissibility required all
three surfaces to recur under one candidate contract. That falsifier was **correct
for a monolithic row** and **incorrect as a permanent gate** once surfaces are
honestly independent.

Keeping one row would continue to force every X-series review to repeat:
`not admissible yet` — without telling owners which half is actually ready for
review-language graduation.

---

## New proof-matrix rows

| Row | Contract name | Scoped symbols (examples) |
| --- | --- | --- |
| **70a** | Query backend label discipline | `TrainingResultSpatialQueryBackend`, `VisualTruthSpatialQueryBackend`, `CardDetectionSpatialQueryBackend` |
| **70b** | Render backend label discipline | `HoldoutRenderQualityBackend` |
| **70c** | Quality backend label discipline | MC `render_backend` (render-family), Balatro `quality_backend`, osu witness strings |

**Retired:** monolithic row **70** — tombstone footnote **¹²** in proof matrix.

Shared discipline rule (unchanged from row 70 / A5b-prep):

1. Stable backend-family labels belong in persisted manifests (`snake_case` enum or owner-accepted equivalent).
2. Raw runtime command text does not belong in persisted artifacts.
3. Inspect/read should expose the stable label where the surface exists.

---

## Per-row verdicts (Core-A6)

| Row | Main matrix verdict | Ruling | Blockers / notes |
| --- | --- | --- | --- |
| **70a** | `candidate, helper-only admissible`¹² | **helper-only admissible** (review language); default **defer** extraction | Field naming asymmetry (`selected_backend` vs `query_backend`); enum cardinality differs — **convergence**, not discipline absence. Future slice: **Core-A5b-query** only. |
| **70b** | `candidate, not admissible yet` | **keep app-specific** (MC); **defer** shared helper | Second donor with persisted `render_backend` enum under same rule **or** accept MC-only render surface permanently. Future slice: **Core-A5b-render**. |
| **70c** | `candidate, not admissible yet` | **keep app-specific** per donor; **defer** shared helper | osu quality path lacks enum (A4 F3); MC `render_backend` vs Balatro `quality_backend` semantic split; X4 strengthened Balatro lineage only. Future slice: **Core-A5b-quality**. |

**Admissible ≠ recommended now ≠ extraction pressure** (unchanged).

---

## Core-A5b naming map (no longer one vague total)

| Old name | Core-A6 meaning |
| --- | --- |
| **Core-A5b** (umbrella) | **Retired** as implementation target — use surface-specific names |
| **Core-A5b-query** | Maps to row **70a** only — narrow label-discipline helper **if** owner names extraction |
| **Core-A5b-render** | Maps to row **70b** — blocked until second render donor or MC-only acceptance |
| **Core-A5b-quality** | Maps to row **70c** — blocked until osu enum and/or quality-vs-render provenance mapping |

**Core-A5b implementation remains closed** until owner names a **surface-specific**
slice with acceptance of prep/split docs.

---

## What this decision does **not** unlock

- Shared backend-label trait or cross-donor enum merge
- Core-B extraction pressure
- Row 69 (`metric_partial`) — independent track (Core-A5a)
- Balatro live admission or fourth donor scouting

---

## Related references

- A5b-prep (analytical split; predates owner decision):
  [`2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md`](2026-06-30-auv-core-a5b-prep-backend-label-discipline-split-review.md)
- X5 (last monolithic row 70 review):
  [`2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md`](2026-06-30-auv-core-x5-post-x4-third-donor-graduation-review.md)
- Proof matrix rows 70a–70c:
  [`2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md`](2026-06-27-auv-core-spatial-result-consumption-proof-matrix.md)

## One-sentence summary

Core-A6 **splits** backend label discipline into query / render / quality matrix
rows so **70a** can carry helper-only admissible review language without render
and quality halves blocking it; **Core-A5b** as a single implementation name is
retired in favor of **Core-A5b-query**, **Core-A5b-render**, and **Core-A5b-quality**.
