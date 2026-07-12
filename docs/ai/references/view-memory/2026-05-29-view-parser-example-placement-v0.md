# AUV View Parser Example Placement v0

Date: 2026-05-29

Status: v0 placement spec. Pins where view parser code (framework
side) and the NetEase example (consumer side) live in the workspace,
what each may import, and what each must not touch.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
about to create the view parser crate, the NetEase example crate, or
modify either's `Cargo.toml`.

## Purpose

The view parser design draws a firm framework / example boundary, but
the line is described in prose. Without a placement spec, an
implementer choosing where to put a file has to guess between:

- a new `crates/auv-view` crate vs adding modules under `src/`
- a NetEase example as a single `examples/*.rs` file vs a full crate
- where projection record types (`PlaylistSidebarProjection`) live
- whether the example may import workspace-private modules

Two implementers will land at different layouts; refactoring later is
expensive because every consumer file has to move.

This spec closes those questions for v0.

## Relationship to other specs

```text
view-parser-ir-netease-playlist-example-design.md   framework / example boundary
view-parser-contract-bridge-v0.md                    must-use existing contracts
view-parser-ir-shapes-v0.md                          IR types
view-parser-diagnostic-policy-v0.md                  diagnostic rules
view-parser-merge-fixtures-v0.md                     merge acceptance tests
view-parser-trace-layout-v0.md                       span tree + signals
view-parser-example-placement-v0.md  (this doc)      crate locations + import rules
```

## The two crates

v0 introduces exactly two new workspace members:

| Crate | Location | Role |
|---|---|---|
| `auv-view` | `crates/auv-view/` | Generic view parser IR types, merge engine, diagnostic emission, trace layout helpers |
| `auv-example-netease-playlist` | `crates/auv-example-netease-playlist/` | NetEase-specific parsers, projection types, CLI binary |

Both are workspace members of the root `Cargo.toml`. Neither is
re-exported by `src/lib.rs` (the top-level `auv-cli` crate); they
stand independently.

If a future example needs to be added (e.g. another app), it follows
the same pattern: `crates/auv-example-<app>-<surface>/`. The example
crate name encodes app and surface so the workspace grows
horizontally rather than nesting.

## `auv-view` crate structure

```text
crates/auv-view/
  Cargo.toml
  src/
    lib.rs            // public re-exports of the IR types
    types/
      mod.rs
      ids.rs          // ViewNodeId, ViewCandidateId, ViewEvidenceId, ObservationIndex, ViewportFingerprint
      bounds.rs       // CoordinateSpace, ViewBounds, ScrollOffset, ScrollAxis
      scope.rs        // ViewScope, ViewRegion, ViewViewport
      observation.rs  // ViewObservation, ViewEvidenceNode, EvidenceSource
      candidate.rs    // ViewCandidate, Confidence, ConfidenceLevel
      node.rs         // ViewNode, ViewNodeKind, ViewLayout, ViewCapability, ViewScrollable, ScrollBoundary, BoundaryState
      anchor.rs       // ViewAnchor, ReacquireStrategy, ViewLandmark, LandmarkPurpose
      action.rs       // ViewAction, ViewActionTarget, ViewActionTargetKind
      reconstruction.rs // ViewReconstruction, ParserDiagnostic, ParserDiagnosticKind
      projection.rs   // ViewProjection<P>
      evidence_ref.rs // EvidenceRef
    merge/
      mod.rs
      rules.rs        // the 5 merge rules
      engine.rs       // candidate -> node merge step
      fixtures/       // the 9 canonical merge fixtures (per merge-fixtures-v0)
    diagnostic/
      mod.rs
      policy.rs       // firing-rule helpers per diagnostic-policy-v0
    trace/
      mod.rs
      spans.rs        // span name constants and attribute builders per trace-layout-v0
      artifacts.rs    // artifact role constants (view-observation etc.)
  tests/
    merge_fixtures.rs // runs the 9 fixtures
    json_roundtrip.rs // round-trip serialization for every top-level artifact
    id_derivation.rs  // determinism + Unknown non-stability
```

Notes on the structure:

- `lib.rs` re-exports the public surface; consumers do
  `use auv_view::{ViewNode, ViewReconstruction, ...}` without
  reaching into submodules.
- `merge/fixtures/` lives inside the crate (not under `tests/`) so the
  fixture functions can be reused across `tests/merge_fixtures.rs`
  and any later doc-example or property-test harness.
- `trace/spans.rs` exposes a typed builder so consumers cannot
  misspell `view.parse.observe.<index>`.

The crate's `Cargo.toml` depends on:

- `auv` (workspace, internal — for `crate::contract::{ArtifactRef,
  CandidateQuery, SurfaceNode, RecognitionResult, ...}`)
- `serde` + `serde_json` (for JSON serialization)
- `sha2` (for the SHA-256 used in ID derivation)
- Standard workspace dependencies (`anyhow`, `thiserror` per workspace
  conventions)

**It does not depend on** `auv-driver-macos`. The IR is platform-
agnostic; the example crate is responsible for ferrying driver outputs
into IR types.

## `auv-example-netease-playlist` crate structure

```text
crates/auv-example-netease-playlist/
  Cargo.toml
  src/
    lib.rs                // exposes parsers + projection types as library
    parsers/
      mod.rs
      app.rs              // NeteaseAppParser
      view.rs             // NeteaseSidebarViewParser
      region.rs           // SidebarRegionParser
      item.rs             // PlaylistSidebarItemParser
    projection/
      mod.rs              // PlaylistSidebarProjection, SidebarSection, PlaylistSidebarItem
      render.rs           // CLI renderer
    adapters/
      mod.rs
      ocr.rs              // ferry OCR results -> ViewEvidenceNode
      ax.rs               // ferry AX nodes -> ViewEvidenceNode
      capture.rs          // capture coordination
      scroll.rs           // scroll step + boundary detection
  src/bin/
    netease-playlist-ls.rs // the CLI binary
  tests/
    integration.rs        // ignored by default; smoke tests against live NetEase
    parser_fixtures.rs    // recorded-fixture parser tests
    rendering.rs          // CLI render snapshots
  tests/fixtures/
    sidebar-resized.json  // recorded OCR + capture for a resized sidebar
    sidebar-collapsed.json
    sidebar-modal.json
    sidebar-empty.json
    sidebar-multi-section.json
```

The binary `netease-playlist-ls` is the user-facing CLI. The library
(`lib.rs`) is what other example crates or future tests could import.

The crate's `Cargo.toml` depends on:

- `auv-view`
- `auv-driver-macos` (for OCR, capture, AX, window enumeration, scroll
  primitives)
- `auv` (workspace, internal — for the contract types reachable via
  the bridge spec rules)
- `serde` + `serde_json`
- Workspace standard dependencies

**It does not depend on** any other example crate. Examples are
siblings, not a tree.

## Import rules

### `auv-view` may import

- `auv`'s `crate::contract::*` types (per bridge spec): `ArtifactRef`,
  `CandidateQuery`, `SurfaceNode`, `RecognitionResult`,
  `VerificationResult` (read-only references — no construction of
  contract types except via documented constructors).
- `auv::trace::*` for trace-record-adjacent helpers when emitting
  spans (read-only).
- Standard / serde / hashing crates.

### `auv-view` may NOT import

- `auv-driver-macos` — the IR is platform-agnostic.
- `auv-overlay-macos`.
- Any example crate.
- Top-level `auv-cli` binary code.

### `auv-example-netease-playlist` may import

- `auv-view` (full public surface).
- `auv-driver-macos` (capture, OCR, AX, window, scroll APIs).
- `auv` (the contract types, per the bridge rule that example code
  consumes existing types rather than reinventing).
- Standard / serde / hashing crates.

### `auv-example-netease-playlist` may NOT

- Modify `src/contract.rs` (per bridge spec).
- Add commands to `catalog.rs` (per design doc Non-Goals).
- Add NetEase-specific types to `auv-view` (per IR shapes Non-Goals).
- Touch `auv-overlay-macos`.
- Re-export contract types under its own namespace.
- Depend on another example crate.

## Where domain types live

`PlaylistSidebarProjection`, `SidebarSection`,
`PlaylistSidebarItem`, and any other NetEase-specific record types
live in `auv-example-netease-playlist/src/projection/mod.rs`. They
**do not** appear in `auv-view`.

When the projection is serialized as a `view-projection-netease`
artifact, the generic record (`ViewProjection<P>`) comes from
`auv-view` and the type parameter `P` resolves to the NetEase-side
type. Serialization works through `serde` without `auv-view` ever
naming the NetEase type.

## CLI binary placement

`netease-playlist-ls` is a binary in
`crates/auv-example-netease-playlist/src/bin/`. It is **not**:

- a command in the top-level `auv-cli` catalog
- a recipe in `recipes/`
- a retired bundle-era manifest

Invocation: `cargo run -p auv-example-netease-playlist --bin
netease-playlist-ls -- <args>`. v0 does not add a shorter alias.

## Workspace `Cargo.toml`

Two members are added to the workspace:

```toml
members = [
  # existing members…
  "crates/auv-view",
  "crates/auv-example-netease-playlist",
]
```

No other workspace-level config (dependency overrides, feature flags,
profiles) is added for v0.

## Test placement

Per-crate:

- `auv-view/tests/` — IR-level tests: merge fixtures, JSON round-trip,
  ID derivation, span builder correctness, diagnostic policy
  conformance.
- `auv-example-netease-playlist/tests/` — example-level tests:
  recorded-fixture parser runs, CLI render snapshots, optional ignored
  integration tests against live NetEase.

Cross-crate integration tests (e.g. "the example uses the merge
fixtures correctly") live in the example crate, not in `auv-view`.

Workspace-level tests are not added in v0.

## v0 done criteria

The placement is v0-complete when:

1. `crates/auv-view/` and `crates/auv-example-netease-playlist/` exist
   and are members of the workspace.
2. `auv-view` compiles without any `auv-driver-macos` dependency,
   direct or transitive through workspace internal paths.
3. `auv-example-netease-playlist` depends on `auv-view` and
   `auv-driver-macos`; no other example crate depends on it.
4. The binary `netease-playlist-ls` builds with `cargo build -p
   auv-example-netease-playlist --bin netease-playlist-ls`.
5. `cargo check --workspace` passes with both new crates in.
6. No NetEase-specific identifier appears in `auv-view`'s public API.
7. The example's domain types resolve through `ViewProjection<P>`
   without requiring `auv-view` to name the type.

## Forbidden in v0

- Placing NetEase code outside the example crate.
- Adding NetEase-specific identifiers (constants, type names, signal
  strings) into `auv-view`.
- Re-exporting `auv-view` types through `auv-example-*` to "make
  imports shorter".
- Adding `auv-driver-macos` as a dependency of `auv-view`.
- Adding `auv-overlay-macos` as a dependency of either crate.
- Making `netease-playlist-ls` a member of the `auv-cli` catalog.
- Treating `examples/` (the single-file directory) as the home for
  the NetEase code. v0 uses crates only; single-file examples are
  reserved for trivial demos and are not subject to this spec.

## Non-goals for this spec

Intentionally deferred:

- Adding a second example crate. v0 ships NetEase only; the second
  example (when it exists) confirms or revises the placement pattern.
- Promoting parts of `auv-example-netease-playlist` into a shared
  `auv-example-common` crate. v0 keeps the example self-contained;
  shared helpers wait for a second consumer.
- Replacing the binary with a library exposed through `auv-cli`. v0
  keeps the example off the core catalog per the design doc.
- Cross-platform variants of the example. v0 is macOS only.

## How to use this spec

When creating files, before saving anything outside `crates/auv-view/`
or `crates/auv-example-netease-playlist/`:

- Re-read the import rules above. If a file would need an import that
  the rules forbid, stop and file a gap.
- If a type wants to be in both crates, it belongs in `auv-view`
  **only** if it is platform- and domain-agnostic. Otherwise it
  belongs in the example.
- If you find yourself wanting to add the example as a workspace
  command alias, you have crossed the boundary. Use the explicit
  `cargo run -p ...` form.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
