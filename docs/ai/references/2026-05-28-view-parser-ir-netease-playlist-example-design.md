# AUV View Parser IR And NetEase Playlist Example Design

Date: 2026-05-28

Status: design draft approved for planning

## Purpose

This design defines the next AUV direction after capture fast path and
low-disturbance input work: a generic view parsing and reconstruction substrate,
validated by a NetEase Cloud Music playlist sidebar example.

AUV should not implement a universal Screen2AX parser and should not place
NetEase-specific logic in the framework. AUV should provide driver primitives,
generic observation contracts, structured artifacts, and orchestration
boundaries. App-specific parsing belongs in examples or automation layers built
on top of AUV.

The first vertical slice is an example CLI that performs a structured
`playlist ls` over the NetEase sidebar. It is a reference implementation and a
stress test for sectioned scrollable views, not a core AUV command.

## Non-Goals

- Do not add NetEase-specific commands to the core command catalog.
- Do not add NetEase-specific parsing logic to `auv`, `auv-driver-macos`, or
  shared runtime modules.
- Do not build a general-purpose screenshot-to-AX model.
- Do not require persistent app memory in the first slice.
- Do not require inspect viewer integration in the first slice.
- Do not batch-migrate the command catalog in this slice. Record framework gaps
  for later migration.

## Design Principles

- AUV is the framework. App/view parsers are consumers of AUV APIs.
- AX is one evidence source and compatibility target, not the only IR.
- OCR, icon matching, and future YOLO-like detectors are provider or pipeline
  middleware. Per-app/per-view parser code decides how to combine them.
- Parser-specific overfitting is acceptable when it is scoped to an app/view
  parser and based on regions, text, layout, and evidence rather than hardcoded
  absolute pixels.
- Structured output is the primary contract. Human CLI text is a renderer over
  structured artifacts.
- Uncertainty must be explicit. Boundary detection, section assignment, and
  item identity should report evidence and known limits rather than pretending
  to be certain.

## Terminology

Use the `View*` prefix for the view parsing and reconstruction layer.

| Term | Meaning |
|---|---|
| `ViewScope` | The app/view scope being parsed, such as a NetEase main window sidebar. |
| `ViewRegion` | A named region inside a scope, such as a sidebar body or search results body. |
| `ViewViewport` | The currently visible slice of a scrollable region. |
| `ViewObservation` | One observation pass over a viewport or region, including evidence. |
| `ViewTree` | The structured tree projected from one observation. It is evidence-local and not a complete view. |
| `ViewNode` | A generic node in a `ViewTree`, from AX, OCR, icon matching, visual segmentation, or parser logic. |
| `ViewCandidate` | A parser-local candidate found in one viewport, before cross-viewport merge or rejection. |
| `ViewSection` | A reconstructed section, such as feature navigation, playlist navigation, created playlists, or favorited playlists. |
| `ViewItem` | A reconstructed stable item, such as one playlist row. |
| `ViewReconstruction` | A scan result stitched from multiple observations. This is not a hash map or generic mapping table. |
| `ViewAnchor` | A reference attached to a node, section, or item that can later be reacquired when it is not currently visible. |
| `ViewLandmark` | A derived relocation signal used for viewport pose and reacquisition, such as a section header or nearby item label. |
| `ViewMemory` | Optional saved reconstruction, anchors, and landmarks for later commands. |
| `ViewAction` | A semantic action supported by a node or item. |
| `ViewActionTarget` | The concrete target for a view action, such as an AX path or window-local point. |

Layering:

```text
ViewScope
  ViewRegion
    ViewViewport
      ViewObservation
        ViewTree
          ViewNode
        ViewCandidate[]
  ViewReconstruction
    root_layout
      ViewSection
        section_anchor
        section_layout
          ViewItem
            item_layout
            item_anchors[]
            item_actions[]
    anchor_index
    landmark_index
  ViewMemory
```

`ViewTree` is one observed structure. `ViewReconstruction` is a scan-level
result. `ViewMemory` is optional reuse across commands. `anchor_index` and
`landmark_index` are lookup indexes over anchors and landmarks already attached
to layout, section, item, or evidence records; they are not the main business
hierarchy.

## V0 Layout And Node Taxonomy

V0 intentionally keeps the taxonomy small. The goal is to support the NetEase
sidebar example and leave room for later parsers without designing a complete
UI ontology.

Initial layout kinds:

| Kind | Meaning |
|---|---|
| `ScrollBody` | A scrollable view region that emits viewport observations. |
| `VStack` | A vertical stack, such as the sidebar list body. |
| `HStack` | A horizontal stack, such as one row's icon plus text. |
| `Group` | A fallback container when the parser has evidence for grouping but not a stronger layout. |

Initial semantic kinds:

| Kind | Meaning |
|---|---|
| `Text` | Text evidence from OCR, AX, or parser projection. |
| `SectionHeader` | A visible or reconstructed section boundary. |
| `ActionItem` | A row or item that can be selected or opened. |
| `Icon` | Optional visual/icon evidence. |
| `Unknown` | Evidence exists, but the parser cannot assign stronger semantics. |

Initial actions:

| Action | Meaning |
|---|---|
| `Open` | Open or activate an item. |
| `Select` | Select an item or navigation row. |
| `Scroll` | Scroll a `ScrollBody`. |
| `ObserveOnly` | No action is declared; the node exists as evidence. |

Additional roles and layouts such as `Button`, `Link`, `Input`, `Dialog`,
`FileDialog`, `Grid`, `Canvas`, `Table`, and modal/system surfaces are
follow-up work. They should be added only when a real parser or command needs
them.

A NetEase playlist row should therefore be expressible as:

```text
ViewItem(domain_kind=netease.playlist_item, semantic_kind=ActionItem)
  item_layout: HStack
    ViewNode(semantic_kind=Icon)?
    ViewNode(semantic_kind=Text, label=playlist_name)
  item_anchors[]
  item_actions[Open, Select]
```

A section should own its layout and may also have an anchor:

```text
ViewSection(kind=my_playlists)
  section_anchor?
  section_layout: VStack
    ViewItem(...)
    ViewItem(...)
```

`ViewLandmark` is not a native UI property. It is a reconstruction-level signal
derived from evidence, such as a stable section header, stable item label,
unique icon, viewport fingerprint, or first/last visible item sequence. One
node may be both an anchor target and a landmark.

## Framework Boundary

AUV framework code may provide app-agnostic contracts and helpers:

- capture, OCR, icon matching, scroll, AX capture, and input primitives from
  existing driver crates
- structured observation/result types when they are useful outside one app
- artifact writing and CLI rendering helpers
- evidence and diagnostics conventions

The NetEase example owns:

- resolving NetEase-specific windows and regions
- sidebar presence and resized-sidebar detection
- OCR search strings, spacing rules, and section labels
- row classification and section assignment
- scroll step policy for this sidebar
- human-readable playlist output

If the example reveals missing generic capability, record it as a framework gap
before moving it into AUV.

## Parser Shape

The example parser should be layered so future app parsers can follow the same
pattern without entering framework code.

```text
NeteaseAppParser
  -> NeteaseSidebarViewParser
  -> SidebarRegionParser
  -> PlaylistSidebarItemParser
```

Responsibilities:

- App parser: confirm the target app/window state.
- View parser: confirm the expected view exists and has a sidebar.
- Region parser: resolve the sidebar body region from current evidence. The
  sidebar may be resized, collapsed, or absent.
- Item parser: parse one viewport into section and item candidates.

Parsing inputs should prefer existing `auv-driver-macos` APIs:

- capture over the target window or region
- OCR over the sidebar region
- optional icon/template matching when it improves confidence
- AX evidence where it is already available and app-agnostic
- no hard dependency on a general visual model

## Structured Artifact V0

The example should emit a structured artifact first and render CLI text from
that artifact.

Suggested shape:

```rust
pub struct PlaylistSidebarScan {
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub sidebar_region: Option<ViewRegionRecord>,
  pub observations: Vec<SidebarViewportObservation>,
  pub sections: Vec<SidebarSection>,
  pub items: Vec<PlaylistSidebarItem>,
  pub boundary: ScrollBoundarySummary,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

pub struct SidebarViewportObservation {
  pub observation_index: usize,
  pub viewport: ViewViewportRecord,
  pub source_artifacts: Vec<ArtifactRefLike>,
  pub candidates: Vec<SidebarViewportCandidate>,
  pub viewport_fingerprint: String,
  pub parser_notes: Vec<String>,
}

pub struct PlaylistSidebarItem {
  pub item_id: String,
  pub label: String,
  pub section_hint: Option<SidebarSectionKind>,
  pub bounds: ViewBounds,
  pub source_text: String,
  pub observation_index: usize,
  pub confidence: Confidence,
  pub diagnostics: Vec<ParserDiagnostic>,
}
```

The exact Rust placement is part of implementation planning. If these types are
specific to NetEase, keep them in the example. Promote only app-agnostic pieces
after a second parser needs them.

## Sidebar And Scroll Requirements

The NetEase sidebar is a sectioned vertical scroll body. The example must handle
these cases explicitly:

| Case | Required Behavior |
|---|---|
| Sidebar resized | Detect sidebar region from evidence rather than absolute pixels. |
| Sidebar absent/collapsed | Exit with structured error and do not fake an empty playlist list. |
| Section header visible | Record section candidate and assignment evidence. |
| Section header not visible | Carry forward only when evidence supports it; otherwise mark items unassigned or uncertain. |
| Partial top/bottom rows | Record clipped/partial visibility and avoid treating clipped text as stable identity when possible. |
| Scroll top unknown | Continue only with `top=unknown` or `top=likely`, including evidence. |
| Scroll bottom unknown | Stop with uncertainty rather than claiming completion. |
| No movement after scroll | Record boundary or stuck evidence. |
| Repeated viewport | Use repeated viewport fingerprint as boundary/no-progress evidence. |

Boundary state should use explicit confidence:

```text
confirmed | likely | unknown | contradicted
```

## Memory And Anchors

Memory scope is not fixed in this design. It should be derived from the matching
and reacquisition algorithm once the first scan result exists.

V0 should preserve enough evidence for future reacquisition:

- item label
- section hint
- observation index
- nearby visible labels or section headers when available
- viewport fingerprint
- item bounds within the viewport/region
- source artifacts and diagnostics

Future `playlist get <anchor>` should reacquire before acting. It must not click
stale last-seen bounds directly.

## Error Handling

The example should return structured errors for:

- target window not found
- blocking modal or system dialog present
- sidebar region not found
- sidebar collapsed or too narrow to parse
- capture failure
- OCR failure
- scroll action failure
- parser produced no reliable candidates

Human CLI output may summarize these errors, but the structured artifact or
result should retain machine-readable diagnostics.

## Modal And System Surfaces

Modal and system surfaces are future work and should not be folded into the
NetEase sidebar v0 taxonomy.

Examples include:

- file open/save dialogs
- permission dialogs
- login dialogs
- context menus
- popovers

These surfaces should become separate `ViewScope`s or specialized parsers later,
often with AX-first behavior and stricter action safety. If the NetEase sidebar
example detects a modal or system dialog in v0, it should return a structured
blocker error instead of trying to continue the sidebar scan.

## Implementation Phases

Keep phases and commits small. Do not combine speculative framework changes
with NetEase parser logic.

1. Example artifact and result types.
2. One-viewport sidebar OCR parser using existing driver APIs.
3. Sidebar region detection and absent/collapsed sidebar errors.
4. Multi-viewport scroll loop with boundary evidence.
5. Human-readable CLI rendering over the structured result.
6. Follow-up documentation for inspect viewer integration and command catalog
   migration.

Command catalog migration is explicitly follow-up work. Scope future migration
by crate/module names, such as `auv-driver-macos`, `auv-cli`, and `contracts`,
not by broad platform labels.

## Testing Strategy

Start with parser-level fixtures and avoid requiring a live app for every test.

- Unit-test row classification and section assignment from OCR-like inputs.
- Unit-test sidebar region detection with recorded geometry fixtures.
- Unit-test viewport fingerprint and repeated-page detection.
- Unit-test structured artifact serialization.
- Add live NetEase smoke tests only as explicit/manual or ignored tests.

If a live run exposes a parser failure, preserve the capture/OCR evidence as a
fixture before changing parser logic.

## Open Investigation Items

These should be answered during implementation, not guessed in the spec:

- Which evidence best detects the resized sidebar: OCR anchors, visual boundary,
  AX nodes, or a hybrid?
- Which scroll top/bottom strategy works best for NetEase: viewport
  fingerprint, first/last known sections, no-new-items, scroll delta, or a
  hybrid?
- Which generic types should be promoted from the example after the first
  parser works?

## Follow-Ups

- Inspect viewer panel for View observations and reconstructions.
- Command catalog migration to expose newer `auv-driver-macos` capabilities.
- Persistent `ViewMemory` and anchor reacquisition.
- Optional icon/template detector integration for section/item confidence.
- Additional app/view examples to validate whether generic contracts should be
  promoted into framework code.
