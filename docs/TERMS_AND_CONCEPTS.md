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

## Device

A device is the controllable/observable computer target a run executes
against. Examples include the local macOS host, a remote macOS host, a macOS
or Windows VM, a container desktop, and future browser-like sandboxes.

Every run carries a `device_id`. When callers do not specify one, the runtime
uses the default device id `local`. The id is recorded on each run's
attributes under `auv.device.id` and is threaded into every `DriverRunContext`
so drivers, evidence artifacts, and future RPC frontends can route correctly
once remote devices land.

The current AUV release only executes on the local macOS host. Remote, VM,
and container devices are a planned protocol direction; they are not
implemented yet.

## Session

A session is the automation context on a device. It groups target app/window
defaults, observation cache, run recording state, and per-session
permission/capability profile.

Every run carries a `session_id`. When callers do not specify one, the
runtime uses the default session id `default`. The id is recorded on each
run's attributes under `auv.session.id` and is threaded into every
`DriverRunContext`. The id exists so future RPC/JS-SDK/REPL frontends can
scope cache, namespaces, and action locks per session without changing the
recording contract again.

Today there is one implicit session per CLI invocation. Multi-session
semantics, session-scoped artifact namespaces, and device-level action locks
are planned, not implemented.

## Run

A run is the user-visible top-level record for a trace. The `run_id` is the
stable handle used by CLI commands, storage paths, and viewer APIs.

A run is scoped to one `device_id` and one `session_id`. Both identifiers are
recorded on the run's attributes (`auv.device.id`, `auv.session.id`) so
historical runs remain self-describing once multi-device and multi-session
land. Runs from different devices or sessions never share local state.

For local storage, a run is expected to live under `.auv/runs/{run_id}/`. The
on-disk layout is independent of device/session — those are run-record
attributes, not path components — so existing run directories remain readable
across the protocol skeleton expansion.

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

## Segmented Region

A segmented region is a derived region produced by an observation or scan step.
It is evidence about visible layout, not a user-authored target region.

Segmented regions should carry their coordinate space, bounds, role,
confidence, and evidence. For example, a list scanner may emit one segmented
region with the role `list_region` after detecting a repeated row pattern.

## Recognition Result

A recognition result is a provisional structured observation contract for
detector-like outputs. It should preserve the best match, rejected candidates,
filtered candidates, bounds, provider-native detail, and evidence references in
one inspectable object.

Recognition results sit between raw provider output and higher-level
candidates. OCR rows, visual row bands, segmented regions, icon matches, and
future detector outputs should be able to project into this shape before an
action consumes them.

## Surface Node

A surface node is a provisional structured projection of an observation or
recognition item. It carries a stable node ref, kind, label, geometry, and
provenance so later actions can refer to a node instead of raw coordinates.

Surface nodes sit between recognition results and later node-aware actions.
They are inspectable records, not app-specific semantic objects.

The inspect viewer may surface `ScrollScanArtifact.nodes` as a lightweight
preview for review, but that preview is still evidence, not a node-action
contract.

## Observation Snapshot

An observation snapshot is a provisional envelope around the
observed-UI-layer projection. It groups one moment of looking at a surface —
AX tree, OCR pass, visual detector output, scroll-scan list-item batch, or a
fused multi-source view — into one normalized record bound to a run/span.

An observation snapshot carries:

- A `source` tag at coarse granularity: `ax`, `ocr`, `visual`, `merged`.
- The `scope` it was captured in (screen / display / window / region) plus
  app/window context.
- Optional reference to a capture contract artifact that defines the
  coordinate system, scale, and source bounds.
- Evidence artifact references (screenshots, raw provider JSON, AX snapshot
  files).
- A list of `SurfaceNode`s as the per-item projection. Each node retains its
  own finer-grained `recognition_source` so the envelope's coarse tag does
  not erase per-item provenance.
- Raw provider-specific detail for debugging and forward-compatibility.
- Known limits documenting incomplete coverage, low confidence, or missing
  context.

Status: provisional. Today no producer emits observation snapshots; existing
`RecognitionResult`, AX snapshots, and scroll-scan outputs remain
authoritative. The type exists so future projection work (AX trees, OCR row
groupings, scroll-scan rows, image detectors) can converge on one shape
rather than diverging per producer. Field set and semantics may shift before
this is marked stable.

## Node Ref

A node ref is the stable handle for a surface node inside a run. It is the
intended input shape for later node-aware actions and verifiers.

Node refs are provisional and may gain more provenance fields later, but they
should always identify one surface node unambiguously within the recorded run.

## Anchor

An anchor is a visible or native UI signal used to locate another observation or
action target. Anchors may come from OCR text, AX text, image features, stable
window metadata, or previously recorded geometry.

Anchors are evidence, not guarantees. Recipes should record which anchor was
used and how it resolved to a region, row, item, or action point.

## List Region

A list region is a segmented region that appears to contain repeated list-like
content. It is not tied to one domain such as playlists, tables, search results,
or inboxes.

A list region may contain section headers, partial rows, ads, dividers, and
other non-item content. Those are filtered or interpreted by later stages.

## List Row Candidate

A list row candidate is a row-like visual or textual band observed inside a
region. It is a candidate because row detection can include headers, tabs,
partial rows, and other repeated or near-repeated layout elements.

List row candidates should preserve source evidence such as OCR fragments,
visual-band bounds, row index, and detection strategy.

## Row Filter

A row filter is a deterministic step that turns list row candidates into list
item candidates by rejecting candidates that are clearly outside the expected
row pattern.

Row filters should be conservative. They should avoid semantic parsing and
should preserve rejected candidates with reasons so a reviewer or later hook can
inspect what was lost.

## List Item Candidate

A list item candidate is a list row candidate that survived row filtering and is
ready for item-level observation or recipe handling.

A list item candidate is still not a parsed domain object. It may have geometry
and OCR fragments, but it does not become a semantic song, email, file, or table
record until a recipe or parser interprets it.

## List Item Observation

A list item observation is recorded evidence extracted from a list item
candidate on one scan page. It can include text fragments, geometry, source
artifacts, row-filter metadata, and parser attributes.

List item observations are the per-page entries that can later be merged into
an observed collection.

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

Directional scroll-boundary evidence is provisional. The current implementation
records a `scroll_boundary_candidates` list when an up/down/left/right scroll is
followed by a page with no new observation signatures. That maps directions to
top/bottom/left/right boundary candidates with either `confidence=heuristic`
(`no_new_observations_after_scroll`) or `confidence=corroborated` when repeated
row overlap or adjacent screenshot-diff stability also supports the claim. It
is still not proof from scrollbar geometry or AX scroll values.

## Observed Collection

An observed collection is the structured result of a scroll scan. It contains
page records, raw observations, conservative clusters, optional section
candidates, hook decisions, stop evidence, and a completeness claim.

Observed collections are evidence artifacts. They are not application-specific
semantic objects such as playlists, search results, inboxes, or tables.

## Surface Selector

A surface selector is a provisional, cross-surface query contract for producing
candidates from a target surface. It can describe AX, OCR, row, DOM, visual, or
command-like constraints, but a backend may support only a subset.

Surface selectors do not execute UI actions. They resolve to candidates with
evidence; actions consume those candidates and verify their own results.

## Verification Method

A verification method is the typed taxonomy of an assertion carried by a
`VerificationResult`. AUV's value isn't "the action ran"; it's "the world is
in the expected state, and here is the evidence". The method makes that
claim explicit instead of leaking it through the producing command id.

Standard methods:

- `text_visible`: a specific text fragment is visible on the captured
  surface. Evidence: OCR pass over the capture.
- `ax_text`: an AX node carries the expected label / value / role.
  Evidence: AX snapshot.
- `state_changed`: the UI state changed between two captures.
  Evidence: pre/post screenshots, AX diff, or both.
- `candidate_alive`: a previously emitted candidate is still valid.
  Evidence: re-observation of the candidate's anchor context.
- `semantic_match`: the broader semantic goal of an action was achieved
  (e.g. "the track titled X is now playing"). Evidence: domain-specific
  signals.
- `no_progress_boundary`: a scroll/scan reached a content boundary and no
  further progress is expected. Evidence: stop reason plus screenshot
  diff stability plus completeness claim.
- `custom`: producer-defined kind with a `name` hint. Consumers must not
  pattern-match on the hint string for safety-critical decisions.

Status: provisional. The taxonomy may grow. The `custom` variant lets
producers emit verifications outside the standard set without forking the
enum. Legacy `VerificationResult` records deserialized without a method
default to `custom { name: "legacy" }` so the carve-out is explicit.

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

## Sub Recipe

A sub recipe is a recipe manifest invoked by another runtime workflow instead
of directly by a user-facing command. The host workflow supplies context from
its own execution, such as a scan page, list item candidate, or stop candidate.

Sub recipes should declare their invocation host and stage in the manifest so
the caller can reject incompatible recipes before execution. Current scan sub
recipes use a provisional scalar context; typed context artifacts should replace
that once the hook contract stabilizes.

## List Scan Hook

A list scan hook is a scan hook that runs while scanning list-like content.
Depending on where it is attached, it may inspect a segmented region, list row
candidate, list item candidate, page observation, or stop candidate.

List scan hooks should make their input stage explicit. A hook that runs after
row filtering is not the same as a row filter, because it can run recipe logic
and may produce annotations or decisions rather than only deterministic
accept/reject results.
