# Terms and Concepts

This document defines the working vocabulary for AUV runtime recording,
inspection, and future replay work. Terms marked as provisional are design
terms, not stable public API names.

## Run

A Run is one explicitly created correlation and control scope from creation
through a terminal phase. Pending, running, succeeded, failed, and canceled are
phases of the same Run, not separate Workflow or Execution resources.

A Run may select several Devices, lease several Runners, and contain several
typed operations. One-shot frontends may create an implicit Run; callers may
also append later calls to an existing Run. Trace records and future Inspect
projections can carry the Run identity, but a Run is not a storage transaction,
revision stream, OpenTelemetry trace, or public Session.

## Operation Scope

An operation scope is an ordinary caller-named AUV span around app or driver
work. It may execute through a Runner and belong to a Run, but it is not yet an
independently persisted operation entity.

## Direct Result

A direct result is the typed result returned by an app or driver operation
directly to its CLI, MCP, or library caller. Recording consumes facts and
artifacts emitted by that execution; tracing and later inspection projections
never reconstruct, gate, or replace the direct application result path.

## Dispatch

`Dispatch` routes typed AUV emissions to a configured `TracingStore` and zero
or more `TraceExporter` destinations. It owns routing policy and does not execute operations, schedule
application work, or contain an operation catalog.

## Context

`Context` is a cloneable snapshot of the current AUV run and optional span scope
together with its associated `Dispatch`. It propagates instrumentation scope;
it is not an operation session or application runtime.

## Trace Record

A `TraceRecord` is one full-fidelity producer observation: span start, span end,
typed event with canonical payload, or stored artifact metadata. Records are
append-only observations. They do not imply commits, revisions, snapshots,
idempotency, recovery, or reconstructed operation results.

Trace names, error codes, artifact purposes, content types, and attribute keys
are caller-declared strings. The tracing layer preserves these values without
enforcing namespaced-name, MIME, count, or encoded-size policies. It continues
to enforce structural and storage invariants such as identifier shape, finite
JSON numbers, byte limits, and artifact integrity.

## TracingStore

`TracingStore` is the write-only port for full-fidelity trace records and
artifact bodies. It exposes `write`, `write_artifact`, and `flush`; generic
lookup, snapshots, pagination, subscriptions, and artifact reads do not belong
to this producer-side port. Concrete in-memory stores may expose observations
for tests without turning those methods into the generic store contract.

## Trace Exporter

`TraceExporter` receives trace records for an intentionally lossy external
telemetry representation. OpenTelemetry is one exporter. Exporters do not own
artifact bytes, install application-global providers, or become canonical AUV
storage.

## Projection

A projection is a deliberately lossy mapping from AUV trace records into
another read or telemetry model. Projections support presentation and external
observability; they are not storage or reconstructed operation truth.

## Verification

Verification evaluates asserted external state. It is independent from target
resolution, input delivery, operation completion, and persistence.

## Inspector (deferred)

An inspector is a future read-side component that may ingest durable trace
records, build indexes and read models, and present artifacts. It is explicitly
outside `auv-tracing`. The current `auv-inspect-model` and `auv-inspect-server`
crates do not define this boundary, and this document does not stabilize an
inspector API yet.

## Core CLI frontend / auv-cli

The core command frontend package (`auv-cli`, located at `crates/auv-cli`):

- Owns the root `auv` binary, core CLI frontend, core invoke entrypoint,
  built-in MCP server, foreground serving frontend, and development xtasks.
- Depends directly on the command, driver, protocol, and tracing crates needed
  by those frontends. There is no `auv-runtime` package or root Cargo package.
- Supported app/game packages own their command frontends and integration
  wiring; they must not depend on `auv-cli` to reach command or tracing types.

## Runtime responsibility

Runtime is an architectural responsibility, not a crate name. Typed app or
command modules execute operations and return direct results. Frontend roots
create tracing contexts and flush recording. `auv-tracing` persists emitted
events and artifacts without executing operations. AUV does not have or require
an `auv-runtime` package that aggregates these responsibilities.

## CLI plugin

A CLI plugin is an independently installed executable named `auv-<name>` that
extends the root command at one top-level name. For example,
`auv balatro cards play` delegates to `auv-balatro cards play`. The root CLI
resolves plugins through `PATH`, gives built-ins precedence, forwards remaining
operating-system arguments unchanged, and exposes its executable through
`AUV_PATH`. Plugins own their nested command trees and help; they are not loaded
into the core invoke or MCP registry.

Root Device and Run flags written before the plugin name are parent context.
The root resolves them and supplies `AuvContext` as inline JSON in `AUV_CONTEXT`.
Flags written after the plugin name remain plugin-owned; a plugin may use AUV
CLI utilities to expose equivalent overrides. `auv invoke` remains restricted
to atomic operations registered by `auv-cli-invoke` and does not invoke plugin
commands.

## AuvContext

`AuvContext` is the process/client context supplied to a CLI plugin as inline
JSON in `AUV_CONTEXT`. It carries resolved references such as Device ID,
optional Run ID, daemon discovery, configuration profile, and
credential-profile reference. It must not contain private keys, bearer tokens,
or other reusable credentials. Parsers ignore unknown fields and apply normal
configuration/default resolution when optional fields are absent; the contract
does not use a separate context version variable.

`AUV_PATH` independently identifies the root `auv` executable that launched the
plugin. It lets a plugin invoke the same AUV installation and does not carry or
select Device, Run, configuration, credential, or transport context.

A plugin can construct an AUV client from this context without reproducing
Device-name ambiguity rules or daemon discovery. A plugin's explicit flags may
override inherited values only when that plugin deliberately supports them.

## Device

A Device is an addressable execution node and trust boundary. Examples include
the local macOS host, a remote host, a VM, a container desktop, and a future
browser-like sandbox. It has a stable unique ID, an optional non-unique display
name, connection/trust profiles, labels, registered Runner classes, and live
Runners.

The current machine is represented by an implicit local Device. Omitting a
Device selection uses this local Device rather than bypassing the Device model.
Device names may be ambiguous; a Device ID is canonical.

Device-, Run-, and Runner-producing clients select local or remote execution
automatically by default from explicit options, inherited `AuvContext`, Run
placement, Runner claims, and configured Device availability. With no stronger
selection, they use the implicit local Device. A `local` shortcut constrains
placement to that Device and forbids remote fallback or offload; it does not
bypass Device, Run, Runner, lease, capability, or authorization semantics.
Combining the shortcut with a conflicting remote Device selection is an error.

Pairing is the process that enrolls or establishes trust with a Device. Device
is the resource exposed through `auv devices`; pairing is not a top-level
resource name.

## Daemon

The AUV daemon is the long-lived process role that owns API listeners,
Device authority, Runner creation, private Runner IPC, routing, health,
draining, and reusable resources. It is not a catch-all Rust runtime crate.

The accepted foreground role is `auv serve` with one or more listeners. A
future `auv daemon start|status|stop` frontend may integrate with launchd,
systemd, or brew services while executing the same serving implementation.
Listener type is transport configuration rather than a separate server role.

## Session

Session is not a public AUV control-plane resource. It may name an internal
Driver session, ONNX session, SDK connection pool, or Runner-owned cache. An
internal session is not canonical Run identity, Device identity, or an
authentication credential.

The retired experimental daemon once exposed a public `SessionService`. That
prototype, its session-scoped `Connection`, and legacy `VisionService` were
removed on 2026-07-31. Public control now uses Device, Run, Runner, and typed
capability services.

## RunnerClass

A RunnerClass is the discoverable description of a kind of Runner a Device can
create. It identifies the allowed service bundle and the lifecycle or
configuration contract for that process. One RunnerClass may implement many
protobuf services and support many application/plugin subcommands.

The daemon-side factory that realizes a RunnerClass is a RunnerProvider.
Provider is an implementation boundary; RunnerClass is the public control
resource. A custom RunnerProvider is admitted from operator-owned daemon
configuration that pins its RunnerRuntime, canonical protobuf descriptor
digest, service exposure, lifecycle policies, and capacity. Runtime Health,
Reflection, metadata, and status prove that the implementation matches that
configuration; they never grant it authority to self-register or widen public
routes.

## RunnerRuntime

RunnerRuntime is the daemon-side Rust/configuration transport used by a
RunnerProvider. It is not a protobuf resource or a client-visible placement
choice. `Executable` starts an arbitrary approved binary with arguments over
daemon-owned private IPC; first-party classes use this variant to self-spawn
the root `auv` binary. `RemoteGrpc` attaches an existing compatible gRPC
endpoint without taking ownership of its process. Both must implement typed
RunnerRuntimeService metadata/status/watch/drain plus Health, Reflection, and
their admitted business services. Stopping a RemoteGrpc-backed Runner detaches
the daemon and does not implicitly drain or terminate the remote endpoint.

## Runner

A Runner is one daemon-owned process/runtime on exactly one Device. It exposes
a validated set of versioned typed services over daemon-private IPC. A Runner
does not span several Devices; a Run composes work across several Runners and
Devices.

A Runner uses one lifecycle policy:

- `ephemeral`: stop after the last Run releases it;
- `unless-idle`: stop after it has no active Run leases and no active
  operations for the configured idle timeout;
- `unless-shutdown`: do not stop automatically when idle; remain ready until
  explicit stop or Device/daemon shutdown.

For lifecycle decisions, a Runner is idle only when both
`active_run_leases == 0` and `active_operations == 0`. An `unless-idle` Runner
starts its idle timeout after both conditions become true; becoming idle does
not stop it immediately. A Runner owns runtime resources such as Driver handles,
app state, OCR engines, or inference model sessions. The daemon owns its
creation, readiness, health, capacity, draining, routing, and termination.

Application/game implementations such as NetEase Music or Balatro may provide
RunnerClasses. Their CLI plugins remain separate frontend processes even when
both roles reuse the same Rust package and typed service implementation.

## Runner lease and claim

A Runner lease attaches a Run to a Runner and participates in retention and
capacity. It is not caller identity or an authentication credential. Stopping
a Run releases its leases; `unless-idle` and `unless-shutdown` policy decides
whether the Runner remains ready.

A Runner claim is a provisional typed scheduling request that describes Device
selection, required services, RunnerClass or label constraints, resources,
reuse policy, lease deadline, and concurrency. The daemon may bind it to a
ready compatible Runner or create another one.

## Capability manifest

A capability manifest is the authoritative set of versioned protobuf services
and methods a ready Runner has registered and is allowed to implement. Labels
may mirror capabilities for selection, but caller-writable labels do not prove
service availability or authority.

The daemon derives the manifest from validated protobuf descriptors,
RunnerClass policy, and health. Service registration is flat; generated clients
and domain facades may organize calls hierarchically.

AUV method annotations do not grant capabilities or authority. An optional
`discoverable` marker includes a concrete typed RPC in DevTools, Inspector,
capability panels, and future JavaScript REPL discovery. Its required effect
classification distinguishes read-only, mutation, and input-delivery calls for
tool presentation and confirmation. Unannotated RPCs remain callable through
their generated typed clients. Device authentication/authorization and optional
Run association are independent of these developer-tool annotations.

## Driver API

The Driver API is the typed protobuf projection of `auv-driver` capabilities.
Portable services use the `auv.api.driver.v1` package. Platform-specific
services use `auv.api.driver.<platform>.v1`, such as
`auv.api.driver.macos.v1`; they extend the available service set rather than
inherit from or reinterpret a portable service. Device, Run, and Runner
control-plane resources remain in `auv.api.core.v1`.

A Runner advertises the exact portable and platform Driver services/methods it
implements. Calling a method that the selected Runner or platform does not
implement returns gRPC `UNIMPLEMENTED`. Missing OS permission, temporary
unavailability, missing target resources, and invalid requests are distinct
failures and do not use `UNIMPLEMENTED`.

Reusable image values such as image frames, sizes, pixel formats, pixel regions,
and normalized regions belong to `auv.api.image.v1`; that package does not own
capture, recognition, or inference service behavior. Screen/window coordinate
types belong to `auv.api.driver.v1`. AUV does not define a global `geometry`
package or a coordinate-free `Rect`: screen, window, image-pixel, normalized,
and inference-specific geometry remain distinct types unless their semantics
and compatibility requirements are the same.

## Principal

A principal is the authenticated authority identity projected from transport
context before an API handler executes. It is not a caller-provided protobuf
field, Device ID, Run ID, Runner lease, or internal session.

Local owner-checked transport and paired mutual TLS are current ways to derive a
principal. The target authorization decision includes Principal, target
Device, optional Run, selected Runner, and protobuf service/method. The daemon
performs this check before forwarding to private Runner IPC. Runner processes
do not repeat external authentication.

## Span

An AUV span is an optional timed diagnostic scope inside a run. Spans may form
a tree through `parent_span_id`. An operation scope is one ordinary caller-named
use of this span API; spans need not belong to a persisted operation entity.
Span start and end are separate `TraceRecord` values. OpenTelemetry spans are
lossy exports, not AUV persistence identity.

## Event

An AUV event is an optional typed, timestamped point-in-time fact associated
with the current run and, optionally, a span. Events need not belong to a
persisted operation entity.

Examples include `command.resolved`, `driver.invoke`, `action.started`,
`artifact.captured`, `assertion.passed`, and `assertion.failed`.

An event record includes its canonical typed JSON payload. Events should
describe small occurrences; large payloads belong in artifacts.

## Artifact

An artifact is inspection, evidence, replay, or domain-output material
correlated with a run. A `TracingStore` writes its bytes and then emits an
artifact metadata record after validating the complete byte stream. This is a
simple write pipeline, not a run transaction or commit protocol.

Artifacts may optionally be associated with a span. V1 does not assign artifact
ownership to a persisted operation entity or verification. Artifacts may
contain structured JSON documents, images, reports, logs, media, or other files.
Byte emitters may declare a physical file extension in `EmitBytesOptions`;
file-backed stores may use it without changing the transport-independent
`ArtifactUri`.

Examples include screenshots, click-overlay images, accessibility snapshots,
driver input/output JSON, distillation reports, validation reports, and video
segments.

Typed facts and resources refer to artifacts through `ArtifactUri`.
An `ArtifactUri` is the transport-independent identity of an artifact. Spans
and events may add diagnostic links, but they do not own artifacts; large
payloads remain store-owned bytes rather than embedded event data.

## View Memory (retired experimental contract)

`ViewMemory` previously named an app-neutral structured payload derived from a
view-scan artifact for later target reacquisition. Its only application
consumer and the app-local artifact read/reuse seam were retired on 2026-07-25.
It is not a current runtime or frontend contract.

Candidate identifiers remain local observation facts. No current frontend may
carry a candidate or scan artifact URI into a later application call. A future
cross-run reacquisition contract requires owner approval and must live on the
shared runtime and inspection model rather than an app-local manifest,
artifact directory, process-global cache, or application-owned tracing-store
reader.

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

## Overlay Display

An overlay display is temporary visual feedback drawn over the live
desktop to make AUV's selected scope, target geometry, and recent operation
visible to a person. It is a trust and debugging surface, not an input delivery
backend, an observation artifact, or semantic verification.

An overlay display may follow a successful operation, such as outlining a
captured display or showing the delivered click point. Rendering success proves
only that the platform adapter rendered the requested visual layers. Overlay
unavailability or rendering failure should be reported separately and should
not change the underlying operation result.

## Overlay

An overlay is an ordered, renderer-independent collection of visual layers.
The initial shared layer vocabulary is `cursor`, `outline`, and `status`.
Cursor layers may use a built-in image or a runtime-provided SVG. A layer owns
content and geometry, while its typed style owns visual appearance; callers do
not manage native layer identifiers.

Outline and cursor labels retain content independently from visibility. Their
labels are hidden unless the caller explicitly enables label presentation.
Composite overlays keep target-outline and actor-cursor labels separate rather
than copying one string into both layers. Status text remains the body of its
layer and is visible whenever the status layer is present.

An overlay component is a reusable composition that expands into layers. The
initial shared components are `capture frame` and `click target`. Components
express visual composition only and do not perform capture or input delivery.

Overlay geometry uses the same screen coordinate contract as driver display,
window, and input results. `ShowOptions` separately defines motion and
lifecycle policy through `MotionOptions` and `LifecycleOptions`. The public
driver API shows or removes overlays; the platform overlay adapter renders or
removes their native layers. The adapter owns private layer identity, native
windows, animation timing, inherited starting positions, and rendering details.
Runtime and command frontends provide target state and display policy; they do
not drive animation frames across the platform seam.

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

The current typed values are `background_only`, `background_preferred`, and
`foreground_preferred`. They describe allowed ordering and disturbance policy,
not a promise that the selected path name will contain “background” or
“foreground”; the resulting `InputActionResult` remains authoritative for the
path actually used. Click cardinality is a separate option and may request a
single, double, or explicitly counted repeated click with an interval.

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

## Application Activation Result

Application activation result is the typed evidence returned after a platform
accepts a request to bring one application to the foreground. Request delivery
is not itself proof that the target became frontmost. The result therefore keeps
the requested application identity separate from post-activation verification.

Verification is `verified_foreground` only when a platform observation reports
the requested application as frontmost. A mismatched observation or unavailable
observation is an explicit activation-only outcome; callers must not promote it
to semantic success or recover that claim by taking an unrelated screenshot.

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

## Anchor

An anchor is a visible or native UI cue used to locate another observation or
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

## Inspect Server (legacy, not the inspector boundary)

The existing inspect server is not the future `auv-inspector` and must not
shape the producer-side tracing API. A later owner-approved inspector slice may
define ingestion, indexing, artifact resolution, subscriptions, and viewer
protocols over data written by `TracingStore`. No such API is part of the
current tracing contract.

## Interaction Instrumentation

An interaction is application- or runtime-owned orchestration that composes
multiple driver operations. Scroll scan is the motivating example: it observes
a surface, scrolls, observes again, and returns merged typed evidence.

The owning module keeps the control flow and direct result. It may instrument
the interaction with ordinary `auv-tracing` operation spans, typed events, and
artifacts. `auv-tracing` does not execute the interaction, and AUV does not
introduce a separate interaction tracing crate or recorder.

## Direct Operation Result

An operation's direct result is the app- or driver-owned Rust `Result<T, E>`
returned to its caller. It is not reconstructed from tracing and is not a
generic persisted `OperationResult` entity.

CLI and MCP adapters may call the same typed operation, but each frontend owns
its protocol-specific presentation. Enabling or disabling `auv-tracing` must
not change `T`, `E`, dispatch the operation again, or turn recording failure
into operation failure.

Delivery and semantic verification remain separate domain facts. A typed
`InputActionResult` can prove which input path was delivered without proving
the expected UI state; an app-owned typed result can state what was checked and
what the app observed without redefining the operation's Rust return type. V1
intentionally has no shared `VerificationResult`, `VerificationMethod`,
`OperationStatus`, `ControlFailure`, or persisted operation-result record in
`auv-tracing`.

## CLI Invoke Boundary

The CLI invoke boundary owns ad-hoc command invocation as a frontend capability.
It parses or receives invoke-style command ids and arguments, then routes to
typed handlers or temporary adapters without owning driver execution or run
recording. The current invoke redesign is intentionally breaking: legacy
bundle, recipe, skill, `debug.*`, `verify.*`, and app-specific `music.*`
command ids should not be retained as executable compatibility aliases.

The crate for this boundary is `auv-cli-invoke`. It owns invoke command
registration, typed clap arguments, and help rendering. Commands are organized as
a domain-owned command tree: each domain exposes its own group or subtree, while
the registry composes groups and flattens them for lookup. Command declarations
are handler-first: the annotated handler function generates the invoke command
export, so command id, typed arguments, inline examples, and handler identity
stay together. Natural primary operands are positional. CLI argv is parsed by
the command-local clap type, while MCP decodes protocol fields directly into
the same input type without reconstructing argv. Atomic driver capabilities remain typed APIs in their owning
driver crates; V1 does not reserve an unused cross-driver operation metadata
schema. The CLI boundary is not the core runtime and should not own run
recording semantics, recipe execution, or bundle discovery.

An invoke handler returns its direct value or error independently from run
recording. `InvokeCommandOutput` is the registered command's cross-frontend
direct-result envelope: it may carry the typed result projection and owned
artifact receipts, plus an optional CLI report. CLI rendering consumes that
report; MCP consumes only the direct result and does not reconstruct it from
trace records. The envelope does not carry a generic summary, backend, notes,
known-limits, verification, or signal bag. Command failure is the `Err` branch
rather than a successful output with an optional failure field. The MCP adapter
decodes protocol fields into the same command-local input type and invokes the
same registered handler without simulating argv. It disables incidental live
overlay presentation; explicit `overlay.*` commands remain visual operations by
definition. Artifact discovery and reading remain a separate inspector
boundary.

## Historical Terms

Before V1, AUV experimented with persisted operation/execution entities,
recorder fan-out, and a driver-specific tracing boundary. Those surfaces are
retired rather than compatibility-supported. The repository audit and migration
record remain in
[`docs/ai/references/inspect/2026-07-17-auv-tracing-contract-and-invoke-output-design.md`](ai/references/inspect/2026-07-17-auv-tracing-contract-and-invoke-output-design.md).

## Viewer

A viewer is a future UI over inspector-owned read models. It must not read
through the producer-side `TracingStore` trait.

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
page records, raw row observations, conservative clusters, directional
scroll-boundary candidates, stop evidence, and a completeness claim.

Observed collections are evidence artifacts. They are not application-specific
semantic objects such as playlists, search results, inboxes, or tables.
Retired recipe hooks and unimplemented section-candidate states are not reserved
as empty fields in this payload. A future typed app composition must add only
the states its producer actually emits.

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

## Semantic Verification

Semantic verification is an application-owned typed result stating what an
operation checked and what application state it observed. Input delivery,
state change, content matching, scan completeness, and app-specific failure
reasons are distinct facts; `auv-tracing` does not merge them into a universal
status, method taxonomy, or optional-field record.

An application may return its verification type directly and may emit the same
type in an application-owned tracing event. Artifacts referenced by that event
remain canonical run artifacts. Inspect reads the event schema and payload; it
does not infer semantic success from span completion, input delivery, or a
generic verification projection.

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

New scroll-scan work should use typed Rust composition and ordinary
`auv-tracing` instrumentation. Do not add new recipe-manifest hook execution.

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

Future list-scan behavior should live in typed Rust orchestration with optional
`auv-tracing` instrumentation, not recipe logic.

## Tombstone

A tombstone is a short file or module-level comment left after a path is removed
or archived. It contains no execution logic, names the removed path, points to
the replacement owner, and states the exact condition for deleting the
tombstone.
