# AUV Inspect Server Crate And Viewer Spec

Date: 2026-07-10

Status: implemented

Owner approval: implemented in branch `inspect-server-viewer`

## Summary

This spec records the split of the inspect HTTP/WebSocket server and
viewer assets out of the root `auv-cli` crate into a dedicated
`crates/auv-inspect-server` crate. The same slice also replaces the former
single-file `src/inspect_server_viewer.html` authoring model with a Vite,
Vue, and TypeScript frontend that builds into static assets served by the
Rust inspect server.

The goal is not to change inspect behavior. The goal is to move the
viewer-facing storage API behind a clearer crate boundary and make the
frontend maintainable without expanding the inspect contract.

## Current Evidence

Before this split, the inspect surface was spread across these root-crate
files:

- `src/inspect_server/`: Axum routes, live WebSocket stream, write endpoints,
  session discovery, static viewer serving, design asset serving, and tests.
- `src/inspect_server_viewer.html`: a single HTML file with inline CSS and
  JavaScript. It is currently about 4.4k lines.
- `src/inspect_view_parser.rs`: thin wrappers over `view_parser_read` for
  inspect text and HTTP consumption.
- `src/inspect_scene_state.rs`: thin wrappers over `scene_state_read` for
  text inspect consumption.

The final implementation removes the server/viewer files and also deletes the
two thin read wrapper modules. Text inspect now calls `view_parser_read` and
`scene_state_read` directly from `src/inspect.rs`; the root projection adapter
calls `view_parser_read` directly.

`src/inspect_server/mod.rs` already documents its boundary as a viewer-facing
storage API. It does not execute commands and does not perform UI automation.
That matches the AUV architecture rule that inspection/viewer APIs read durable
run data and artifacts instead of depending on transient CLI behavior.

The blocker to a mechanical move was dependency direction. The old server
module imported root-crate modules:

- `crate::contract::{ObservationSnapshot, VerificationResult}`
- `crate::model::{AuvResult, now_millis}`
- `crate::run_read::{...}`
- `crate::inspect_view_parser::build_view_parser_inspect_for_run`

The split should therefore include a narrow read-side boundary instead of
copying root-crate internals into the new crate.

## Non-Goals

- No new inspect routes unless required to preserve existing behavior.
- No schema changes to run records, trace records, artifacts, or command
  results.
- No runtime, driver, or command execution changes.
- No candidate-action or archived AX copilot surface expansion.
- No scene-state viewer expansion beyond preserving existing text/API behavior.
- No UI redesign as part of the first migration.
- No public package publishing setup.

## Proposed Crate Boundary

Add:

```text
crates/auv-inspect-server/
  Cargo.toml
  src/
    lib.rs
    server.rs
    session.rs
    read_projection.rs
    viewer_assets.rs
  viewer/
    package.json
    tsconfig.json
    vite.config.ts
    index.html
    dist/
    src/
      main.ts
      App.vue
      viewer.ts
      styles/viewer.css
      ...
```

The root `auv-cli` crate should keep command parsing and process orchestration:

```text
auv inspect serve
  -> resolve store root
  -> create LocalStore and BroadcastRunRecorder
  -> resolve write token
  -> auv_inspect_server::serve(...)
```

The new crate should own:

- `InspectServeConfig`
- `InspectWriteConfig`
- inspect session descriptor read/write helpers
- Axum router construction
- route handlers
- WebSocket streaming over `BroadcastRunRecorder`
- write endpoint conflict handling
- static viewer asset serving
- route-level tests that do not need CLI parsing

The root crate should continue to own:

- CLI argument parsing
- `resolve_inspect_server_target`
- command-specific inspect client wiring
- runtime construction
- app/domain read projections that are not yet shared crate contracts

## Read Projection Boundary

The server needs enriched `GET /runs/{run_id}` responses. Today those
enrichments are built by root-crate read helpers. Moving the server crate
should avoid a reverse dependency from `auv-inspect-server` back to `auv-cli`.

Use a small trait owned by `auv-inspect-server`:

```rust
pub type InspectResult<T> = Result<T, String>;

pub trait InspectReadProjection: Send + Sync + 'static {
  fn run_enrichment(
    &self,
    store: &LocalStore,
    run: &CanonicalRun,
  ) -> InspectResult<InspectRunEnrichment>;
}
```

`InspectRunEnrichment` should contain the existing response additions:

- `command_boundary_claims`
- `verifications`
- `observation_snapshots`
- `detector_recognition_lineage`
- `view_parser`
- `view_parser_summary`

Because `VerificationResult`, `ObservationSnapshot`, and
`DetectorRecognitionLineage` currently live in the root crate, the first split
should carry those root-only enrichments as `serde_json::Value` inside
`InspectRunEnrichment`. That keeps the HTTP response stable without making
`auv-inspect-server` depend on `auv-cli`. Strongly typed fields can replace
those JSON values later if the underlying contract types move to a shared crate.

The Minecraft quality baseline endpoint should be attached when the HTTP
server moves, not in the initial crate shell. It should use a generic
run-extension hook rather than a Minecraft-specific projection method. That
keeps the first boundary focused on run enrichment and avoids publishing
vertical-specific placeholder APIs before the route exists in the new crate.

For the first slice, define a root-crate adapter that calls existing helpers:

- `run_read::extract_verifications`
- `run_read::extract_observation_snapshots`
- `run_read::extract_detector_recognition_lineage`
- `view_parser_read::build_view_parser_inspect`

This keeps the server crate independent from root-crate internals while still
preserving the current HTTP shape.

When the existing Minecraft quality baseline HTTP route moves, preserve the
existing route by passing the extension key
`minecraft-quality-baseline-report` through a generic root adapter hook that
calls `run_read::quality_baseline_report_with_verdicts_for_run`.

NOTICE: A later slice may move stable read-side projection code into a shared
crate. That is deferred until at least two non-CLI consumers need the same
projection without root-crate adapters.

## Viewer Frontend

Use Vite + Vue + TypeScript for the viewer authoring model.

Reasons:

- Vite has a small setup surface and first-party Vue TypeScript templates.
- The viewer is an interactive single-page app, not just static prose.
- TypeScript is useful because the inspect response shape is broad and has
  many optional sections.
- Vue fits the current viewer shape: list panes, detail panels, filters,
  derived badges, and artifact cards can become components without a large
  state-management framework.

Initial frontend layout:

```text
viewer/src/
  api/
    inspectApi.ts
    types.ts
  components/
    RunList.vue
    RunDetail.vue
    ArtifactPreview.vue
    ViewParserPanel.vue
  state/
    runs.ts
  styles/
    tokens.css
    viewer.css
  main.ts
  App.vue
```

The first migration should keep the UI behavior equivalent to the current
viewer. Component extraction should follow the existing visible surfaces rather
than introducing a redesign.

## Rust Static Asset Strategy

The Rust crate should serve built viewer assets from Vite output.

Recommended first implementation:

- `viewer/dist/index.html` is included with `include_str!`.
- `viewer/dist/assets/*` files are included with generated or explicit
  `include_bytes!` entries.
- `GET /` serves the built `index.html`.
- `GET /viewer-assets/{name}` serves the stable Vite asset filenames emitted
  by this first migration. Because the filenames are stable
  (`assets/viewer.js`, `assets/index.css`), the route uses `Cache-Control:
  no-cache` instead of immutable caching.
- Existing design SVG assets can either stay under `/assets/{name}` or be
  copied/imported into the Vite asset graph. Preserve current URLs during the
  first slice if that reduces churn.

NOTICE: Runtime filesystem serving of `viewer/dist` is intentionally omitted
from the first slice. The current binary ships a self-contained viewer, and
preserving that property avoids adding install-time asset path assumptions.

## Development Workflow

Add npm scripts under `crates/auv-inspect-server/viewer`:

```json
{
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "typecheck": "vue-tsc --noEmit"
  }
}
```

During frontend development, Vite can proxy inspect API calls to a running
Rust server:

```ts
server: {
  proxy: {
    "/runs": "http://127.0.0.1:8765",
    "/write": "http://127.0.0.1:8765",
    "/assets": "http://127.0.0.1:8765"
  }
}
```

Production serving remains Rust-owned.

## Testing

Rust tests:

- Move route tests from `src/inspect_server/mod.rs` to the new crate where
  possible.
- Keep CLI parsing tests in `auv-cli`.
- Add a regression test that `GET /` serves the built viewer HTML.
- Keep write security, conflict, artifact ambiguity, and WebSocket filtering
  tests under `auv-inspect-server`.
- Add one adapter-level root-crate test proving enriched `GET /runs/{id}`
  still includes the existing read-side fields.

Frontend tests:

- Preserve the current viewer self-test coverage as TypeScript unit tests where
  practical.
- Add a build check through `npm run build`.
- Keep route/API assumptions close to `api/types.ts` and `inspectApi.ts`.

Validation commands for the implementation slice:

```text
cargo fmt --check
cargo check
cargo test -p auv-inspect-server
cargo test -p auv-cli inspect
pnpm --filter auv-inspect-viewer build
git diff --check
cargo run --quiet -- invoke --help
```

## Migration Plan

1. Create `crates/auv-inspect-server` and move `session.rs` plus server route
   code behind the new crate API.
2. Introduce `InspectReadProjection` and a root-crate adapter so the server
   crate does not depend on `auv-cli`.
3. Update `Cargo.toml` workspace membership and root `auv-cli` dependencies.
4. Change `src/main.rs` and `src/cli.rs` references from
   `auv_cli::inspect_server` to `auv_inspect_server` where appropriate.
5. Move route tests to the new crate and leave CLI behavior tests in the root
   crate.
6. Add Vite + Vue + TypeScript viewer project with equivalent initial UI.
7. Replace `include_str!("../inspect_server_viewer.html")` with built Vite
   asset serving.
8. Remove root-crate inspect server modules only after the moved crate passes
   equivalent tests.

## Acceptance Criteria

- `auv inspect serve` starts the same local inspect server by default.
- Default host and port remain `127.0.0.1:8765`.
- Existing read endpoints keep their response shape.
- Existing write endpoint security behavior is preserved.
- Existing artifact lookup and scoped artifact behavior is preserved.
- The built binary still serves a viewer without requiring a separate frontend
  dev server.
- Frontend source is authored as Vue single-file components with TypeScript.
- The inline viewer HTML files (`src/inspect_server_viewer.html` and the
  temporary `crates/auv-inspect-server/src/inspect_server_viewer.html`) are
  removed after the Vite build is wired.
- No runtime, driver, command execution, or archived vertical behavior changes
  are included.

## Open Questions

- Should generated Vite asset include mappings be checked in, generated by
  `build.rs`, or maintained explicitly for the first slice?
- Should `InspectRunEnrichment` use strongly typed fields for every existing
  projection, or keep app-specific sections as `serde_json::Value` until a
  shared read-side crate exists?
- Should the frontend package manager be npm by default, or should the repo
  standardize on another tool before adding the first frontend package lock?
