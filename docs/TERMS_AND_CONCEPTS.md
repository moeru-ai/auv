# Terms and Concepts

This document defines the working vocabulary for AUV runtime recording,
inspection, and future replay work. Terms marked as provisional are design
terms, not stable public API names.

## Trace

A trace is one complete inspectable workflow. Examples include one recipe
execution, one app probe, one app distillation, one validation pass, or one
ad-hoc command invocation.

A trace is the unit that inspection tools load as a whole. It should contain
enough structure to reconstruct what AUV attempted, what happened, what state
was observed, and which captured materials support that account.

## Run

A run is the user-visible top-level record for a trace. The `run_id` is the
stable handle used by CLI commands, storage paths, and viewer APIs.

For local storage, a run is expected to live under `.auv/runs/{run_id}/`.
Internally, a run may also carry an OpenTelemetry-compatible trace identifier so
the recorded data can later be exported to OTLP without treating the human
readable `run_id` as the telemetry trace id.

## Span

A span is a timed unit of work inside a run. Spans form a tree through
`parent_span_id`.

Expected span levels include workflow phases, case-matrix cases, recipe steps,
command invocations, and driver actions. A single ad-hoc command can be a run
with one root span and one command span. A recipe execution should be one run
with child spans for its steps and command invocations.

## Event

An event is a timestamped occurrence attached to a span. Events are small and
append-friendly. They should describe what happened, not carry large payloads.

Examples include `command.resolved`, `driver.invoke`, `action.started`,
`artifact.captured`, `assertion.passed`, and `assertion.failed`.

## Artifact

An artifact is persisted inspection material produced during a run. Artifacts
may be files, structured JSON documents, images, reports, logs, or media.

Examples include screenshots, click-overlay images, accessibility snapshots,
driver input/output JSON, distillation reports, validation reports, and video
segments.

Artifacts are referenced from spans or events by metadata. Large payloads should
remain as files or blobs rather than being embedded directly in events.

## Observation Scope

An observation scope is the coordinate and capture surface used by an
observation or pointer action. The scope determines how region ratios are
interpreted, how OCR or row bounds are projected into clickable coordinates,
and which candidate objects are eligible for selection.

The current scope terms are `screen`, `display`, `window`, and `region`.

## Screen

A screen is the logical desktop observation surface. It is the user-facing
workspace formed by one or more displays.

`screen` is a logical term, not a physical identifier. AUV should not expose a
`screen_id` for desktop automation. Commands that operate at screen level may
choose a display-backed capture source, but selector names should use display
terminology when they refer to physical or system display objects.

## Display

A display is a physical or system-reported monitor area that contributes to the
logical screen.

Display selectors identify which part of the screen to capture or inspect. AUV
may expose selectors such as a display ref, native display id, or main-display
flag. Display refs are scoped to an observation snapshot unless a command
explicitly documents a stronger stability guarantee.

## Window

A window is an application-owned observation surface with bounds, ownership
metadata, and a relationship to one or more displays.

For the first macOS window-capture implementation, AUV treats a window as
eligible for window-scoped capture only when it can be resolved to one display.
If a window straddles displays or its display containment is ambiguous, the
operation should fail with structured metadata rather than guessing. Future
platforms may need richer containment models for surfaces such as browser
elements that overlap multiple layout or backing surfaces.

## Window Candidate

A window candidate is one possible window match returned by a window-listing
operation. Candidates should include enough metadata for inspection and stable
selection, such as window ref, native window id when available, owner bundle id,
owner pid, title, bounds, display relationship, layer, area, visibility, and the
reason it appears in the ordered list.

Candidate list order is useful for presentation and fallback heuristics, but it
is not a stable identity. Recipes should prefer explicit selectors such as a
window ref from the same observation, a native window id, an owner/title
predicate, or another documented stable selector over relying on a bare list
index.

## Window Resolver

A window resolver turns a target application and optional window selector into
one selected window candidate.

All window-scoped commands should share the same resolver so that
`captureWindow`, `clickWindowPoint`, OCR window commands, and row window
commands agree about which window they are using. When the resolver cannot make
a clear choice, it should return an ambiguity error that points users to the
window-listing API instead of silently selecting an arbitrary candidate.

## Region

A region is a crop or filter applied inside an observation scope.

Region coordinates and ratios are relative to the current scope. For example, a
`region_top_ratio` on a window-scoped command is relative to the captured
window image, while the same ratio on a display-scoped command is relative to
the selected display capture. A region should not be used as a substitute for
the scope itself.

## Inspect Server

The inspect server is a read-only HTTP and WebSocket access layer over stored
and same-process live run data. It is not the runtime execution API.

The server exists so browser viewers, Android WebViews, IDE integrations, and
other tools can list runs, fetch run structure, load artifacts, and subscribe to
live run events.

The default CLI endpoint is `127.0.0.1:8765`. A standalone `inspect serve`
process can read historical runs from `.auv/runs/`; live streaming requires the
runtime and server to share the same in-process event sink.

Replay and mutation APIs are out of scope for the first inspect-server design.

## Viewer

A viewer is any UI that renders run data from the inspect server or directly
from the run store. The first viewer is expected to be browser-based so it can
work across desktop, remote, and mobile contexts.

The viewer should render spans, events, and artifacts as an inspectable
timeline.
