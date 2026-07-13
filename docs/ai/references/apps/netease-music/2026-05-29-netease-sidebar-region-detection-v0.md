# NetEase Sidebar Region Detection v0

Date: 2026-05-29

Status: v0 detection algorithm spec for the NetEase Cloud Music
sidebar. **Every threshold, weight, and stage ordering decision below
is marked `REVIEW(...)` because the heuristics are starting points
that must be tuned against real NetEase captures before being treated
as load-bearing.**

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing `SidebarRegionParser` in
`auv-example-netease-playlist/src/parsers/region.rs`.

## Purpose

The view parser design lists "which evidence best detects the resized
sidebar" as an open investigation. The
`view-parser-layer-contracts-v0.md` spec pins the `RegionParser` trait
signature but leaves the algorithm open.

Without a v0 algorithm, the implementer either:

- guesses at strategy ordering and threshold values silently, or
- writes an ad-hoc detection routine that drifts from the diagnostic
  policy (`RegionNotFound` / `RegionCollapsed` / `RegionResized`).

This spec pins a v0 default strategy as a starting point. It
explicitly does **not** declare these heuristics correct. Every value
carries a `REVIEW` marker naming the condition under which it should
be tuned.

## Relationship to other specs

```text
view-parser-ir-shapes-v0.md            ViewRegion + ViewBounds + ViewportFingerprint
view-parser-layer-contracts-v0.md       RegionParser trait + RegionParseOutput
view-parser-diagnostic-policy-v0.md     RegionNotFound / RegionCollapsed / RegionResized firing rules
view-parser-trace-layout-v0.md          view.parse.region_detect span + attributes
view-parser-example-placement-v0.md     SidebarRegionParser lives in the example crate
view-parser-ir-netease-playlist-example-design.md   resized / collapsed / absent must be distinguished
```

## v0 cascading strategy

Region detection runs three stages in order. Each stage either
produces a region candidate with confidence, or hands off to the
next. Multiple stages may produce candidates; the final region is
chosen by a confidence ranking.

```text
Stage 1: AX node search          (highest precision, may not exist on every NetEase version)
   │
   ▼ (no AX candidate)
Stage 2: OCR section-header anchors    (medium precision, requires window-localized OCR)
   │
   ▼ (no anchor candidates)
Stage 3: Geometry probe          (lowest precision, fallback only)
   │
   ▼ (no candidate)
Emit RegionNotFound (Fatal) and stop.
```

The stage order is itself a v0 default:

> **REVIEW(region-stage-ordering):** AX first is the v0 default because
> AX gives precise bounds when available. If NetEase versions tested
> show AX is unreliable (e.g. role hierarchy changes per release),
> demote AX below OCR. Track which stage produced the chosen region
> in `view.parse.region_detect` span attributes so this decision can
> be re-evaluated from collected runs.

### Stage 1: AX node search

Query the captured AX tree (via `auv-driver-macos` AX capture, ferried
through the contract `SurfaceNode` type per the bridge spec) for nodes
matching:

| Field | Candidate match values (v0 default) |
|---|---|
| `role` | `AXGroup`, `AXScrollArea`, `AXSplitGroup` |
| `subrole` | `AXContentList`, `AXNavigationList`, or empty |
| `label` / `identifier` / `description` | contains any of `"sidebar"`, `"navigation"`, `"侧边栏"`, `"导航"` (case-insensitive, NFKC-normalized) |
| `bounds.width` | between min and max (see thresholds below) |
| `bounds.height` | ≥ window height × min-height-ratio |

> **REVIEW(ax-role-allowlist):** The four role / subrole combinations
> are guesses. NetEase may use different AX roles per build. After
> first live runs, capture the actual AX tree and update this list.
> Until then, accept any match.

> **REVIEW(ax-label-keywords):** The keyword list is v0 hand-curated.
> If NetEase localizes the sidebar label differently per locale, the
> list expands. Future revisions may switch to a regex.

Stage 1 produces 0 or 1 candidate. If found, its bounds are the
candidate region bounds, with confidence
`AxConfidence(strong)`.

### Stage 2: OCR section-header anchors

If stage 1 found nothing, capture the window region and OCR it. Look
for known NetEase sidebar section headers:

| Header label (v0 list) | Match form |
|---|---|
| `"我的播放列表"` | exact |
| `"我创建的歌单"` | exact |
| `"我收藏的歌单"` | exact |
| `"推荐"` | exact |
| `"我喜欢的音乐"` | exact (special: item, not section, but stable anchor) |

> **REVIEW(section-header-list):** The five strings above are the v0
> NetEase macOS 2.x labels. NetEase versions may localize, abbreviate,
> or change them per release. After first runs, snapshot the real
> sidebar OCR and update. Do not silently extend the list at parse
> time — extension means a revision of this document.

When ≥ 2 anchor strings are found vertically aligned (X centers within
`anchor-x-tolerance`) and Y-sorted, infer sidebar bounds:

- `bounds.x` = min anchor X − `anchor-x-padding`
- `bounds.width` = anchor X span + `anchor-x-padding` × 2, clamped
- `bounds.y` = topmost anchor Y − `anchor-y-top-margin`
- `bounds.height` = bottommost anchor Y + `anchor-y-bottom-margin` −
  bounds.y

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `anchor-x-tolerance` | 6 px | `REVIEW(anchor-x-tolerance)` |
| `anchor-x-padding` | 20 px | `REVIEW(anchor-x-padding)` |
| `anchor-y-top-margin` | 48 px | `REVIEW(anchor-y-top-margin)` |
| `anchor-y-bottom-margin` | 24 px | `REVIEW(anchor-y-bottom-margin)` |

> **REVIEW(anchor-derivation):** The derivation extrapolates the
> sidebar from 2+ visible anchors. If only one anchor is visible,
> v0 falls through to stage 3. If a future investigation shows
> single-anchor derivation is reliable enough, revisit here.

If anchors found, confidence is `OcrConfidence(strong)` when ≥ 3
headers match and they are vertically contiguous, otherwise
`OcrConfidence(weak)`.

### Stage 3: Geometry probe

Last-resort fallback. Capture the window's left strip and look for a
visual band consistent with a sidebar:

- Sample the left `geometry-strip-width` pixels of the window.
- Compute color variance per row.
- A vertical band of consistently low variance (background area,
  bounded by a vertical separator) is a candidate sidebar.

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `geometry-strip-width` | 280 px | `REVIEW(geometry-strip-width)` |
| `geometry-variance-threshold` | grayscale stddev ≤ 18 | `REVIEW(geometry-variance-threshold)` |
| `geometry-min-band-height-ratio` | 0.5 × window height | `REVIEW(geometry-min-band-height-ratio)` |

> **REVIEW(geometry-probe-effectiveness):** Geometry probe is a v0
> fallback. It may produce false positives on dark-themed apps with
> wide left margins. If false positive rate is > 10 % on collected
> captures, demote stage 3 to be skipped unless explicitly opted in.

Confidence is `GeometryConfidence(weak)`.

## Ranking and selection

If multiple stages produce candidates:

| Confidence | Score |
|---|---|
| `AxConfidence(strong)` | 100 |
| `OcrConfidence(strong)` | 70 |
| `OcrConfidence(weak)` | 40 |
| `GeometryConfidence(weak)` | 20 |

> **REVIEW(confidence-score-weights):** Scores 100 / 70 / 40 / 20 are
> v0 placeholder weights. Tune against the precision / recall measured
> on captures from at least two NetEase versions.

Pick the highest-scoring candidate. Tied scores: prefer earlier stage
(more precise source). The selected candidate becomes `ViewRegion`.

The losing candidates (if any) are recorded as
`ParserDiagnostic::kind: IncompleteEvidence` with a note describing
the discarded candidate. This makes alternative evidence auditable
without polluting the chosen region.

## Resized vs collapsed vs absent

After selection, compare the candidate's bounds to a per-app baseline:

| Outcome | Rule | Diagnostic |
|---|---|---|
| Absent | Stage 3 produced no candidate either | `RegionNotFound` (Fatal, stops parser) |
| Collapsed | `bounds.width < min-parseable-width` | `RegionCollapsed` (Fatal, stops parser) |
| Resized | `bounds.width` differs from baseline by ≥ `resize-tolerance` | `RegionResized` (Warn, continues) |
| Normal | within tolerance | no diagnostic |

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `min-parseable-width` | 140 px | `REVIEW(min-parseable-width)` |
| `baseline-width` | 240 px (NetEase 2.x macOS default) | `REVIEW(baseline-width)` |
| `resize-tolerance` | 20 % of baseline | `REVIEW(resize-tolerance)` |

> **REVIEW(baseline-source):** v0 hard-codes the NetEase 2.x macOS
> default sidebar width as 240 px. After the first runs, record the
> observed baseline per NetEase version and prefer the observed value
> over the hard-coded default. A `BaselineRegistry` future module is
> the natural place; v0 does not introduce it.

Per `view-parser-diagnostic-policy-v0.md`, Fatal diagnostics pair
with `known_limits` entries on the reconstruction:

| Diagnostic | known_limits entry (v0 template) |
|---|---|
| `RegionNotFound` | `"sidebar region not detected via AX, OCR anchors, or geometry probe"` |
| `RegionCollapsed` | `"sidebar present but width <min-parseable-width>px; below v0 parseable minimum"` |
| `RegionResized` | (Warn, not Fatal; no `known_limits` entry required) |

## Cross-observation continuity

Region detection runs once per observation pass. The view parser layer
calls `parse_region(scope, viewport, ...)` for each viewport.

Across observations:

- If the region's bounds drift between observations by ≥
  `resize-tolerance`, emit `RegionResized` per observation pair where
  the drift was detected (per aggregation rules in the policy spec).
- If the region disappears in a later observation, emit `ModalBlocked`
  (if a modal is detected) or `RegionNotFound` (otherwise) and stop.
- Stale region bounds from earlier observations must never be reused.
  Each observation re-runs the cascade.

> **REVIEW(skip-redetection-on-stable-viewport):** v0 always re-runs
> the cascade per observation. If profiling shows this is wasteful on
> stable viewports (same fingerprint as previous observation), an
> optimization that re-uses the previous region is allowed — provided
> the diagnostic policy still fires on real changes.

## Span and signal contract

Per `view-parser-trace-layout-v0.md`, the region detect step lives
under `view.parse.region_detect` with the following extended
attributes (in addition to the standard required ones):

| Attribute | Value | When |
|---|---|---|
| `view.region.stage_used` | `"ax"` / `"ocr-anchor"` / `"geometry"` | always |
| `view.region.candidate_count` | total candidates considered | always |
| `view.region.confidence_score` | the winning confidence's numeric score | only when a region was selected |
| `view.region.baseline_width` | configured baseline | always |
| `view.region.outcome` | `"normal"` / `"resized"` / `"collapsed"` / `"absent"` | always |

These attributes let readers (inspect viewer, list_*) compare detection
behavior across runs without re-parsing the artifact.

## v0 done criteria

The region detection is v0-complete when:

1. The three stages are implemented in
   `auv-example-netease-playlist/src/parsers/region.rs` per the
   `RegionParser` trait from layer-contracts-v0.
2. Every threshold above lives in a single `RegionDetectionConfig`
   struct with named fields, not magic numbers scattered through
   the implementation. Tuning means changing one struct.
3. Confidence ranking matches the table; ties prefer earlier stages.
4. `RegionNotFound`, `RegionCollapsed`, `RegionResized` fire per
   the diagnostic policy with the matching `known_limits` template.
5. The span attribute set above is set on every parse.
6. Recorded-fixture tests cover at least: stage 1 success, stage 2
   success (no AX), stage 3 success (no AX, no OCR anchors), absent
   (no stages succeed), collapsed, resized.
7. Adding a new stage or threshold requires a dated revision of this
   document.

## Forbidden in v0

- Hard-coding pixel positions absolute to the screen. All bounds are
  window-local.
- Silent extension of the section-header list or AX role allow-list.
- Returning `Err(...)` for `RegionNotFound` / `RegionCollapsed`. Use
  the Ok-plus-Fatal-diagnostic pattern.
- Caching the region across observations beyond what
  `skip-redetection-on-stable-viewport` allows.
- Adding a fourth detection stage without revising this spec.
- Using OCR confidence numbers from the underlying OCR provider as if
  they were the spec's confidence scores. The spec's scores live in
  the ranking table above.

## Non-goals for this spec

Intentionally deferred:

- Cross-version baseline registry (`BaselineRegistry` future module).
- Automatic baseline learning from successful runs.
- Multi-language section header detection beyond the v0 list.
- Detection of secondary sidebar panels (right-side play queue, etc.).
- AX role discovery / dynamic role allowlist building.
- Performance budget for the cascade. v0 runs all enabled stages.

## How to use this spec

When implementing or tuning region detection:

- All thresholds in one struct. Use that struct, not literals.
- Every `REVIEW(...)` marker is a known incomplete decision. Do not
  treat them as final.
- When tuning, record the measured precision / recall and update the
  matching `REVIEW(...)` block in this document.
- If a stage's win rate drops to zero across all collected captures,
  the stage is a candidate for removal — revise this document before
  deleting code.
- Span attributes are how runs are compared across the corpus. Do not
  remove or rename them without bumping a wire version.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
