# Terms and Concepts

This document defines the working vocabulary for AUV runtime recording,
inspection, and future replay work. Terms marked as provisional are design
terms, not stable public API names.

## Run

A run is an explicitly created correlation, persistence, inspection, and replay
scope. It has no start, finish, status, or seal fact in V1 and is not an
OpenTelemetry trace.

## AuthorityId

An `AuthorityId` is the stable, non-secret identity of the sole authority
`RunStore` selected to persist a run. It prevents one propagated `RunId` from
silently splitting canonical history across stores.

## Operation Scope

An operation scope is an ordinary caller-named AUV span around app or driver
work. It is not a persisted operation entity and does not require an AUV-owned
operation trait, runner, execution id, or session object.

## Run Commit

A run commit is one atomic, ordered set of facts accepted by the authority
`RunStore`. Accepted commits are the canonical durable truth for a run.

## Run Snapshot

A run snapshot is a disposable read model reduced from accepted commits through
one revision. `through_revision` is a read cursor, not a schema version.

## Dispatch

`Dispatch` routes typed AUV emissions to configured authority and projection
destinations. It owns routing policy and does not execute operations, schedule
application work, or contain an operation catalog.

## Context

`Context` is a cloneable snapshot of the current AUV run and optional span scope
together with its associated `Dispatch`. It propagates instrumentation scope;
it is not an operation session or application runtime.

## RunStore

`RunStore` is the authority storage and read port for ordered commits, artifacts,
snapshots, history, and recovery. It is not an exporter, hook, subscriber,
application runtime, or operation handler.

## Projection

A projection is a deliberately lossy mapping from canonical AUV run data into
another read or telemetry model. Projections support presentation and external
observability; they are not canonical run truth.

## Verification

Verification evaluates asserted external state. It is independent from target
resolution, input delivery, operation completion, and persistence.

## InspectSection / InspectDocument / InspectComposer (provisional)

Provisional core vocabulary for shared inspect composition across frontends
(owned by `crates/auv-inspect-model`):

- **InspectSection** — one collectible inspect unit (`id` + `collect` → optional
  type-erased `InspectSectionOutput { id, text, json }`).
- **InspectDocument** — ordered list of collected section outputs; `render_text`
  concatenates section text in registration order.
- **InspectComposer** — explicit value holding registered sections; product CLI,
  product MCP, and product inspect-server projection use the same product factory
  and section set, with each frontend explicitly injecting its composer `Arc`
  into all text / document paths for that lifecycle. Core library default
  composer is core-only (prefix+suffix). Named JSON extensions (e.g. quality
  baseline) are served via generic `/runs/{id}/extensions/{extension}` keys
  registered by the product projection — not first-class donor routes in the
  shared server.

Semantics: registration order is render order; duplicate registered ids fail
assembly; after `collect` returns `Some(output)`, `output.id` must equal the
registered `section.id()` (mismatch aborts the document); `collect` returning
`None` omits the section; a section error aborts the document. Product assembly
(not the core library default) owns donor-including composers.

## Product CLI package / auv-cli (provisional)

Provisional packaging term for the app-integration composition package
(`auv-cli`, located at `crates/auv-cli`):

- Owns root `auv` and app-specific bins, CLI frontend, integration wiring, product
  `InspectComposer`, query-wired OperationResult adapters (S3b; stay in product
  until contract ownership moves), and product inspect-server projection wrappers.
- Depends on library-only `auv-runtime` plus `auv-game-*` / `auv-godot`.
- Must not be confused with core `auv-runtime`; game crates must not depend on
  `auv-cli` to reach product types.

## Device

A device is the controllable/observable computer target a run executes
against. Examples include the local macOS host, a remote macOS host, a macOS
or Windows VM, a container desktop, and future browser-like sandboxes.

Device identity is not a required field in the V1 run contract. Callers may
record it as domain metadata when a workflow needs to distinguish targets.

The current AUV release only executes on the local macOS host. Remote, VM,
and container devices are a planned protocol direction; they are not
implemented yet.

## Session

A session is the automation context on a device. It groups target app/window
defaults, observation cache, run recording state, and per-session
permission/capability profile.

The V1 run contract does not require a `session_id` or an AUV session object.
Application runtimes may use session concepts for caches, namespaces, and
action locks without making them canonical run identity.

Today there is one implicit session per CLI invocation for ordinary command
execution. The first in-process `SessionRuntime` substrate now exists for
callers that need resource-style observation state: it can hold providers
across repeated observe calls, retain observation/node resources, record
verification resources, and invalidate observations after an action. Daemon
transport, JS/REPL handles, session-scoped artifact namespaces, and
device-level action locks are still planned, not implemented.

## Span

An AUV span is an optional timed diagnostic scope inside a run. Spans may form
a tree through `parent_span_id`. An operation scope is one ordinary caller-named
use of this span API; spans need not belong to an operation execution and do not
create an independent tracing authority.

A span becomes durable only when the run's authority `RunStore` accepts it
through a `RunCommit`. Until then it is transient diagnostic data.
OpenTelemetry spans are projections, not AUV persistence identity.

## Event

An AUV event is an optional typed, timestamped point-in-time fact associated
with the current run and, optionally, a span. Events need not belong to an
operation execution and do not create an independent event or tracing authority.

Examples include `command.resolved`, `driver.invoke`, `action.started`,
`artifact.captured`, `assertion.passed`, and `assertion.failed`.

An event becomes durable only when the run's authority `RunStore` accepts it
through a `RunCommit`. Events should describe small occurrences; structured or
large payloads belong in typed values or artifacts.

## Artifact

An artifact is committed inspection, evidence, replay, or domain-output
material owned by a run. It combines typed metadata with authority-owned bytes
and becomes visible only when the authority `RunStore` atomically includes it
in a `RunCommit` after validating the complete byte stream.

Artifacts may optionally be associated with a span. V1 does not assign artifact
ownership to an operation execution or verification. Artifacts may contain
structured JSON documents, images, reports, logs, media, or other files.

Examples include screenshots, click-overlay images, accessibility snapshots,
driver input/output JSON, distillation reports, validation reports, and video
segments.

Committed typed facts and resources refer to artifacts through `ArtifactUri`.
An `ArtifactUri` is the transport-independent identity of an artifact. Spans
and events may add diagnostic links, but they do not own artifacts; large
payloads remain authority-owned bytes rather than embedded event data.

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
is not a stable identity. Workflow code and legacy recipe compatibility paths
should prefer explicit selectors such as a window ref from the same observation,
a native window id, an owner/title predicate, or another documented stable
selector over relying on a bare list index.

## Window Resolver

A window resolver turns a target application and optional window selector into
one selected window candidate.

All window-scoped commands should share the same resolver so that
`captureWindow`, `clickWindowPoint`, OCR window commands, and row window
commands agree about which window they are using. When the resolver cannot make
a clear choice, it should return an ambiguity error that points users to the
window-listing API instead of silently selecting an arbitrary candidate.

## Window Mutation

A window mutation changes a resolved window's geometry or coarse window state,
such as moving, resizing, setting a frame, minimizing, restoring, or zooming a
window.

Window mutation is a driver-level window management capability. It is not an
input delivery result and should not be reported as `InputActionResult`.
Drivers should report the selected mutation path, attempts, before/after frame
or state evidence when available, and verification outcome separately from
pointer, keyboard, or overlay presentation.

On macOS, the first implementation is AX-backed and best-effort across
applications. When a native window id is available, it should be treated as the
authoritative target identity; title matching is only a fallback when no native
window id was requested.

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

## Spatial Result Consumption Pattern

Spatial result consumption pattern is a provisional design term for a
consumption-first chain over persisted result artifacts:

```text
producer artifact
→ semantic gate
→ spatial query
→ action readiness view
→ witness artifact
→ quality measurement
```

This is a pattern note, not a stable runtime API. See
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
for the current design boundary, ownership split, and defer list.

## Semantic Gate

Semantic gate is a provisional term for the first typed consumer over a
persisted producer artifact.

It answers whether the upstream artifact is structurally consumable for the
next semantic stage. A semantic gate should preserve lineage, report explicit
status and reason, and avoid grading usefulness, outcome quality, or downstream
actionability.

The current expected stage-state shape is `ready`, `blocked`, or `failed`.
This term is design vocabulary, not approval to extract current app-specific
semantic gate code into core. See
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
and
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-admission-table.md`.

## Action Readiness View

Action readiness view is a provisional term for a derived read model over an
existing persisted query result.

It answers whether an existing answer can be consumed by action-facing code
without rereading the raw query contract each time. An action readiness view
does not dispatch actions, does not back-write new producer truth, and must not
upgrade a blocked or failed query into readiness.

The current expected action-facing shape is `click_ready`,
`answer_non_clickable`, or `not_consumable`.

## Witness Artifact

Witness artifact is a provisional term for a persisted evidence artifact that
names the concrete witness item later measurement or audit should use.

Typical witness facts include the selected evidence frame, basis artifact,
comparison image or scene reference, and copied lineage. The key rule is that
later stages should consume the authoritative witness artifact rather than
silently re-selecting witness inputs from scratch.

Witness artifact is still evidence, not usefulness verdict.

## Quality Measurement

Quality measurement is a provisional term for an evidence-bearing measurement
stage over an authoritative witness artifact.

It records what measurements were computed, under which backend or measurement
policy, and with which known limits. It should stay explicit about omitted
metrics, alignment assumptions, resizing or non-resizing policy, and partial
measurement conditions.

Quality measurement is narrower than quality verdict. The current expected
evidence shape is `measured_only`, `metric_partial`, `blocked`, or `failed`.
It should not imply downstream promotion, usability judgment, or action
approval by itself.

## Capture Frame

Capture frame is a provisional term for an in-memory screenshot or cropped
image result before it is persisted as an artifact. A capture frame should carry
image data plus coordinate metadata, capture source, backend, scale, and timing
information.

Driver crates may produce capture frames. The caller or configured
instrumentation path decides whether to persist them as artifacts. This keeps
the operation path from requiring synchronous filename allocation or image
writes when the caller only needs pixels for OCR, recognition, or immediate
interaction logic.

## Input Mode

Input mode is a provisional term for the caller's allowed input disturbance
level. It describes constraints such as background-only operation, preferring
background operation, or allowing foreground fallback. The exact type name is
still under review.

Input mode is not the same as the selected native input method. For example,
a background-only click might be delivered through an AX action, a pid-targeted
CGEvent, a browser protocol command, or an ADB input path depending on the
target and driver capabilities.

## Scroll Delivery Strategy

Scroll delivery strategy is a provisional driver contract for the ordered
scroll candidates an action may try under an input mode. It is pre-execution
intent, not proof of what happened. Examples include AX scroll, window-targeted
wheel delivery, window-targeted keyboard scroll, and foreground/global HID.

The selected input delivery path is the post-execution fact. A background
preferred scroll may try background candidates first and still report
foreground/global HID when fallback was required. For scroll input,
foreground preferred means foreground/global HID can be the first candidate;
it is not the same as background preferred with faster fallback. Scroll scans
and product workflows should record the selected path next to observation
evidence so reviewers can distinguish background delivery from foreground
fallback.

## Prepare For Input Options

Prepare for input options is a provisional term for how an action may prepare a
target application, window, page, or device before input delivery. Examples include
keeping the current foreground app, synthetic focus, background activation,
focus-without-raise, and explicit foreground activation.

Preparation behavior should be recorded in action results and traces because it
is central to whether an operation can run without disrupting the user's current
work. When preparation creates temporary state, the API should return an input
preparation lease that can be passed back to restore the previous state.

## Action Executor

Action executor is a provisional term for the layer that performs one concrete
input action, such as click, type text, press key, paste, or scroll, against a
target. It selects an input delivery path subject to the caller's delivery and
activation constraints, records attempted fallbacks, and returns an action
report.

The action executor is below reusable interactions such as scroll scan or
pagination. It should not own high-level workflow control; it should make one
action explainable and bounded.

## Interaction Pipeline

Interaction pipeline is a provisional term for the layer above driver
primitives and below frontends or Rust orchestration. It composes primitive
observations and input operations into reusable workflows such as candidate
extraction, candidate parsing, matching, selection, verification, list scan,
and scroll-until behavior. The retired JSON recipe lane should not be expanded
as an interaction pipeline frontend.

The interaction pipeline is not a driver. Drivers expose platform capabilities
such as capture, OCR, AX tree capture, pointer scroll, keyboard input, and
clipboard operations. The interaction pipeline decides how to combine those
capabilities for a UI workflow while preserving inspectable decisions and
evidence.

## Candidate Context

Candidate context is a provisional structured record passed to parsers,
matchers, hooks, and interaction workflows. It should include the candidate's
text, bounds, coordinate space, recognition provenance, source evidence,
surface node refs when available, rejected/filtered reasons, and optional
collection or page context.

Candidate context should be available as typed Rust data and, when needed, as a
structured JSON boundary for external code. Scalar template variables may exist
as compatibility aliases for historical data, but they should not be the main
parser or matcher contract.

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

Status: provisional. `scroll_scan` now emits per-page observation snapshots in
`ScrollScanArtifact.snapshots`; other producers still emit their current
authoritative shapes such as `RecognitionResult` and AX snapshots. The type
exists so future projection work (AX trees, OCR row groupings, image
detectors) can converge on one shape rather than diverging per producer.
Field set and semantics may shift before this is marked stable.

## Node Ref

A node ref is the stable handle for a surface node inside a run. It is the
intended input shape for later node-aware actions and verifiers.

Node refs are provisional and may gain more provenance fields later, but they
should always identify one surface node unambiguously within the recorded run.

## Anchor

An anchor is a visible or native UI signal used to locate another observation or
action target. Anchors may come from OCR text, AX text, image features, stable
window metadata, or previously recorded geometry.

Anchors are evidence, not guarantees. Workflow code and legacy recipe
compatibility paths should record which anchor was used and how it resolved to a
region, row, item, or action point.

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
ready for item-level observation or workflow handling.

A list item candidate is still not a parsed domain object. It may have geometry
and OCR fragments, but it does not become a semantic song, email, file, or table
record until Rust orchestration, a parser, or a legacy compatibility path
interprets it.

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

V1 selects exactly one authority `RunStore` for each run. An inspect server may
have one of three explicit relationships to that authority:

- read snapshots, commits, subscriptions, and artifacts from the selected
  `RunStore`;
- receive best-effort `InspectPublisher` projections whose delivery failures do
  not alter committed truth; or
- explicitly implement `RunStore`, making the inspect server the authority for
  that run.

Only `RunCommit` values accepted by the selected authority define durable run
truth. Inspect rendering, broadcasting, and best-effort publication cannot
change already committed history.

Reliable replication between authorities requires a separate protocol with an
outbox, acknowledgement, resume cursor, and conflict policy; that protocol is
deferred. Artifact metadata and bytes are committed atomically through the
authority `RunStore` artifact contract.

## Interaction Tracing Boundary

The interaction tracing boundary records macro interactions that compose
multiple driver operations. Scroll scan is the motivating example: it observes
a surface, scrolls, observes again, and records merged evidence as one
interaction-level trace structure.

The working crate name for this boundary is `auv-tracing-interaction`.
Interaction tracing may call the driver tracing boundary, but it should not
become a command catalog, recipe runtime, or platform driver implementation.

## Operation Spec

An operation spec is driver-owned metadata for one atomic capability that can
be invoked by a frontend, runtime, or orchestration layer. It names the
operation id, target driver id, driver operation name, disturbance profile,
operation namespace, and short summary.

An operation spec is not a CLI command and not a recipe step. CLI invoke may
wrap an operation spec with argument/help metadata, and Rust orchestration may
call the same operation contract without going through CLI parsing.

In code, the current type is `auv_driver::OperationSpec`.

## Operation Disturbance

Operation disturbance is the coarse user-visible disturbance profile attached
to an operation spec. It describes the possible effects of an operation, such
as no disturbance, focus changes, foreground activation, keyboard input,
clipboard use, or pointer movement.

Operation disturbance is metadata for planning, help text, review, and future
policy checks. It does not prove semantic success; the recorded operation
result and verification evidence carry that.

In code, the current type is `auv_driver::OperationDisturbance`.

## Operation Namespace

Operation namespace is a provisional taxonomy for grouping driver operations
by execution family, such as observation, action, verification, scan, overlay,
domain workflow, or test fixture.

The namespace is set explicitly on operation specs rather than inferred from a
CLI command id. This keeps future RPC, MCP, library, and CLI frontends from
guessing behavior from string prefixes. The taxonomy is provisional and may
grow as typed driver APIs replace more legacy string-operation adapters.

In code, the current type is `auv_driver::OperationNamespace`.

## CLI Invoke Boundary

The CLI invoke boundary owns ad-hoc command invocation as a frontend capability.
It parses or receives invoke-style command ids and arguments, then routes to
typed handlers or temporary adapters without owning driver execution or run
recording. The current invoke redesign is intentionally breaking: legacy
bundle, recipe, skill, `debug.*`, `verify.*`, and app-specific `music.*`
command ids should not be retained as executable compatibility aliases.

The crate for this boundary is `auv-cli-invoke`. It owns invoke command
registration, argument metadata, and help rendering. Commands are organized as
a domain-owned command tree: each domain exposes its own group or subtree, while
the registry composes groups and flattens them for lookup. Command declarations
are handler-first: the annotated handler function generates the invoke command
export, so command id, argument metadata, driver mapping, and handler identity
stay together. It wraps driver operation specs but does not own the operation
contract itself. It is not the core runtime and should not own run recording
semantics, recipe execution, or bundle discovery.

## Historical Terms

The definitions below record vocabulary used by the legacy run-recording
implementation. They are retained for migration context and are not the active
contract.

### Operation

An operation was a reusable typed capability identified by `OperationName`.
CLI command names and MCP tool names were described as frontend routes to an
operation rather than canonical execution identity.

### Operation Execution

An operation execution was one invocation that actually started. Its direct
typed result was defined independently from persistence and verification.

### Trace

A trace is one complete inspectable workflow. Examples include one Rust
orchestration workflow, one app probe, one validation pass, or one ad-hoc
command invocation. Historical JSON recipe execution produced traces before
the recipe lane was retired.

A trace is the unit that inspection tools load as a whole. It should contain
enough structure to reconstruct what AUV attempted, what happened, what state
was observed, and which captured materials support that account.

### Run

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

### Run Recording Backend

A run recording backend is a dependency of execution surfaces, not of the
legacy `Runtime` type specifically. It owns run recording effects by combining
one store for canonical snapshots and artifact staging with one or more run
recorders for incremental updates.

The backend lets CLI, library calls, and future frontends share the same runtime
execution model while choosing different recording policies. Examples include
local-only recording, local plus inspect server reporting, server-required
reporting, and library-supplied recorders.

### Driver Tracing Boundary

The driver tracing boundary is implemented by `auv-tracing-driver`. It owns
durable AUV run/span/event/artifact recording and may emit Rust `tracing`
spans/events for observability. It does not install global subscribers or
OpenTelemetry exporters; binaries and servers configure those layers.

Typed driver calls and Rust orchestration should use this boundary when they
need inspectable artifacts without depending on command catalog or CLI argument
parsing code. The root `Runtime` still exposes temporary facade methods for
remaining invoke and historical callers.

### Run Recorder

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

## Action Resolver

An action resolver is the policy layer that chooses how AUV will act on a
target after that target has been grounded. It does not discover the target
from scratch; it consumes a query, candidate, or surface node and selects an
execution method such as AX action, AX focus, keyboard/menu command, or pointer
fallback.

The resolver must record the selected method, fallback policy, fallback reason,
disturbance class, and evidence artifacts. A successful dispatch is not the
same thing as semantic success; Rust orchestration or legacy recipe
compatibility paths still need verification results for the expected state.

Status: provisional. The first implementation scope is `debug.smartPress`
(`ax-action` first, optional `pointer-click` fallback). It is a discovery and
debug contract, not a production default for validated workflows.

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

Status: retired.

A scan hook is the historical recipe-manifest hook used by the removed JSON
scroll-scan implementation.

New scroll-scan work should use `auv-tracing-interaction` with typed Rust
context and hook contracts. Do not add new recipe-manifest hook execution.

## Sub Recipe

Status: retired.

A sub recipe is a historical recipe manifest invoked by another runtime
workflow instead of directly by a user-facing command.

Sub recipes must not be expanded as an active workflow mechanism. The checked-in
recipe lane has been deleted; future composition should use typed Rust
orchestration.

## List Scan Hook

Status: retired.

A list scan hook is the historical scan-hook variant used while scanning
list-like content.

Future list-scan behavior should live in typed Rust orchestration or
`auv-tracing-interaction`, not recipe logic.

## Tombstone

A tombstone is a short file or module-level comment left after a path is removed
or archived. It contains no execution logic, names the removed path, points to
the replacement owner, and states the exact condition for deleting the
tombstone.
