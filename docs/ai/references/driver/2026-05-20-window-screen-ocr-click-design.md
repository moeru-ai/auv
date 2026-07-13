# Window and Screen OCR Click Design

## Context

A NetEaseMusic playback recipe exposed a flaw in the current PoC command set.
The recipe stored fixed global logical click coordinates for the search box and
result row. When the app window moved to another display, those coordinates
landed on unrelated UI. The recipe still appeared to pass because OCR
verification looked across too broad a region and matched stale bottom-player
text.

The root issue is broader than that recipe. AUV has screen-level OCR click
commands, window capture commands, and window-relative click commands, but they
do not share one explicit observation model. Screen OCR currently behaves like
main-display OCR. Window commands do not all use the same candidate/resolver
model. The project needs a complete window/screen OCR command family before
recipes can reliably express replayable UI workflows.

This design is PoC-scoped and does not preserve backward compatibility.

## Goals

- Define screen, display, window, region, window candidate, and window resolver
  semantics in durable docs.
- Migrate screen-level OCR commands away from implicit main-display behavior.
- Add a complete window-level text and row command family.
- Promote window listing to a first-class API aligned with `listDisplays`.
- Make all window commands share one candidate model and resolver.
- Migrate NetEaseMusic playback recipe away from fixed global coordinates.
- Preserve inspectability through JSON and text artifacts.

## Non-Goals

- Do not add recipe-level structured output binding in this phase.
- Do not support cross-display window capture in this phase.
- Do not design browser element overlap semantics beyond documenting the
  containment concern.
- Do not retain old command behavior for compatibility.

## Terminology

`screen` is the logical desktop observation surface formed by one or more
displays.

`display` is a physical or system-reported monitor area. Display selectors use
display terminology such as `display_ref`, `native_display_id`, and `main`.
AUV should not expose `screen_id`; the screen is logical, not an addressable
physical object.

`window` is an application-owned observation surface with bounds, owner
metadata, and display relationship metadata.

`region` is a crop/filter inside the active observation scope. Region ratios
are always relative to the active scope.

`window candidate` is one possible window match returned by `debug.listWindows`.
Candidate ordering helps humans and fallback selection, but it is not a stable
identity.

`window resolver` is the shared mechanism that turns a target app and optional
window selector into one selected window candidate.

These definitions are also added to `docs/TERMS_AND_CONCEPTS.md`.

## Command Family

### Window Listing

`debug.observeWindows` should migrate to:

```text
debug.listWindows
```

`debug.listWindows` is the window counterpart to `debug.listDisplays`. It
returns ordered candidates and writes both machine-readable JSON and
human-readable text artifacts.

Each candidate should include:

```text
windowRef
nativeWindowId
ownerBundleId
ownerPid
title
bounds
displayRef
nativeDisplayId
isMainCandidate
isFullyContainedInDisplay
area
layer
visibility or onscreen metadata when available
selectionReason
```

Candidate order is not a stable selector. A `candidateIndex` may exist as an
artifact/display field for human inspection, but it should not be accepted as a
recipe selector or command selector. AUV should not make users depend on window
list ordering for automation.

### Window Resolver

The resolver should be shared by:

```text
debug.captureWindow
debug.clickWindowPoint
debug.findWindowText
debug.waitForWindowText
debug.clickWindowText
debug.findWindowRows
debug.waitForWindowRows
debug.clickWindowRow
```

Resolution order:

```text
explicit window selector
-> target app main candidate
-> largest visible normal fully-contained candidate
-> structured ambiguity or failure
```

Supported selectors in this phase:

```text
--window_ref
--native_window_id
--title
```

Candidate order may be shown as `candidateIndex` in artifacts for readability,
but it is not a stable identity and is not part of the selector contract.

If an app has multiple plausible windows across displays and no selector
resolves the ambiguity, commands should fail and point users to
`debug.listWindows`.

### Screen Text and Row Commands

Existing screen commands remain named as screen commands, but their semantics
migrate:

```text
debug.findScreenText
debug.waitForScreenText
debug.clickScreenText
debug.findScreenRows
debug.waitForScreenRows
debug.clickScreenRow
```

They operate on the logical screen, with a display-backed capture source chosen
by this order:

```text
explicit display selector
-> display containing the resolved target app window
-> main display
```

Supported display selectors:

```text
--display_ref
--native_display_id
--main true
```

If a target app resolves to multiple plausible windows on multiple displays,
screen-level commands should fail with an ambiguity error unless the user also
supplies a display or window selector.

### Window Text and Row Commands

Add:

```text
debug.findWindowText
debug.waitForWindowText
debug.clickWindowText
debug.findWindowRows
debug.waitForWindowRows
debug.clickWindowRow
```

These mirror the screen-level commands but capture the resolved window instead
of a display. OCR bounds and row bounds are local to the captured window image,
then projected through the window capture contract into global logical
coordinates for pointer actions.

Window commands require a single-display window in this phase. If the resolved
window crosses displays or has ambiguous containment, commands should fail with
metadata that includes window bounds and candidate display relationships.

### AX Tree Naming

`debug.observeWindowTree` currently sounds like a process tree or generic
window hierarchy, but the implementation captures accessibility information.
This should be renamed in the same migration wave to the clearer AX-specific
name:

```text
debug.observeAxTree
```

The public command should make it clear that the output is an accessibility
tree, not a window candidate list.

## Shared Implementation Shape

The implementation should avoid separate screen/window copies of the same OCR
logic. Internally, introduce shared helpers for:

- resolving an observation source
- capturing a display or window with a coordinate contract
- applying scope-relative region constraints
- running OCR text matching
- detecting rows with OCR-first and visual-band fallback
- projecting captured image pixels to global logical coordinates
- producing JSON and text artifacts

The public API stays explicit (`Screen` commands and `Window` commands), while
the internal implementation shares the capture/OCR/projection pipeline.

## Recipe Migration

The NetEaseMusic recipe should no longer use fixed global `clickPoint`
coordinates.

Target flow:

```text
captureWindow before-search
clickWindowPoint search box by window-relative position
pasteTextPreserveClipboard query + return
waitForWindowText "Cure For Me" inside result region
findWindowText "AURORA" inside result region
clickWindowText "Cure For Me" inside result region with click_count=2
captureWindow after-play
findImageText title/artist inside bottom-player region
```

If title OCR is unstable, the activation step can use:

```text
waitForWindowRows inside result region
clickWindowRow row_index=1
```

The previous fixed-coordinate inputs should be removed:

```text
search_click_x
search_click_y
result_click_x
result_click_y
```

The old validated-local status should not survive migration. The migrated
recipe must be revalidated with a live run.

## Artifacts and Inspectability

Every list/find/wait/click command should produce artifacts aligned with the
rest of AUV:

- Human-readable text report.
- Machine-readable JSON report where the command output contains structured
  candidates, matches, rows, scope, selected source, and projected points.
- Screenshot artifact when a capture was used.
- Capture contract artifact when a display or window capture was used.

Important report fields:

```text
scope
captureSource
displayRef
nativeDisplayId
windowRef
nativeWindowId
windowBounds
region
matchBounds or rowBounds
screenshotPoint
logicalPoint
selectionReason
ambiguityReason
```

The recipe engine still only supports coarse step artifact references in this
phase. Structured recipe binding can consume these JSON reports later.

## Error Handling

Ambiguity should be explicit. Commands should fail rather than guess when:

- multiple target windows are plausible across displays
- a window selector matches multiple candidates
- a window is not fully contained in one display
- a display selector does not match current display candidates
- OCR text or row detection finds no filtered match
- the projected click point is outside the selected capture contract

Error messages should recommend a concrete next step, usually
`debug.listWindows` or `debug.listDisplays`.

## Testing

Unit tests should cover:

- screen source selection order
- display selector parsing and failure cases
- window candidate ordering metadata
- resolver ambiguity and explicit selector behavior
- region ratios relative to screen/display/window scope
- projection from window capture pixels to global logical coordinates
- row detection reuse across screen and window captures
- command catalog registration for all new/renamed commands
- recipe validation after NetEaseMusic migration

Live validation should include:

- `debug.listDisplays`
- `debug.listWindows --target com.netease.163music`
- `debug.captureWindow --target com.netease.163music`
- `debug.findWindowText` on a visible NetEaseMusic window
- `debug.clickWindowText` or `debug.clickWindowRow` against a known result
- migrated NetEaseMusic recipe dry-run and live run

## Open Follow-Up

Recipe-level structured output binding remains a separate design. This phase
should prepare for it by writing structured JSON artifacts, but not add bind or
JSONPath syntax yet.
