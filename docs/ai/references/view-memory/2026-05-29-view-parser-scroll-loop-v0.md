# AUV View Parser Scroll Loop Policy v0

Date: 2026-05-29

Status: v0 scroll loop control-flow spec. Pins when the `ViewParser`
implementation issues a scroll action, when it stops, and how it
distinguishes "reached the boundary" from "stuck" from "looped".

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing the observation loop inside a `ViewParser` (e.g.
`NeteaseSidebarViewParser::parse_view`).

## Purpose

The layer contracts spec said the view parser owns "the scroll loop"
but did not pin the control flow. The trace layout spec defined
`view.parse.scroll.<index>` spans without saying when they are
emitted. The diagnostic policy specified `ScrollStuck` and
`RepeatedViewport` firing rules without saying who drives them.

Without this spec, two implementations land at different loop shapes
and the boundary / stuck / loop distinctions become inconsistent
across runs. Long parses can also become unbounded if step decisions
are ad-hoc.

This spec pins the v0 loop control flow as a starting point. NetEase-
specific thresholds carry `REVIEW(...)` markers because they must be
tuned against real captures.

## Relationship to other specs

```text
view-parser-layer-contracts-v0.md       ViewParser owns the loop
view-parser-trace-layout-v0.md          view.parse.scroll.<index> spans
view-parser-diagnostic-policy-v0.md     ScrollStuck / RepeatedViewport firing rules
view-parser-ir-shapes-v0.md             ScrollBoundary + BoundaryState
netease-sidebar-region-detection-v0.md  region per observation
netease-playlist-item-parsing-v0.md     per-observation item extraction
```

## Loop shape

The canonical loop:

```text
observations.clear()
seen_fingerprints.clear()
direction = StartingDirection            // v0 default: downward
step_index = 0
while true {
    viewport      = capture_viewport(scope, current_pose)
    fingerprint   = viewport.fingerprint
    region_output = region_parser.parse_region(scope, viewport, ...)?

    if let Some(region) = region_output.region {
        evidence = collect_evidence(&region, &viewport, ...)?
        items    = item_parser.parse_items(&region, &viewport, &evidence, ...)?
        observations.push(build_observation(viewport, evidence, items, region_output.diagnostics))
    } else {
        // Fatal RegionNotFound / RegionCollapsed already in region_output.diagnostics
        break
    }

    record_fingerprint(seen_fingerprints, fingerprint, observations.len() - 1)

    if should_stop(&observations, &seen_fingerprints, &boundary_state)? {
        break
    }

    scroll_result = adapters::scroll::scroll_region(scope, region, axis, step_logical, ...)
    on_scroll_step(scroll_result, &mut boundary_state)
    step_index += 1
    if step_index >= max_steps { break }   // hard ceiling, separate from boundary detection
}
```

`should_stop` is the v0 termination decider. Sections below pin its
inputs.

## Direction policy

v0 starts by scrolling **downward** from the top of the region.

> **REVIEW(starting-direction):** v0 starts at the top because the
> NetEase sidebar shows the user's most-relevant playlists at the top
> ("我的播放列表" before "推荐"). For future view parsers whose first-
> render-priority is at the bottom, the loop must support a `Reverse`
> direction. v0 does not.

Before the loop starts, the implementation scrolls the region to its
top via a single `scroll_to_top` action (one scroll call with a large
upward step or an AX `AXScrollTo` call when available). If the scroll-
to-top action fails, treat as observed failure (continue with the
current pose; emit `IncompleteEvidence` diagnostic).

> **REVIEW(scroll-to-top-implementation):** The scroll-to-top
> mechanism is platform-specific. v0 default: use AX-based scroll-to-
> top when available, falling back to a single large upward
> ScrollStep (step size 10× normal). If neither is supported, run the
> loop from the current pose without resetting.

## Step size policy

Each scroll step moves the viewport by `step_logical` along the
scroll axis. v0 default:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `step_logical` (downward) | `viewport.height × step-fraction` | `REVIEW(step-fraction)` |
| `step-fraction` | 0.8 (80 % of viewport height) | `REVIEW(step-fraction)` |
| `max_steps` | 200 | `REVIEW(max-steps-hard-ceiling)` |

> **REVIEW(step-fraction):** 0.8 produces 20 % overlap between adjacent
> viewports, which gives the cross-viewport merge step room to align
> the same item across viewports. Too small (≥ 0.95) and the loop
> takes more steps than necessary. Too large (< 0.5) and merge loses
> stitching evidence. Tune against measured merge precision.

> **REVIEW(max-steps-hard-ceiling):** 200 is an upper bound to
> prevent runaway loops if boundary detection fails silently. It is
> not a normal termination point. If hit, the parser must emit
> `IncompleteEvidence` with `"max_steps reached without boundary"`.

## Boundary detection

A boundary is "the loop should stop because there is no more useful
content in this direction". v0 recognizes three boundary classes:

| Class | Detected by | Resulting `BoundaryState` |
|---|---|---|
| Hard boundary | Two consecutive observations produced identical viewport fingerprints (no movement after scroll) | `Confirmed` |
| Soft boundary | Two consecutive observations differ but produced zero new item candidates (already-seen items only) | `Likely` |
| Repeat boundary | The fingerprint matches a non-adjacent earlier observation | `Likely` (with `RepeatedViewport` diagnostic) |

Adjacent identical fingerprints → `ScrollStuck` diagnostic (Error,
stops loop) per the diagnostic policy. The `ScrollBoundary.bottom`
becomes `Confirmed`.

Non-adjacent identical fingerprints → `RepeatedViewport` diagnostic
(Info, continues by default but may inform termination). If the
fingerprint matches one from > `repeat-history-window` ago, treat as
suspicious — likely a loop or scroll-jump.

Thresholds:

| Constant | v0 default | REVIEW key |
|---|---|---|
| `repeat-history-window` | 4 observations | `REVIEW(repeat-history-window)` |
| `soft-boundary-no-new-items-tolerance` | 1 observation | `REVIEW(soft-boundary-tolerance)` |

> **REVIEW(soft-boundary-tolerance):** v0 declares soft boundary
> after one observation with zero new items. Aggressive but
> debounce-free; tune up if real NetEase sometimes has a single
> blank viewport before more items appear (unlikely but possible at
> section gaps).

## `should_stop` rule

The loop stops on any of:

1. Region not found in the current observation (Fatal diagnostic
   already emitted).
2. `BoundaryState.bottom == Confirmed` (hard boundary).
3. `BoundaryState.bottom == Likely` AND step_index ≥
   `likely-boundary-stop-grace` additional steps without progress.
4. Fatal diagnostic of any kind set on the current observation
   (`ModalBlocked`, `RegionCollapsed`).
5. `step_index >= max_steps` (hard ceiling; emits
   `IncompleteEvidence`).

| Constant | v0 default | REVIEW key |
|---|---|---|
| `likely-boundary-stop-grace` | 1 extra step | `REVIEW(likely-boundary-grace)` |

> **REVIEW(likely-boundary-grace):** v0 takes one extra step after a
> Likely boundary to confirm. Drop to 0 if false positives are rare;
> raise to 2 if NetEase has occasional pauses in content.

## Scroll step trace

Each scroll action emits a `view.parse.scroll.<index>` span per the
trace layout. The span's required attributes:

| Attribute | Value |
|---|---|
| `view.scroll.axis` | `"vertical"` (v0 default; NetEase sidebar is vertical) |
| `view.scroll.from_observation` | observation index before scroll |
| `view.scroll.to_observation` | observation index after scroll |
| `view.scroll.step_logical` | the requested step in logical pixels |
| `view.scroll.actual_delta` | the measured fingerprint shift (or `"unknown"`) |
| `view.scroll.detected_boundary` | `"none"` / `"likely"` / `"confirmed"` / `"stuck"` / `"repeat"` |

The `view.parse.observe.<index>` span of the next observation must
follow immediately after this scroll span. v0 does not insert any
other span between them.

## Boundary handling outcomes

When the loop stops:

| Stop reason | Loop outcome | `view.parse.outcome` |
|---|---|---|
| Hard boundary | clean | `"clean"` |
| Likely boundary confirmed by grace | clean | `"clean"` |
| Region not found / collapsed | observed failure | `"observed-failure"` |
| Modal blocked | observed failure | `"observed-failure"` |
| `ScrollStuck` | observed failure | `"observed-failure"` |
| `max_steps` reached | observed failure (`IncompleteEvidence` carried) | `"observed-failure"` |

The view parser is responsible for setting `view.parse.outcome` on
the root span (per trace layout). The Fatal diagnostic kind (if any)
goes into `view.parse.fatal_diagnostic_kind`.

## `ScrollBoundary` field population

When the loop ends, the resulting `ViewScrollable.boundary` is
populated:

| Field | Value |
|---|---|
| `top` | `Confirmed` (the parser scrolled to top before the loop) |
| `bottom` | `Confirmed` if hard boundary; `Likely` if likely with grace satisfied; `Contradicted` if `ScrollStuck` or `max_steps` |
| `repeated_viewport_fingerprints` | every non-adjacent fingerprint repeat observed during the loop |

The `ScrollBoundary` lives on the scrollable container `ViewNode` in
the reconstruction.

## v0 done criteria

The scroll loop is v0-complete when:

1. The loop matches the canonical shape above and is implemented in
   the `ViewParser` (e.g. `NeteaseSidebarViewParser`).
2. Every threshold lives in a single `ScrollLoopConfig` struct.
3. `should_stop` returns true under exactly the 5 conditions above;
   adding a 6th termination condition requires revising this spec.
4. `view.parse.scroll.<index>` span attributes match the table; each
   scroll action emits one span.
5. `ScrollBoundary.bottom` is populated per the rules table; the
   reconstruction's scrollable node carries it.
6. `view.parse.outcome` is set on the root span per the stop-reason
   table.
7. Recorded-fixture tests cover at least: hard boundary, likely
   boundary with grace, repeat detection, stuck detection, modal
   blocked, max_steps ceiling.

## Forbidden in v0

- Backward / reverse scrolling within a single loop run. v0 is one
  pass in one direction.
- Mid-loop direction reversal to "recover from a missed item". The
  cross-viewport merge step is the v0 mechanism for recovering
  partial views; re-scrolling is not.
- Variable step sizes per observation. `step_logical` is constant
  during one loop run; tuning means changing the config struct, not
  branching mid-loop.
- Adaptive `should_stop` rules based on item count or section count.
  v0 termination is fingerprint- and boundary-driven only;
  domain-specific termination (e.g. "stop after 200 items") is
  forbidden until the merge corpus is validated.
- Looping past `max_steps`. The ceiling is non-negotiable in v0.
- Bypassing the per-observation `parse_region` call (e.g. caching a
  region across the loop). The region may shift; redetection is per
  observation.

## Non-goals for this spec

Intentionally deferred:

- Bidirectional scrolling (top + bottom from a starting middle
  pose). v0 is unidirectional from top.
- Parallel observation collection. v0 is sequential.
- Predictive scroll step sizing based on previous observation
  density.
- Cross-region scroll coordination (e.g. nested scroll containers).
- Async / interruptible loops (Ctrl-C during long parses).
  Implementations may wire OS signal handling but the spec does not
  pin its semantics in v0.

## How to use this spec

When implementing or reviewing the loop:

- All thresholds in `ScrollLoopConfig`. Use the struct, not literals.
- Every `REVIEW(...)` marker is a known incomplete decision; record
  precision / recall / step count distributions from real captures
  before treating any number as final.
- The 5 stop conditions are the entire termination contract. If you
  feel you need a 6th, file a gap before adding one.
- `max_steps = 200` is a backstop, not a normal exit. Reaching it
  is always a fixable signal (boundary detection is silently failing
  or the region is much larger than expected).

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
