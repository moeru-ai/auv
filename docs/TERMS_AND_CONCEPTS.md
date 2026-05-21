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

## AX Tree

An AX tree is a snapshot of accessibility elements exposed by a target
application or window. It is an inspection structure used for text
verification, candidate discovery, and accessibility actions when a platform
provides reliable accessibility metadata.

AX tree capture is different from window listing. A window candidate describes
system window ownership and bounds; an AX tree describes the accessibility
elements inside an app surface.

## Capture Contract

A capture contract is structured metadata that explains how an image artifact
maps to an observation scope. It should include enough information to interpret
pixel bounds, project selected points back to logical coordinates, and diagnose
why a capture was rejected.

Capture contracts are produced alongside display, region, and window captures.
They are inspection artifacts, not screenshots.

## Inspect Server

The inspect server is an HTTP and WebSocket access layer over stored and live
run data. It is not the runtime execution API.

The server exists so browser viewers, Android WebViews, IDE integrations, and
other tools can list runs, fetch run structure, load artifacts, and subscribe to
live run events.

The default CLI endpoint is `127.0.0.1:8765`. A standalone `inspect serve`
process can read historical runs from the configured store root and can accept
cross-process run updates only when write mode is explicitly enabled.

Inspect server write mode is opt-in. In write mode, runtimes can report
incremental run updates to the server over local HTTP, and the server applies
accepted updates to its configured store before broadcasting them to live
viewers. The server rejects conflicting updates instead of silently merging or
overwriting them.

Local recording and inspect server reporting are multi-write behavior. AUV does
not define a universal single source of truth when both are enabled; each target
owns the records it accepted, and callers choose which store or server they
inspect.

Artifact byte upload is available in write mode after the corresponding
artifact metadata has been accepted for the run. Replay and broader mutation
APIs remain out of scope for the first inspect-server design.

## Run Recording Backend

A run recording backend is the runtime dependency that owns run recording
effects. It combines one store for canonical snapshots and artifact staging
with one or more run recorders for incremental updates.

The backend lets CLI, library calls, and future frontends share the same runtime
execution model while choosing different recording policies. Examples include
local-only recording, local plus inspect server reporting, server-required
reporting, and library-supplied recorders.

## Run Recorder

A run recorder receives incremental run updates such as `runStarted`,
`spanStarted`, `eventAppended`, `artifactCreated`, `spanFinished`, and
`runFinished`.

Recorder implementations may persist updates locally, broadcast them to
same-process viewers, report them to an inspect server, fan them out to multiple
targets, or intentionally discard them.

## Inspect Server Session

An inspect server session is a local discovery descriptor written by
`inspect serve` when write mode is enabled. It contains the local server URL,
store root, write-enabled state, optional write token, process id, and start
time.

Ordinary CLI runs may use this descriptor when inspect server reporting is left
at its default. Discovery must use a user-private session path and reject unsafe
or non-local descriptors before sending run data.

## Viewer

A viewer is any UI that renders run data from the inspect server or directly
from the run store. The first viewer is expected to be browser-based so it can
work across desktop, remote, and mobile contexts.

The viewer should render spans, events, and artifacts as an inspectable
timeline.

## Scroll Scan

A scroll scan is a recorded workflow that repeatedly observes a window or
region, scrolls it, and accumulates visible observations into an inspectable
collection artifact.

A scroll scan records what AUV saw, how it moved the viewport, and why it
stopped. It should not claim a complete collection unless the stop evidence
supports that claim.

## Observed Collection

An observed collection is the structured result of a scroll scan. It contains
page records, raw observations, conservative clusters, optional section
candidates, hook decisions, stop evidence, and a completeness claim.

Observed collections are evidence artifacts. They are not application-specific
semantic objects such as playlists, search results, inboxes, or tables.

## Completeness Claim

A completeness claim is the scanner's structured statement about whether the
observed collection appears complete, partial, or unknown.

Completeness claims must distinguish evidence from uncertainty. For example,
`complete_by_no_visual_progress` means the scanner observed no further visual
progress under its configured policy; it does not mean the target application
proved that no additional content exists.

## Scan Hook

A scan hook is an optional recipe executed at a stable point inside a scroll
scan. Hooks can annotate observations, request stop, request retry, or adjust
future scan behavior.

Hooks are observation-only by default. Hooks that mutate UI must declare their
disturbance explicitly.
