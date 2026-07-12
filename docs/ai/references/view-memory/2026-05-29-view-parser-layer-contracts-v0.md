# AUV View Parser Layer Contracts v0

Date: 2026-05-29

Status: v0 contract spec. Pins the trait shape, input/output types,
adapter rules, and composition pattern for the four parser layers
described in the design doc.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
writing parser layers or the adapter code that ferries driver outputs
into IR evidence.

## Purpose

The design doc lists four parser layers:

```text
AppParser
  -> ViewParser
     -> RegionParser
        -> ItemParser
```

It describes each layer's responsibility in prose but does not pin
trait signatures, input shapes, output shapes, or how the layers
compose. Without that, two implementations land at different function
signatures, the composition wiring is bespoke, and the boundary
between "framework helper" and "domain logic" blurs.

This spec pins those for v0. It also pins the adapter rules for
turning existing `auv-driver-macos` outputs into `ViewEvidenceNode`
without inventing parallel evidence types.

## Relationship to other specs

```text
view-parser-ir-netease-playlist-example-design.md   layered parser model
view-parser-ir-shapes-v0.md                          IR types consumed below
view-parser-contract-bridge-v0.md                    SurfaceNode / RecognitionResult / ArtifactRef rules
view-parser-diagnostic-policy-v0.md                  diagnostic firing
view-parser-trace-layout-v0.md                       span tree + signals
view-parser-example-placement-v0.md                  which crate each trait lives in
view-parser-layer-contracts-v0.md  (this doc)        trait signatures + adapter rules
```

## Where each layer lives

Per the placement spec:

| Layer trait | Crate | Module |
|---|---|---|
| `AppParser` | `auv-view` | `parsers::app` |
| `ViewParser` | `auv-view` | `parsers::view` |
| `RegionParser` | `auv-view` | `parsers::region` |
| `ItemParser` | `auv-view` | `parsers::item` |

The **traits** live in `auv-view`. The NetEase **implementations** of
those traits live in `auv-example-netease-playlist/src/parsers/`. This
keeps the contract platform-agnostic and the example owning its
domain logic.

`auv-view/src/parsers/` is added on top of the layout in the placement
spec. It is a sibling of `auv-view/src/merge/`, not a child of
`types/`.

## Layer responsibilities (recap from design doc)

| Layer | Responsibility |
|---|---|
| `AppParser` | Confirm the target app/window state. Produce a `ViewScope`. |
| `ViewParser` | Drive the observation loop, scroll policy, merge, and projection. Confirm the expected view exists. |
| `RegionParser` | Resolve the target region inside the current viewport (e.g. sidebar bounds). |
| `ItemParser` | Parse one viewport's evidence into section + item `ViewCandidate`s. |

The view layer is the orchestrator. Region and item run per-observation.
App runs once at the start.

## Trait shapes

The four traits, with concrete input/output types. All types come from
`auv-view`; the `auv` references on the right are
`crate::contract` types per the bridge spec.

### `AppParser`

```rust
pub trait AppParser {
    /// Domain-specific application details. The view crate does not
    /// name this; only the implementation does (e.g. NetEaseAppDetails).
    type Details;

    /// One-shot: confirm the app, build the scope.
    fn parse_app(
        &self,
        input: AppParseInput,
    ) -> AuvResult<AppParseOutput<Self::Details>>;
}

pub struct AppParseInput {
    pub app_bundle_id: String,
    pub window_title_hint: Option<String>,
    pub run_context: DriverRunContext,
}

pub struct AppParseOutput<D> {
    pub scope: ViewScope,
    pub details: D,
    pub diagnostics: Vec<ParserDiagnostic>,
    pub artifacts: Vec<ArtifactRef>,
}
```

`AppParser::Details` is the generic seat for domain data (window
number, foreground state, foreground app pid, etc.) the lower layers
need without `auv-view` knowing what those fields look like.

### `ViewParser`

```rust
pub trait ViewParser {
    type AppDetails;
    /// Domain-specific view-level details (e.g. NetEase sidebar presence flags).
    type Details;
    /// Domain projection record type (e.g. PlaylistSidebarProjection).
    type Projection;

    /// Drives the observation loop. Returns the full parse output.
    fn parse_view(
        &self,
        scope: ViewScope,
        app_details: Self::AppDetails,
        run_context: DriverRunContext,
    ) -> AuvResult<ViewParseOutput<Self::Details, Self::Projection>>;
}

pub struct ViewParseOutput<D, P> {
    pub details: D,
    pub observations: Vec<ViewObservation>,
    pub reconstruction: ViewReconstruction,
    pub projection: Option<ViewProjection<P>>,
    pub diagnostics: Vec<ParserDiagnostic>,
    pub artifacts: Vec<ArtifactRef>,
}
```

The view parser owns the scroll loop. Implementers compose
`RegionParser` and `ItemParser` calls per observation pass, then call
`auv_view::merge::merge_candidates` to produce the reconstruction.

`projection` is `Option<...>` because not every view parser produces a
domain projection; rec-only runs return `None`.

### `RegionParser`

```rust
pub trait RegionParser {
    type AppDetails;
    type ViewDetails;

    /// One-pass: find the target region inside the current viewport.
    fn parse_region(
        &self,
        scope: &ViewScope,
        viewport: &ViewViewport,
        app_details: &Self::AppDetails,
        view_details: &Self::ViewDetails,
    ) -> AuvResult<RegionParseOutput>;
}

pub struct RegionParseOutput {
    pub region: Option<ViewRegion>,
    pub diagnostics: Vec<ParserDiagnostic>,
    pub artifacts: Vec<ArtifactRef>,
}
```

`region` is `Option<...>`. `None` plus a Fatal `RegionNotFound` or
`RegionCollapsed` diagnostic per the diagnostic policy is the canonical
"region failed" outcome — implementations must not `Err(...)` for
those.

### `ItemParser`

```rust
pub trait ItemParser {
    type AppDetails;
    type ViewDetails;

    /// One-pass: parse the current viewport's evidence into candidates.
    fn parse_items(
        &self,
        region: &ViewRegion,
        viewport: &ViewViewport,
        evidence: &[ViewEvidenceNode],
        app_details: &Self::AppDetails,
        view_details: &Self::ViewDetails,
    ) -> AuvResult<ItemParseOutput>;
}

pub struct ItemParseOutput {
    pub candidates: Vec<ViewCandidate>,
    pub diagnostics: Vec<ParserDiagnostic>,
}
```

`ItemParser` does not get a list of artifacts because every artifact
the items reference is already in the evidence nodes' source refs. It
also does not produce artifacts of its own — the observation it
contributes to owns the artifact.

## How the layers compose

The reference composition pattern lives in the example. v0 does not
provide a default `compose_layers` helper — composition is concrete
enough to write inline, and a helper would constrain
non-NetEase examples prematurely.

The composition skeleton:

```rust
let scope_output = app_parser.parse_app(input)?;
let view_output = view_parser.parse_view(
    scope_output.scope.clone(),
    scope_output.details,
    run_context,
)?;
```

Inside `parse_view`, the per-observation loop:

```rust
loop {
    let viewport = current_viewport()?;
    let region_output = region_parser.parse_region(&scope, &viewport, &app_details, &view_details)?;

    if let Some(region) = region_output.region {
        let evidence = collect_evidence(&region, &viewport, &run_context)?;
        let items = item_parser.parse_items(&region, &viewport, &evidence, &app_details, &view_details)?;
        observations.push(build_observation(viewport, evidence, items.candidates));
    } else {
        // Fatal RegionNotFound / RegionCollapsed already in region_output.diagnostics
        break;
    }

    if should_stop_scrolling(&observations) { break; }
    scroll_step(&scope)?;
}

let reconstruction = merge_candidates(&observations);
let projection = build_projection(&reconstruction);
```

The boundary policy:

- **App-level state** flows downward as `AppDetails`.
- **View-level state** flows downward as `ViewDetails`.
- **Region** is rebuilt each observation (it may move).
- **Items** are observation-scoped.
- **Reconstruction** crosses observations and lives at the view layer.
- **Projection** is built from the reconstruction, also at the view
  layer.

## Adapter rules (driver outputs → `ViewEvidenceNode`)

Adapters live in the **example crate**, not in `auv-view`, because
they import `auv-driver-macos`. The placement spec forbids
`auv-view` from depending on `auv-driver-macos`.

`auv-view` provides only the IR-side helpers that consume already-
contract-typed evidence. Example flow:

```text
auv-driver-macos::observe::find_screen_text(...)
  → some driver-typed result
        ↓ (adapter, in example crate)
auv::contract::RecognitionResult
        ↓ (helper, in auv-view)
Vec<ViewEvidenceNode>
```

### `auv-view`-side helpers

`auv-view::adapters::evidence` exposes the platform-agnostic
converters. They take contract types and produce
`ViewEvidenceNode`s:

```rust
pub fn evidence_from_ocr(
    result: &RecognitionResult,
    viewport: &ViewViewport,
    source_ref: ArtifactRef,
) -> Vec<ViewEvidenceNode>;

pub fn evidence_from_ax(
    nodes: &[SurfaceNode],
    viewport: &ViewViewport,
    source_ref: ArtifactRef,
) -> Vec<ViewEvidenceNode>;

pub fn evidence_from_icon(
    matches: &[IconMatch],
    viewport: &ViewViewport,
    source_ref: ArtifactRef,
) -> Vec<ViewEvidenceNode>;
```

These helpers:

- Set `EvidenceSource` correctly (`Ax` / `Ocr` / `IconMatch`).
- Translate bounds into the viewport's `ViewBounds` coordinate space.
- Carry the artifact ref into each emitted `ViewEvidenceNode`.
- Serialize the raw payload as the contract type (per bridge spec:
  `Ax` payload is `SurfaceNode`, `Ocr` payload is `RecognitionResult`).
- Derive `ViewEvidenceId` per the IR shapes ID rules.

`IconMatch` is illustrative; the v0 IR's `EvidenceSource::IconMatch`
variant is already defined, and the helper signature lands when an
example exercises icon matching. Until then, the helper is reserved
but unimplemented in v0.

### Example-side adapters

`auv-example-netease-playlist/src/adapters/` holds the
platform-specific bridges:

```rust
// adapters/ocr.rs
pub fn capture_and_ocr_region(
    scope: &ViewScope,
    viewport: &ViewViewport,
    run_context: &DriverRunContext,
) -> AuvResult<(RecognitionResult, ArtifactRef)>;

// adapters/ax.rs
pub fn capture_ax_for_region(
    scope: &ViewScope,
    viewport: &ViewViewport,
    run_context: &DriverRunContext,
) -> AuvResult<(Vec<SurfaceNode>, ArtifactRef)>;

// adapters/capture.rs
pub fn capture_viewport(
    scope: &ViewScope,
    viewport_bounds: &ViewBounds,
    run_context: &DriverRunContext,
) -> AuvResult<(ArtifactRef, ViewportFingerprint)>;

// adapters/scroll.rs
pub fn scroll_region(
    scope: &ViewScope,
    region: &ViewRegion,
    axis: ScrollAxis,
    delta_logical: i32,
    run_context: &DriverRunContext,
) -> AuvResult<ScrollStepResult>;

pub struct ScrollStepResult {
    pub before_viewport: ViewportFingerprint,
    pub after_viewport: ViewportFingerprint,
    pub artifacts: Vec<ArtifactRef>,
}
```

These adapters use `auv-driver-macos` primitives and return:

- contract types (`RecognitionResult`, `SurfaceNode`) — never driver-
  specific intermediate shapes
- `ArtifactRef`s pointing at the driver-produced artifacts (capture,
  OCR result, AX dump)

The adapters compose with `auv-view`'s `evidence_from_*` helpers to
produce `Vec<ViewEvidenceNode>` for `collect_evidence`.

## Where domain types attach

The associated types on the traits (`AppParser::Details`,
`ViewParser::AppDetails`, etc.) are the seat for NetEase-specific
data. Each implementation declares them concretely:

```rust
// auv-example-netease-playlist/src/parsers/app.rs
pub struct NeteaseAppDetails {
    pub window_number: u32,
    pub foreground: bool,
    pub sidebar_present: bool,
}

impl AppParser for NeteaseAppParser {
    type Details = NeteaseAppDetails;
    fn parse_app(...) -> ... { ... }
}
```

Type parameters propagate through the composition chain via the
associated types. Implementers must not erase them into
`serde_json::Value` or `Box<dyn Any>` to "make composition flexible".
The point of the typed associated-types is that `auv-view` stays
domain-agnostic without losing the type information consumers need.

## Error vs diagnostic propagation

Per `view-parser-diagnostic-policy-v0.md`:

- Layer functions return `AuvResult<...>`.
- `Err(...)` is reserved for infrastructure failures (driver capture
  errored, OS API failed). It bubbles unchanged through all layers.
- Observed failures (`RegionNotFound`, `RegionCollapsed`,
  `ModalBlocked`) return `Ok(...)` with a Fatal `ParserDiagnostic`
  and a parallel `known_limits` entry on the enclosing reconstruction.
- Non-fatal diagnostics accumulate on the layer's output (e.g.
  `RegionParseOutput.diagnostics`) and are aggregated into the
  reconstruction's `diagnostics` by the view layer.

The view layer is responsible for de-duplicating diagnostics it sees
from the same target across multiple observations only when aggregation
rules in the policy spec allow it. By default, every diagnostic from
every observation is preserved.

## Span and artifact responsibility per layer

Per `view-parser-trace-layout-v0.md`:

| Layer | Owns the span | Emits the artifact |
|---|---|---|
| `AppParser` | n/a (runs within the root `view.parse.<scope_id>` span) | optional driver evidence |
| `ViewParser` | `view.parse.observe.loop`, `view.parse.reconstruct`, `view.parse.project.<domain>` | `view-reconstruction`, `view-projection-<domain>` |
| `RegionParser` | `view.parse.region_detect` | optional driver evidence (capture, OCR for the detection step) |
| `ItemParser` | n/a (contributes to the observation span owned by the view layer) | none directly |
| Per-observation collector inside the view layer | `view.parse.observe.<index>` | `view-observation` |

`auv-view::trace::spans` exposes typed builders so implementers do
not free-text the span names.

## v0 done criteria

The layer contracts are v0-complete when:

1. All four traits exist in `auv-view::parsers` with the signatures
   above.
2. NetEase implementations of all four traits exist in
   `auv-example-netease-playlist/src/parsers/`.
3. The `evidence_from_ocr` and `evidence_from_ax` helpers exist in
   `auv-view::adapters::evidence` and produce
   `Vec<ViewEvidenceNode>` from contract types.
4. The example's adapter modules (`adapters/ocr.rs`, `ax.rs`,
   `capture.rs`, `scroll.rs`) return contract types and
   `ArtifactRef`s, not driver-specific intermediate shapes.
5. The composition skeleton runs end-to-end against at least one
   recorded fixture (no live NetEase required) and produces a valid
   `ViewReconstruction` with at least one `ViewObservation` artifact.
6. No layer trait returns `Err(...)` for `RegionNotFound`,
   `RegionCollapsed`, or `ModalBlocked` — those route through
   `Ok(...)` with Fatal diagnostics.
7. The associated types propagate through the composition without
   any `Box<dyn Any>` or `serde_json::Value` erasure.
8. `auv-view` has no transitive dependency on `auv-driver-macos`.

## Forbidden in v0

- Adding a fifth parser layer ("ScopeParser", "WindowParser", etc.).
  v0 is exactly four layers.
- Returning `Result<Output, ParserDiagnostic>` instead of
  `AuvResult<Output>` with diagnostics in the output. The latter
  preserves the Ok-plus-diagnostic pattern.
- Replacing the associated types with `Box<dyn Any>` or
  `serde_json::Value` to "make composition more flexible". The
  associated types are the type-safety contract.
- Allowing a `RegionParser` or `ItemParser` to emit a
  `view-reconstruction` or `view-projection-*` artifact. Those belong
  to the view layer only.
- Allowing an `auv-view`-side function to import `auv-driver-macos`
  to "make the adapter easier". The dependency edge is forbidden by
  the placement spec.
- Re-implementing `evidence_from_ocr` / `evidence_from_ax` in the
  example crate. Adapter helpers live in `auv-view`; the example only
  provides the platform call sites.
- Erasing the typed `AppDetails` / `ViewDetails` into the
  reconstruction (e.g. via `attributes: BTreeMap<String, String>`)
  just because the IR doesn't carry them.

## Non-goals for this spec

Intentionally deferred:

- A default `compose_layers` helper. v0 ships explicit composition in
  the example; a helper waits for a second example to validate the
  shape.
- Async trait variants. v0 traits are sync; an async layer can be
  introduced when a real need exists.
- Mid-loop reconfiguration (e.g. switching strategies between
  observations). v0 treats one parse run as one configured pipeline.
- Per-layer feature flags. The example builds with the layer set it
  declares.
- A generic `Parser` super-trait that all four implement. The four
  layers have distinct shapes; a super-trait would be cosmetic.

## How to use this spec

When writing or reviewing parser code:

- Look up the layer's trait above before writing the function
  signature.
- When an implementation needs more data, extend the associated types
  via this document — not the trait signature directly.
- When evidence cannot be expressed by `RecognitionResult`,
  `SurfaceNode`, or the existing `EvidenceSource` variants, file a
  gap before introducing a parallel evidence type.
- Composition glue belongs in the example crate. `auv-view` provides
  traits, types, merge engine, diagnostic helpers, and trace helpers;
  it does not provide a fixed composition.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
