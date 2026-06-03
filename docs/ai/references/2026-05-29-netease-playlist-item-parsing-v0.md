# NetEase Playlist Sidebar Item Parsing v0

Date: 2026-05-29

Status: v0 item parsing algorithm spec for the NetEase Cloud Music
playlist sidebar. **Every threshold, weight, and ordering decision
below is marked `REVIEW(...)` because the heuristics are starting
points that must be tuned against real NetEase captures before being
treated as load-bearing.**

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing `PlaylistSidebarItemParser` in
`auv-example-netease-playlist/src/parsers/item.rs`.

## Purpose

The view parser design says item parsing must turn evidence into
"section + item `ViewCandidate`s" but does not pin row grouping,
section header detection, identity assignment, or partial-visibility
handling. The diagnostic policy specifies when
`SectionAmbiguous`, `IncompleteEvidence`, and `ItemPartiallyVisible`
fire; this spec pins the algorithm that decides whether to fire each.

Without a v0 algorithm, the implementer guesses every threshold and
ordering decision silently. This spec pins them as starting points
with explicit `REVIEW(...)` markers.

## Relationship to other specs

```text
view-parser-ir-shapes-v0.md            ViewCandidate / ViewNodeKind / Confidence
view-parser-layer-contracts-v0.md       ItemParser trait + ItemParseOutput
view-parser-diagnostic-policy-v0.md     SectionAmbiguous / IncompleteEvidence / ItemPartiallyVisible
view-parser-merge-fixtures-v0.md        downstream cross-viewport merge
netease-sidebar-region-detection-v0.md  region bounds passed into this parser
```

## Pipeline overview

```text
Input: region + viewport + Vec<ViewEvidenceNode>
   │
   ▼
Stage 1: Filter evidence to region bounds
   │
   ▼
Stage 2: Group OCR fragments into rows (by Y center proximity)
   │
   ▼
Stage 3: Classify each row as Section header or Item candidate
   │
   ▼
Stage 4: Assign items to the most recently observed section
   │
   ▼
Stage 5: Detect clipped / partially visible items
   │
   ▼
Stage 6: Attach icon evidence (optional, may be empty)
   │
   ▼
Output: Vec<ViewCandidate> + diagnostics
```

Each stage runs in order; later stages depend on earlier ones'
outputs.

## NetEase section taxonomy

This spec is the v0 owner of the NetEase section header → section
kind mapping. The region detection spec lists the same headers as
sidebar anchors; in v0 the lists are duplicated. The duplication is
tracked.

> **REVIEW(section-list-duplication):** v0 carries the same NetEase
> header list in two specs (here and region detection). Consolidate
> into one v0 config constant after both have been validated against
> real captures.

| Section kind | v0 header string | Match form | Carries items? |
|---|---|---|---|
| `MyPlaylists` | `"我的播放列表"` | exact | yes |
| `MyCreatedPlaylists` | `"我创建的歌单"` | exact | yes |
| `MyFavoritePlaylists` | `"我收藏的歌单"` | exact | yes |
| `Recommended` | `"推荐"` | exact | yes |
| `LikedMusicPseudoSection` | `"我喜欢的音乐"` | exact | special: single item, no children |

> **REVIEW(section-header-localization):** v0 strings are NetEase 2.x
> Simplified Chinese. Traditional Chinese, English, or future
> localizations require list extension. Track the NetEase version
> tested and the locale assumed; do not silently extend.

`LikedMusicPseudoSection` is special. In the NetEase UI it appears as
a single-item "section" that immediately renders the user's liked
songs. v0 treats it as a section with exactly one synthetic item
named after the header itself, so downstream code does not have to
special-case it.

> **REVIEW(liked-music-modeling):** v0's pseudo-section modeling is
> the simplest representation. If future commands want to treat
> "我喜欢的音乐" as an item directly under a parent, revisit. The
> trade-off is uniformity (everything is in a section) vs accuracy
> (the UI shows no header above it).

## Stage 1: Filter evidence to region bounds

Drop any `ViewEvidenceNode` whose `bounds` does not intersect the
input `region.bounds` translated into the same coordinate space.

Intersection rule: bounding-box intersection area must be > 0. Edge
touches do not count.

> **REVIEW(intersection-area-zero-rule):** Strict > 0 is a v0 choice.
> If single-pixel sliver overlaps cause noise, raise to a minimum
> intersection area (e.g. ≥ 4 px²). Track false negatives if raised.

## Stage 2: Group OCR fragments into rows

Sort surviving OCR evidence (`EvidenceSource::Ocr`) by Y center
ascending. Walk the sorted list; group adjacent fragments whose Y
centers are within `row-y-tolerance` of each other into one row.

Each row carries:

- `row_y_center` = mean Y center of grouped fragments
- `row_height` = max bottom − min top
- `row_x_extent` = (min left, max right) across fragments
- `fragments` = the grouped evidence nodes
- `text` = concatenation of fragment text in X-ascending order,
  separated by `row-fragment-joiner`

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `row-y-tolerance` | 6 px or `row-y-tolerance-rel` × estimated row height, whichever is larger | `REVIEW(row-y-tolerance)` |
| `row-y-tolerance-rel` | 0.25 (25 % of estimated row height) | `REVIEW(row-y-tolerance-rel)` |
| `row-fragment-joiner` | single space `" "` | `REVIEW(row-fragment-joiner)` |
| `estimated-row-height` | 36 px (NetEase 2.x sidebar default) | `REVIEW(estimated-row-height)` |

> **REVIEW(row-grouping-by-y-only):** v0 groups by Y center only.
> X-axis adjacency is not enforced because NetEase sidebar items are
> single-column. If future captures show false groupings (e.g. two
> items at the same Y but in different columns), add an X-overlap
> requirement.

## Stage 3: Classify each row as Section header or Item

For each row, the classifier runs in order:

1. **Exact header match.** Compare normalized row text (lowercase,
   NFKC, collapsed whitespace, trimmed) to each NetEase section
   header from the taxonomy table. First match wins.
2. **Header-styled row heuristic.** If no exact match, but the row
   has stylistic signals consistent with a header (height >
   `header-height-min`, no preceding icon evidence, or distinctly
   bolder text per AX), classify as `UnknownHeader`. Emit a
   `IncompleteEvidence` diagnostic noting the unrecognized header.
3. **Default.** Classify as Item candidate.

Header-style thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `header-height-min` | 28 px | `REVIEW(header-height-min)` |
| `header-style-requires-no-icon` | `true` | `REVIEW(header-style-requires-no-icon)` |

> **REVIEW(unknown-header-handling):** v0 raises a diagnostic and
> still treats the row as a section divider. If `UnknownHeader` is
> noisy in real captures (too many false positives), switch the
> default to treat as Item candidate and only fire the diagnostic
> in verbose mode.

If the row matches `LikedMusicPseudoSection`, it acts as a
section header AND synthesizes one item; this is the only stage 3
output that is both at once.

## Stage 4: Section assignment

Walk classified rows in observation order. Maintain a "current
section" pointer:

- A Section row sets `current_section = <that section>`.
- An Item row inherits `current_section` as its `section_hint`.
- A `LikedMusicPseudoSection` row sets the current section and
  immediately produces its one synthetic item with that section as
  its hint.

If an Item row is encountered before any Section row:

- Emit `SectionAmbiguous` per the diagnostic policy.
- Set the item's `section_hint = None`.

If an Item row is far from the last seen section header (Y delta
> `section-carry-forward-max-delta`), suspect the section context has
been lost off-viewport:

- Emit `SectionAmbiguous` per the policy.
- Set `section_hint = None` for that item and subsequent items until
  another header is seen.

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `section-carry-forward-max-delta` | 400 px | `REVIEW(section-carry-forward-max-delta)` |

> **REVIEW(section-carry-forward-rule):** 400 px is a v0 guess for
> "if a section header is this far above, you probably scrolled past
> a different section". If real NetEase sidebars are deeper, raise.
> The point is to prevent items from being assigned to a section
> header that is no longer the nearest one.

## Stage 5: Detect clipped / partially visible items

For each Item row, compare to the viewport's bounds:

| Condition | Treatment |
|---|---|
| Row is fully inside viewport | Normal |
| Row top or bottom extends beyond viewport edge by ≤ `clip-tolerance` | Normal (small clip tolerated as edge effect) |
| Row clipped by > `clip-tolerance` | Emit `ItemPartiallyVisible`; label uses only the visible text portion |

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `clip-tolerance` | 4 px | `REVIEW(clip-tolerance)` |

> **REVIEW(clip-label-policy):** v0 uses the visible OCR text as the
> label. This means a clipped row produces a short / truncated label
> in this observation. Cross-viewport merge later combines partial
> views into a non-clipped node (per merge-fixtures-v0); v0 does not
> attempt label completion in this stage.

## Stage 6: Attach icon evidence (optional)

If `EvidenceSource::IconMatch` evidence exists within or adjacent to a
row's bounds, attach it to the row's item candidate:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `icon-adjacency-x-window` | row left − 32 px to row left | `REVIEW(icon-adjacency-x-window)` |
| `icon-adjacency-y-overlap-min` | ≥ 60 % of row height | `REVIEW(icon-adjacency-y-overlap-min)` |

An item without an icon is valid; v0 does **not** require icon
evidence to confirm an item. The icon is supplementary.

> **REVIEW(icon-required-policy):** v0 makes icons optional. If real
> captures show OCR-only items have low precision (false positives
> from non-item text), make icons required for Item classification.

## ViewCandidate construction

For each surviving Item row, emit a `ViewCandidate`:

```rust
ViewCandidate {
    candidate_id: ViewCandidateId(derive_local("item", observation_index, row_index)),
    observation_index,
    kind_hint: ViewNodeKind::Item,
    label: Some(normalized_row_text),
    bounds: ViewBounds {
        origin_space: WindowLocal, // translate from region-local if needed
        x: row_x_extent.0,
        y: row_y_center − row_height / 2,
        width: row_x_extent.1 − row_x_extent.0,
        height: row_height,
    },
    evidence_refs: vec![ each evidence_id in the row's fragments ],
    confidence: assign_confidence(row),
    parser_notes: optional_notes,
}
```

For Section rows, emit a `ViewCandidate` with
`kind_hint: ViewNodeKind::Section` and the section kind in
`parser_notes` (the IR carries `domain_kind` on `ViewNode` after
merge; pre-merge the candidate uses notes).

## Confidence assignment

| Source mix | Confidence level | Provider scores carried |
|---|---|---|
| OCR alone, exact section header match | `Confirmed` | OCR provider score |
| OCR alone, header-style heuristic match | `Likely` | OCR provider score |
| OCR alone, item without icon | `Likely` | OCR provider score |
| OCR + icon evidence within adjacency window | `Confirmed` | OCR + icon scores |
| OCR text is empty or below `min-fragment-confidence` | `Unknown` | (no scores) |
| Conflicting evidence (e.g. two different section assignments for the same row) | `Contradicted` | scores from both sides |

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `min-fragment-confidence` | 0.45 (per OCR provider) | `REVIEW(min-fragment-confidence)` |

> **REVIEW(confidence-mapping):** v0 maps source mixes to confidence
> levels heuristically. Tune against precision / recall measurements
> from real captures. If `Confirmed` is over-claimed (precision drops
> below 0.95), demote some categories to `Likely`.

## Output

`ItemParseOutput` carries:

- `candidates` — every section + item `ViewCandidate` in observation
  order.
- `diagnostics` — `SectionAmbiguous` / `IncompleteEvidence` /
  `ItemPartiallyVisible` per the rules above.

## v0 done criteria

The item parser is v0-complete when:

1. The six stages are implemented in
   `auv-example-netease-playlist/src/parsers/item.rs` per the
   `ItemParser` trait.
2. Every threshold lives in a single `ItemParsingConfig` struct,
   not scattered through the implementation.
3. Section taxonomy is implemented as a single static lookup
   referenced by both stages 3 and 4.
4. Per-row identity uses `derive_local("item", observation_index,
   row_index)` so `ViewCandidateId` is deterministic per
   (observation, row) pair.
5. `SectionAmbiguous`, `IncompleteEvidence`, and
   `ItemPartiallyVisible` fire per the diagnostic policy and per
   the rules above; never fire outside those rules.
6. Recorded-fixture tests cover at least: one full section, an item
   before any section header, an item past the carry-forward limit,
   a clipped top item, a clipped bottom item, an `UnknownHeader`,
   the `LikedMusicPseudoSection` pseudo-section, OCR-only items, and
   OCR + icon items.
7. The Confidence enum value matches the rules in the Confidence
   mapping section.

## Forbidden in v0

- Hard-coding NetEase section strings outside the section taxonomy
  table. The table is the single source of truth.
- Computing per-item confidence based on UI position (e.g. "items
  at top are more confident"). Confidence is from evidence quality,
  not layout.
- Producing items without `evidence_refs`. Every item carries at
  least one OCR fragment reference.
- Assigning a section by guessing when no evidence supports it. The
  policy rule for unmatched sections is `SectionAmbiguous` plus
  `section_hint = None`, not a "most likely guess".
- Skipping Stage 5 (clip detection) under the assumption that the
  merge step will fix it. Stage 5 emits diagnostics that the merge
  step then resolves; without Stage 5 the diagnostic never fires.
- Using icon evidence to claim Item classification on a row whose
  OCR text is empty. v0 requires at least one OCR fragment per
  item; icons are supplementary, not primary.

## Non-goals for this spec

Intentionally deferred:

- Item duplicate detection within a single observation (assumed
  caught by cross-viewport merge fixtures).
- Localized header lists beyond the v0 Simplified Chinese set.
- Section-style learning from observed UI.
- AX-direct item parsing (current v0 is OCR-driven; AX is currently
  used at region detection only).
- Confidence calibration against provider-reported scores
  (v0 uses the static mapping table).
- Domain-typed projection logic — that lives in
  `auv-example-netease-playlist/src/projection/` and feeds the
  generic `ViewProjection<PlaylistSidebarProjection>` record.

## How to use this spec

When implementing or tuning item parsing:

- All thresholds in `ItemParsingConfig`. Use struct fields, not
  literals.
- All section knowledge lives in the taxonomy table. Adding or
  removing a section requires revising this spec.
- Every `REVIEW(...)` marker is a known incomplete decision; record
  measured precision / recall against captures before treating any
  number as final.
- If a stage's output is consistently empty across runs (e.g. no
  icons ever attach), the stage is a candidate for simplification —
  revise this spec before deleting code.
- `LikedMusicPseudoSection` is the only special-case construct; new
  special cases require spec revision.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
