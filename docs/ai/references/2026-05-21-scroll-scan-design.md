# Scroll Scan Design

## Status

Proposed design for review.

## Problem

Many GUI workflows involve content that is only partially visible until the
user scrolls. AUV needs a reusable way to observe a region, scroll it, collect
new observations, and explain why scanning stopped.

This must be a general GUI capability. It must not be designed around one
application domain such as music players, playlists, search results, chats, or
tables. Those domains should be expressed as recipes or scan hooks on top of a
small scroll-scan core.

## Goals

- Add a general recorded scroll-scan workflow for window or region scoped GUI
  content.
- Keep observation, scroll action, and scan orchestration separate.
- Make OCR the first implementation path, with AX as an optional extractor when
  available.
- Persist page-level evidence so partial results are inspectable after failure.
- Merge observations conservatively without assuming text is unique.
- Allow sub recipes to handle domain or page-specific judgments during a scan.
- Record stop evidence instead of claiming completeness without proof.

## Non-Goals

- Do not introduce OpenCV in the first implementation.
- Do not build a universal visual understanding engine.
- Do not create playlist, song, album, or player-specific primitives.
- Do not guarantee full collection completeness unless stop evidence supports
  that claim.
- Do not hide domain-specific behavior inside scanner core.

## Core Primitives

### `observeRegion`

Captures and analyzes the current visible state of a region.

Initial extractor support:

- `ocr-row`: OCR text grouped into visible row-like observations.
- `ax`: optional later extractor using AX tree nodes when reliable.
- `auto`: optional later mode that can choose AX or OCR based on evidence.

The output is one page observation, not a cross-page conclusion. It should
include screenshot artifact references, capture contract metadata, raw OCR
matches, row candidates, optional section candidates, and extractor diagnostics.

### `scrollRegion`

Performs one scroll action against a window or region and records evidence.

The command should accept a target window, scan region or anchor point, axis,
direction, amount, and settle time. It should return the before/after capture
references when requested and signals that describe whether obvious visual
change was observed.

It does not decide whether a list is complete and does not merge observations.

### `scanRegion`

Runs a recorded loop:

1. Observe the current region.
2. Run scan hooks for the page.
3. Merge page observations into an accumulated scan artifact.
4. Evaluate stop policies.
5. Scroll when scanning should continue.
6. Persist page evidence and final or partial scan artifacts.

`scanRegion` is orchestration, not a domain-specific scanner.

## Scan Artifact Model

The first durable output should be an observed collection artifact, tentatively
named `scroll-scan.json`.

It should contain:

- `scan_id`
- target window and region selectors
- page records with screenshot, capture contract, OCR or AX artifacts
- raw observations
- conservative clusters
- section candidates
- scroll boundary candidates
- hook decisions
- stop policy configuration
- stop evidence
- completeness claim
- warnings and uncertainty notes

Completeness should be a structured claim, not a boolean. Expected values:

- `complete_by_no_visual_progress`
- `complete_by_reached_boundary`
- `partial_max_pages`
- `partial_max_duration`
- `partial_unstable_content`
- `partial_next_section_candidate`
- `unknown`

Current boundary detection is direction-aware but still heuristic. AUV records
`scroll_boundary_candidates` when a scroll has happened and the next observed
page contributes no new observation signatures. The candidate records direction,
mapped boundary (`top`, `bottom`, `left`, `right`), basis, confidence, page, and
scroll count. `until-match`, `until-end`, and `until-next-section` may stop on
that candidate with `reached_boundary`; `bounded` scans intentionally ignore it.
This does not replace future stronger evidence from scrollbar/thumb geometry,
AX scroll values, or screenshot-diff stability checks.

## Observation Identity and Merging

The merge layer must not assume visible text is unique.

Each observation should carry multiple weak identity signals:

- normalized text key
- raw OCR text
- row or block bounds
- page index and viewport position
- section context if known
- optional visual fingerprint later
- source artifact references

The first merge policy should be conservative:

- Preserve raw observations.
- Group possible duplicates into clusters only when evidence is strong enough.
- Prefer duplicate-looking separate observations over destructive merging.
- Record `merge_reason` and confidence for every cluster decision.

This matters because repeated labels, duplicate song names, repeated section
headers, static UI chrome, and virtualized rows can all produce identical text.

## Stop Policies

`scanRegion` should support independent stop policies:

- `until_end`: continue until no progress or bottom evidence is observed.
- `until_next_section`: stop when evidence suggests a new section has begun.
- `until_match`: stop when a hook or matcher says the target was found.
- `bounded`: stop after max pages, max duration, or max scroll attempts.

All scans should have bounded safety limits.

Stop reasons should distinguish evidence from uncertainty. For example,
`no_new_observations` is not the same as `bottom_confirmed`, and
`next_section_candidate` is not the same as `section_completed`.

## Hookable Sub Recipes

The scanner should allow sub recipes at stable hook points so scanner core does
not become a universal decision engine.

Initial hook points:

- `per_page_after_observe`: annotate, filter, classify, or request stop after
  one page observation.
- `on_stop_candidate`: confirm or reject scanner-generated stop candidates.

Later hook points may include:

- `before_scan`
- `per_page_before_observe`
- `before_scroll`
- `after_scroll`
- `after_scan`

Hook inputs should include:

- scan id
- page index
- target window and region contract
- current screenshot artifact
- current OCR rows or AX observations
- previous page summary
- accumulated observations and clusters
- candidate stop reason

Hook outputs should be structured:

- `continue`
- `stop`
- `retry_observe`
- `adjust_region`
- `adjust_scroll`
- `annotate`

Sub recipes should be observation-only by default. Hooks that click, type,
expand controls, or otherwise mutate UI must declare disturbance explicitly.

## Section Handling

Sections are candidates, not guaranteed facts.

The scanner should support:

- OCR-derived header candidates.
- User-provided region or section hints.
- `stop_at_next_section` behavior through `until_next_section`.
- Explicit uncertainty when section evidence is weak.

The scanner must not claim it completed a section unless there is enough stop
evidence. Otherwise it should report a partial result with
`partial_next_section_candidate` or `unknown`.

## Virtualization and Progress Detection

The first version should use lightweight evidence:

- OCR row overlap between adjacent pages.
- Page-level new-observation count.
- Screenshot region pixel difference using existing image capabilities.
- Repeated top or bottom row observations.
- Consecutive no-progress counters.

OpenCV should remain out of scope for the first implementation. If lightweight
diffing is insufficient later, an optional visual backend can be designed
separately.

## Review and Workaround Markers

Implementation code should mark unresolved design boundaries explicitly:

```rust
// REVIEW: Explain the judgment that needs product or design review.
```

Use `// REVIEW:` when the code contains a threshold, naming choice, merge rule,
stop condition, or user-visible semantic claim that should be revisited.

```rust
// WORKAROUND: Explain the temporary fallback and the condition for removal.
```

Use `// WORKAROUND:` when the code intentionally uses a temporary fallback,
conservative degradation, or incomplete implementation path.

These comments should be specific. They should describe why the marker exists
and what later evidence or implementation would let the team remove it.

## Error Handling

Scanning should preserve partial evidence on failure. A failed scan should still
write page artifacts and a partial scan artifact when possible.

Important error classes:

- target window not found
- region outside capture bounds
- scroll target did not visually change
- OCR extractor failed
- hook returned invalid output
- hook disturbance exceeded policy
- scan exceeded bounds
- content became unstable

Errors should be visible in the run trace and final scan artifact.

## Testing

Initial tests should focus on deterministic logic:

- conservative merge behavior with duplicate text
- stop policy decisions
- hook output validation
- partial artifact shape
- section candidate uncertainty
- scan context serialization

Driver-level live tests can be added after the core model exists, using narrow
macOS examples as evidence rather than as domain-specific design sources.

## Open Questions

- What should the final public command names be: `observeRegion`,
  `scrollRegion`, and `scanRegion`, or more explicit names such as
  `observeWindowRegion`, `scrollWindowRegion`, and `scanWindowRegion`?
- Should scan hooks reuse the existing recipe manifest shape directly, or use a
  smaller hook-specific manifest?
- How much screenshot diffing should be implemented before a visual backend is
  needed?
- Which completeness claims belong in `docs/TERMS_AND_CONCEPTS.md` before
  implementation begins?
