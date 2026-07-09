# AUV Inspect Server Crate And Viewer Implementation Plan

Status: implemented on branch `inspect-server-viewer`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the inspect HTTP/WebSocket server into `crates/auv-inspect-server` and migrate the viewer authoring surface to Vite, Vue, and TypeScript while preserving the current inspect behavior.

**Architecture:** The new `auv-inspect-server` crate owns the viewer-facing storage API, write security, session descriptor, WebSocket stream, and static viewer serving. The root `auv-cli` crate keeps CLI parsing, runtime construction, and a small read-projection adapter that supplies root-crate enrichments to the server crate without creating a reverse dependency.

**Tech Stack:** Rust 2024, Axum, Tokio, `auv-tracing-driver`, `auv-view`, Vite, Vue 3, TypeScript, npm.

---

## Scope Check

The spec contains two connected subsystems: Rust crate extraction and frontend engineering. Keep them in one implementation branch, but finish them as separately testable phases:

1. Rust extraction preserves the current inline viewer exactly enough for existing route and viewer marker tests to pass.
2. Vite/Vue/TypeScript migration replaces the inline viewer authoring model after the extracted Rust crate is already working.

Do not redesign the viewer, add inspect routes, change trace schemas, move runtime code, or expand archived candidate-action surfaces.

## File Map

Create:

- `crates/auv-inspect-server/Cargo.toml`: crate manifest and dependencies for the server.
- `crates/auv-inspect-server/src/lib.rs`: public API exports and small shared result alias.
- `crates/auv-inspect-server/src/read_projection.rs`: `InspectReadProjection`, default empty projection, and enrichment response types.
- `crates/auv-inspect-server/src/server.rs`: moved Axum routes, write handling, live stream, static asset serving, and route tests.
- `crates/auv-inspect-server/src/session.rs`: moved inspect session descriptor code and tests.
- `crates/auv-inspect-server/src/inspect_server_viewer.html`: temporary crate-owned copy of the legacy viewer payload until Vite output replaces it; removed after Task 11.
- `crates/auv-inspect-server/src/viewer_assets.rs`: Vite build asset embed table after the frontend migration.
- `crates/auv-inspect-server/viewer/package.json`: npm scripts and frontend dependencies.
- `crates/auv-inspect-server/viewer/tsconfig.json`: TypeScript project configuration.
- `crates/auv-inspect-server/viewer/vite.config.ts`: Vite build and dev proxy configuration.
- `crates/auv-inspect-server/viewer/index.html`: Vite HTML entry.
- `crates/auv-inspect-server/viewer/src/main.ts`: Vue app entry.
- `crates/auv-inspect-server/viewer/src/App.vue`: initial viewer component produced from the existing HTML body.
- `crates/auv-inspect-server/viewer/src/legacy/viewer.ts`: initial TypeScript port of the existing inline script.
- `crates/auv-inspect-server/viewer/src/styles/viewer.css`: initial CSS port of the existing inline style.

Modify:

- `Cargo.toml`: add workspace member, add the root dependency on `auv-inspect-server` when the root crate starts using it, and remove root-only inspect server dependencies after migration.
- `src/lib.rs`: stop exporting `inspect_server`.
- `src/cli.rs`: import inspect server default host and port from `auv_inspect_server`.
- `src/main.rs`: use `auv_inspect_server` for serve/session discovery and define the root read-projection adapter.
- `docs/ai/references/INDEX.md`: add this plan to `core/inspect-trace`.

Remove after replacement is proven:

- `src/inspect_server/mod.rs`
- `src/inspect_server/session.rs`
- `src/inspect_server_viewer.html`
- `crates/auv-inspect-server/src/inspect_server_viewer.html`

Leave unchanged in this plan:

- `src/inspect.rs`
- `src/inspect_view_parser.rs`
- `src/inspect_scene_state.rs`
- `src/run_read.rs`
- `src/view_parser_read.rs`

## Task 1: Add The Inspect Server Crate Shell

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/auv-inspect-server/Cargo.toml`
- Create: `crates/auv-inspect-server/src/lib.rs`
- Create: `crates/auv-inspect-server/src/read_projection.rs`

- [ ] **Step 1: Add the workspace member**

In the root `Cargo.toml`, add `crates/auv-inspect-server` to `[workspace].members` near the other core crates:

```toml
  "crates/auv-inspect-server",
```

Do not add the root `[dependencies]` entry yet. The root crate starts using the
new crate in Task 5.

- [ ] **Step 2: Create the new crate manifest**

Create `crates/auv-inspect-server/Cargo.toml`:

```toml
[package]
name = "auv-inspect-server"
version.workspace = true
edition.workspace = true
publish.workspace = true
readme.workspace = true
license.workspace = true
authors.workspace = true
description.workspace = true
documentation.workspace = true
homepage.workspace = true
repository.workspace = true

[dependencies]
auv-tracing-driver = { path = "../auv-tracing-driver" }
auv-view = { path = "../auv-view" }
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 3: Add the public crate API**

Create `crates/auv-inspect-server/src/lib.rs`:

```rust
//! Viewer-facing HTTP/WebSocket inspection server for recorded AUV runs.
//!
//! This crate serves run storage and artifact inspection APIs. It does not
//! execute commands, drive applications, or own runtime semantics.

pub mod read_projection;

pub use read_projection::{DefaultInspectReadProjection, InspectReadProjection, InspectRunEnrichment};

pub type InspectResult<T> = Result<T, String>;

// TODO(auv-inspect-server-extraction): session exports are added in Task 2
// when the real session implementation moves into this crate.
// TODO(auv-inspect-server-extraction): server exports are added in Task 3
// when the real Axum server implementation moves into this crate.
```

- [ ] **Step 4: Add the read projection boundary**

Create `crates/auv-inspect-server/src/read_projection.rs`:

```rust
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewParserInspect, ViewParserListSummary};

use crate::InspectResult;

pub trait InspectReadProjection: Send + Sync + 'static {
  fn run_enrichment(&self, store: &LocalStore, run: &CanonicalRun) -> InspectResult<InspectRunEnrichment>;
}

#[derive(Clone, Debug, Default)]
pub struct DefaultInspectReadProjection;

impl InspectReadProjection for DefaultInspectReadProjection {
  fn run_enrichment(&self, _store: &LocalStore, _run: &CanonicalRun) -> InspectResult<InspectRunEnrichment> {
    Ok(InspectRunEnrichment::default())
  }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct InspectRunEnrichment {
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub command_boundary_claims: Vec<CommandBoundaryClaim>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub verifications: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub observation_snapshots: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub detector_recognition_lineage: Vec<serde_json::Value>,
  pub view_parser: ViewParserInspect,
  pub view_parser_summary: ViewParserListSummary,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandBoundaryClaim {
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub kind: String,
  pub message: String,
}
```

- [ ] **Step 5: Check the empty crate**

Run:

```bash
cargo check -p auv-inspect-server
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add Cargo.toml crates/auv-inspect-server/Cargo.toml crates/auv-inspect-server/src/lib.rs crates/auv-inspect-server/src/read_projection.rs
git commit -m "feat(auv-inspect-server): add inspect server crate shell"
```

## Task 2: Move The Session Descriptor

**Files:**

- Modify: `crates/auv-inspect-server/Cargo.toml`
- Create: `crates/auv-inspect-server/src/session.rs`
- Modify: `crates/auv-inspect-server/src/lib.rs`
- Modify: `src/inspect_server/mod.rs`
- Delete: `src/inspect_server/session.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add the session dependency**

In `crates/auv-inspect-server/Cargo.toml`, add the Unix uid helper dependency:

```toml
libc = "0.2"
```

- [ ] **Step 2: Move the session file**

Run:

```bash
cp src/inspect_server/session.rs crates/auv-inspect-server/src/session.rs
```

- [ ] **Step 3: Replace root-crate result and clock references**

In `crates/auv-inspect-server/src/session.rs`, replace:

```rust
use crate::model::AuvResult;
```

with:

```rust
use crate::InspectResult;
```

Replace every `AuvResult` in that file with `InspectResult`.

Add this helper near `current_user_id_for_path`:

```rust
fn now_millis() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0)
}
```

Replace every `crate::model::now_millis()` in the moved file with `now_millis()`.

- [ ] **Step 4: Delegate the old server to the moved session API**

In `src/inspect_server/mod.rs`, replace the local session module export:

```rust
pub mod session;

pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};
```

with:

```rust
pub use auv_inspect_server::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};
```

This keeps the old HTTP server writing the same session descriptor
implementation that CLI discovery now reads.

Delete the stale root session implementation:

```bash
rm src/inspect_server/session.rs
```

- [ ] **Step 5: Update root session discovery references**

In `src/main.rs`, change the return type of `read_discovered_inspect_session` from:

```rust
Result<Option<auv_cli::inspect_server::InspectServerSession>, String>
```

to:

```rust
Result<Option<auv_inspect_server::InspectServerSession>, String>
```

Change the body call from:

```rust
match auv_cli::inspect_server::read_inspect_session() {
```

to:

```rust
match auv_inspect_server::read_inspect_session() {
```

Replace test construction of `auv_cli::inspect_server::InspectServerSession` with:

```rust
auv_inspect_server::InspectServerSession
```

- [ ] **Step 6: Run session tests in the new crate**

Run:

```bash
cargo test -p auv-inspect-server session
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add Cargo.toml crates/auv-inspect-server/Cargo.toml crates/auv-inspect-server/src/lib.rs crates/auv-inspect-server/src/session.rs src/inspect_server/mod.rs src/inspect_server/session.rs src/main.rs
git commit -m "refactor(auv-inspect-server): move inspect session descriptor"
```

## Task 3: Move The Server With A Default Projection

**Files:**

- Modify: `crates/auv-inspect-server/Cargo.toml`
- Modify: `crates/auv-inspect-server/src/read_projection.rs`
- Create: `crates/auv-inspect-server/src/server.rs`
- Create: `crates/auv-inspect-server/src/inspect_server_viewer.html`
- Modify: `crates/auv-inspect-server/src/lib.rs`

- [ ] **Step 1: Add server dependencies**

In `crates/auv-inspect-server/Cargo.toml`, add the server dependencies:

```toml
axum = { version = "0.8", features = ["ws"] }
tokio = { version = "1", features = ["fs", "macros", "net", "rt-multi-thread", "sync"] }
tracing = "0.1"

[dev-dependencies]
tokio = { version = "1", features = ["fs", "macros", "net", "rt-multi-thread", "sync", "time"] }
tower = { version = "0.5", features = ["util"] }
```

- [ ] **Step 2: Copy the existing server module**

Run:

```bash
cp src/inspect_server/mod.rs crates/auv-inspect-server/src/server.rs
```

- [ ] **Step 3: Remove the nested session module from the copied server**

In `crates/auv-inspect-server/src/server.rs`, delete:

```rust
pub mod session;

pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};
```

Add this import near the other crate imports:

```rust
use crate::read_projection::{CommandBoundaryClaim, DefaultInspectReadProjection, InspectReadProjection, InspectRunEnrichment};
use crate::session::{InspectServerSession, write_inspect_session};
use crate::InspectResult;
```

- [ ] **Step 4: Replace root result and clock references**

In `crates/auv-inspect-server/src/server.rs`, replace:

```rust
use crate::model::{AuvResult, now_millis};
```

with:

```rust
fn now_millis() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|duration| duration.as_millis() as u64)
    .unwrap_or(0)
}
```

Replace every `AuvResult` in the copied server with `InspectResult`.

- [ ] **Step 5: Add projection to server state and config**

Add the generic extension method to `InspectReadProjection` in
`crates/auv-inspect-server/src/read_projection.rs`:

```rust
  fn run_json_extension(&self, extension: &str, store: &LocalStore, run_id: &str) -> InspectResult<serde_json::Value>;
```

Add the default implementation to `DefaultInspectReadProjection`:

```rust
  fn run_json_extension(&self, extension: &str, _store: &LocalStore, run_id: &str) -> InspectResult<serde_json::Value> {
    Err(format!("inspect run extension {extension:?} is not available for run {run_id}"))
  }
```

Change `InspectServerState` to:

```rust
#[derive(Clone)]
struct InspectServerState {
  store: Arc<LocalStore>,
  recorder: Arc<BroadcastRunRecorder>,
  write: InspectWriteConfig,
  write_locks: RunWriteLocks,
  projection: Arc<dyn InspectReadProjection>,
}
```

Remove `store_root` from the new crate's `InspectServeConfig`. The moved
`serve` function already receives a constructed `LocalStore`, so the public
config must not carry a second store root that can disagree with `store.root()`.
Update its `Default` implementation and moved tests in this file to stop
setting `store_root: None`.

Change `router` and `router_with_config` to:

```rust
pub fn router(store: LocalStore, recorder: Arc<BroadcastRunRecorder>) -> Router {
  router_with_projection(store, recorder, InspectWriteConfig::default(), Arc::new(DefaultInspectReadProjection))
}

pub fn router_with_projection(
  store: LocalStore,
  recorder: Arc<BroadcastRunRecorder>,
  write: InspectWriteConfig,
  projection: Arc<dyn InspectReadProjection>,
) -> Router {
  let state = InspectServerState {
    store: Arc::new(store),
    recorder,
    write,
    write_locks: RunWriteLocks::default(),
    projection,
  };
  Router::new()
    .route("/", get(serve_viewer))
    .route("/assets/{asset_name}", get(serve_design_asset))
    .route("/runs", get(list_runs))
    .route("/runs/{run_id}", get(get_run))
    .route("/runs/{run_id}/spans", get(get_spans))
    .route("/runs/{run_id}/events", get(get_events))
    .route("/runs/{run_id}/artifacts", get(get_artifacts))
    .route("/runs/{run_id}/artifacts/{artifact_id}", get(get_artifact))
    .route("/runs/{run_id}/minecraft-quality-baseline-report", get(get_minecraft_quality_baseline_report))
    .route("/runs/{run_id}/stream", get(stream_run))
    .route("/write/runs/{run_id}/updates", post(write_updates))
    .route("/write/runs/{run_id}/artifacts/{artifact_id}", post(write_artifact))
    .with_state(state)
}
```

Change `serve` to call `router_with_projection`:

```rust
pub async fn serve(
  store: LocalStore,
  recorder: Arc<BroadcastRunRecorder>,
  config: InspectServeConfig,
  projection: Arc<dyn InspectReadProjection>,
) -> InspectResult<SocketAddr> {
  config.validate_write_security()?;
  let address = (config.host.as_str(), config.port);
  let display_address = format!("{}:{}", config.host, config.port);
  let listener = TcpListener::bind(address).await.map_err(|error| format!("failed to bind inspect server {display_address}: {error}"))?;
  let local_address = listener.local_addr().map_err(|error| format!("failed to read inspect server address: {error}"))?;
  println!("inspect server: http://{local_address}");
  if config.write.enabled {
    let session = InspectServerSession {
      url: format!("http://{local_address}"),
      store_root: store.root().display().to_string(),
      write_enabled: true,
      write_token: config.write.token.clone(),
      pid: process::id(),
      started_at_millis: now_millis(),
    };
    write_inspect_session(&session)?;
  }
  axum::serve(listener, router_with_projection(store, recorder, config.write, projection))
    .await
    .map_err(|error| format!("inspect server failed: {error}"))?;
  Ok(local_address)
}
```

- [ ] **Step 6: Replace enrichment calls with the projection**

Replace the body of `list_runs` with:

```rust
async fn list_runs(State(state): State<InspectServerState>) -> Result<Response, InspectHttpError> {
  let runs = state.store.list_runs().map_err(InspectHttpError::from_store)?;
  let mut entries = Vec::with_capacity(runs.len());
  for run in runs {
    let run_id = run.run_id.as_str();
    let view_parser_summary = match state.store.read_run(run_id) {
      Ok(canonical) => match state.projection.run_enrichment(state.store.as_ref(), &canonical) {
        Ok(enrichment) => enrichment.view_parser_summary,
        Err(error) => {
          tracing::warn!(
            run_id = %run_id,
            stage = "build_inspect_run_enrichment",
            error = %error,
            "list row view_parser_summary degraded"
          );
          ViewParserListSummary::default()
        }
      },
      Err(error) => {
        tracing::warn!(
          run_id = %run_id,
          stage = "read_run",
          error = %error,
          "list row view_parser_summary degraded"
        );
        ViewParserListSummary::default()
      }
    };
    entries.push(RunListEntry {
      run,
      view_parser_summary,
    });
  }
  Ok(Json(entries).into_response())
}
```

Replace the enrichment portion of `get_run` with:

```rust
async fn get_run(State(state): State<InspectServerState>, Path(run_id): Path<String>) -> Result<Response, InspectHttpError> {
  let run = state.store.read_run(&run_id).map_err(InspectHttpError::from_store)?;
  let enrichment = state.projection.run_enrichment(state.store.as_ref(), &run).map_err(InspectHttpError::from_store)?;
  Ok(
    Json(InspectRunResponse {
      run: run.run,
      command_boundary_claims: enrichment.command_boundary_claims,
      verifications: enrichment.verifications,
      observation_snapshots: enrichment.observation_snapshots,
      detector_recognition_lineage: enrichment.detector_recognition_lineage,
      view_parser: enrichment.view_parser,
      view_parser_summary: enrichment.view_parser_summary,
    })
    .into_response(),
  )
}
```

Replace `get_minecraft_quality_baseline_report` with:

```rust
async fn get_minecraft_quality_baseline_report(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
) -> Result<Response, InspectHttpError> {
  let payload = state
    .projection
    .run_json_extension("minecraft-quality-baseline-report", state.store.as_ref(), &run_id)
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(payload).into_response())
}
```

Delete the local `extract_command_boundary_claims` function from `server.rs`.

- [ ] **Step 7: Change response field types**

Change `InspectRunResponse` to:

```rust
#[derive(serde::Serialize)]
struct InspectRunResponse {
  #[serde(flatten)]
  run: RunRecordV1Alpha1,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  command_boundary_claims: Vec<CommandBoundaryClaim>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  verifications: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  observation_snapshots: Vec<serde_json::Value>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  detector_recognition_lineage: Vec<serde_json::Value>,
  view_parser: ViewParserInspect,
  view_parser_summary: ViewParserListSummary,
}
```

Delete the local `CommandBoundaryClaim` struct from `server.rs`; the type now comes from `read_projection.rs`.

- [ ] **Step 8: Move the temporary viewer payload into the new crate**

Create a crate-owned temporary legacy viewer copy:

```bash
cp src/inspect_server_viewer.html crates/auv-inspect-server/src/inspect_server_viewer.html
```

Then change:

```rust
const VIEWER_HTML: &str = include_str!("../inspect_server_viewer.html");
```

to:

```rust
const VIEWER_HTML: &str = include_str!("inspect_server_viewer.html");
```

NOTICE: this duplicate viewer payload is temporary. Task 10 replaces it with
Vite build output, and Task 11 deletes the old root viewer file.

Keep `DESIGN_ASSETS` paths pointed at the repository design asset directory from the new crate file. For example:

```rust
("logo-mark.svg", include_bytes!("../../../docs/design/assets/logo-mark.svg"), "image/svg+xml"),
```

Apply that same `../../../docs/design/assets/` prefix to every design asset entry in `DESIGN_ASSETS`.

- [ ] **Step 9: Export `router_with_projection` for root adapter tests**

In `crates/auv-inspect-server/src/lib.rs`, change the server export line to:

```rust
pub use server::{DEFAULT_INSPECT_HOST, DEFAULT_INSPECT_PORT, InspectServeConfig, InspectWriteConfig, router, router_with_projection, serve};
```

- [ ] **Step 10: Run the new crate tests and capture expected failures**

Run:

```bash
cargo test -p auv-inspect-server
```

Expected: FAIL from test code still importing root-crate contract and scroll-scan helpers. Server production code should compile far enough that failures are in the copied test module.

- [ ] **Step 11: Commit the production move only**

Run:

```bash
git add crates/auv-inspect-server/Cargo.toml crates/auv-inspect-server/src/lib.rs crates/auv-inspect-server/src/read_projection.rs crates/auv-inspect-server/src/server.rs crates/auv-inspect-server/src/inspect_server_viewer.html
git commit -m "refactor(auv-inspect-server): move inspect server routes"
```

## Task 4: Move Server Tests That Do Not Need Root Internals

**Files:**

- Modify: `crates/auv-inspect-server/src/server.rs`

- [ ] **Step 1: Keep the generic route and write tests**

In `crates/auv-inspect-server/src/server.rs`, keep these copied tests in the `#[cfg(test)] mod tests` module:

```text
write_config_rejects_no_token_on_non_loopback
write_config_allows_no_token_on_loopback
write_config_rejects_token_with_no_token
write_updates_rejects_when_write_disabled
write_artifact_rejects_when_write_disabled
write_updates_payload_deserializes_camel_case_records
write_updates_requires_configured_token
write_updates_rejects_invalid_token
write_artifact_requires_configured_token
write_artifact_persists_bytes_when_authorized
artifact_endpoint_requires_span_id_when_artifact_id_is_ambiguous
write_artifact_uses_span_id_to_target_duplicate_artifact_ids
write_updates_accepts_run_started_and_persists_snapshot
write_updates_accepts_incremental_updates_for_existing_run
write_updates_replaces_existing_finished_snapshot_idempotently
write_updates_allows_no_token_when_configured
write_updates_rejects_conflicting_run_metadata
write_lock_serializes_same_run_sections
write_updates_rejects_nested_run_started_run_id_mismatch
write_updates_rejects_nested_run_finished_run_id_mismatch
write_updates_rejects_event_after_run_finished
write_updates_rejects_artifact_after_run_finished
write_updates_rejects_span_finish_after_run_finished
write_updates_rejects_run_finished_immutable_metadata_mismatch
write_updates_rejects_span_finished_immutable_metadata_mismatch
routes_return_canonical_records_and_artifact_bytes
root_serves_inline_viewer_html
viewer_self_tests_execute_in_node
root_payload_includes_span_tree_markers
root_payload_includes_events_rail_markers
root_payload_includes_websocket_stream_markers
root_payload_includes_artifact_panel_markers
root_payload_includes_surface_node_preview_markers
viewer_does_not_reference_removed_candidate_action_fields
viewer_renders_netease_select_proof_hint_hooks
viewer_renders_view_parser_list_badges_hooks
viewer_renders_view_parser_proof_hooks
viewer_renders_view_parser_diagnostic_links_hooks
viewer_renders_view_parser_list_filter_hooks
assets_route_serves_known_design_svgs_with_svg_mime
assets_route_rejects_unknown_and_traversal_names
stream_payload_filters_events_by_run_id
stream_rejects_missing_run_before_upgrade
artifact_endpoint_rejects_symlink_escape
write_artifact_rejects_symlink_target
```

Remove these copied tests from the new crate for now because they depend on root-crate contract or view-parser fixtures:

```text
run_route_includes_read_side_verifications_and_observation_snapshots
list_runs_includes_view_parser_summary_on_every_row
run_detail_includes_view_parser_summary
```

Those three behaviors are restored through root adapter tests in Task 6.

- [ ] **Step 2: Remove root-only imports from the new crate tests**

Delete imports from the test module that start with:

```rust
use crate::contract::{
```

Delete the import from the test module that starts with:

```rust
use crate::scroll_scan::{
```

Keep these imports:

```rust
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
```

- [ ] **Step 3: Remove root-only helper functions**

Delete these helper functions from the new crate test module:

```text
write_list_degraded_run
write_test_run_with_read_side_contracts
```

Keep generic run, span, event, artifact, temp-dir, and symlink helper functions.

- [ ] **Step 4: Run moved server tests**

Run:

```bash
cargo test -p auv-inspect-server
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/auv-inspect-server/src/server.rs
git commit -m "test(auv-inspect-server): move generic inspect server tests"
```

## Task 5: Wire The CLI To The New Server Crate

**Files:**

- Modify: `Cargo.toml`
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add the root dependency**

Add this dependency under the root `[dependencies]` table:

```toml
auv-inspect-server = { path = "crates/auv-inspect-server" }
```

- [ ] **Step 2: Update CLI default host and port references**

In `src/cli.rs`, change:

```rust
let mut host = auv_cli::inspect_server::DEFAULT_INSPECT_HOST.to_string();
let mut port = auv_cli::inspect_server::DEFAULT_INSPECT_PORT;
```

to:

```rust
let mut host = auv_inspect_server::DEFAULT_INSPECT_HOST.to_string();
let mut port = auv_inspect_server::DEFAULT_INSPECT_PORT;
```

Change CLI tests from:

```rust
assert_eq!(host, auv_cli::inspect_server::DEFAULT_INSPECT_HOST);
assert_eq!(port, auv_cli::inspect_server::DEFAULT_INSPECT_PORT);
```

to:

```rust
assert_eq!(host, auv_inspect_server::DEFAULT_INSPECT_HOST);
assert_eq!(port, auv_inspect_server::DEFAULT_INSPECT_PORT);
```

- [ ] **Step 3: Update inspect serve construction in `src/main.rs`**

Add this import near the existing `use std::sync::Arc;` line:

```rust
use auv_inspect_server::{InspectReadProjection, InspectRunEnrichment};
```

Change config construction from:

```rust
let config = auv_cli::inspect_server::InspectServeConfig {
  host: host.clone(),
  port: *port,
  store_root: Some(store_root.clone()),
  write: auv_cli::inspect_server::InspectWriteConfig {
    enabled: write.enabled || token.is_some(),
    token,
    no_token: write.no_token,
  },
};
auv_cli::inspect_server::serve(store, recorder, config).await?;
```

to:

```rust
let config = auv_inspect_server::InspectServeConfig {
  host: host.clone(),
  port: *port,
  write: auv_inspect_server::InspectWriteConfig {
    enabled: write.enabled || token.is_some(),
    token,
    no_token: write.no_token,
  },
};
auv_inspect_server::serve(store, recorder, config, Arc::new(RootInspectReadProjection)).await?;
```

- [ ] **Step 4: Add the root read projection adapter**

In `src/lib.rs`, add this code near the runtime builder helpers. The adapter
must live in the library crate because it uses crate-private `run_read`
extraction helpers over an already-loaded `CanonicalRun`.

```rust
#[derive(Clone, Debug)]
pub struct RootInspectReadProjection;

impl InspectReadProjection for RootInspectReadProjection {
  fn run_enrichment(
    &self,
    store: &auv_tracing_driver::store::LocalStore,
    run: &auv_tracing_driver::store::CanonicalRun,
  ) -> Result<InspectRunEnrichment, String> {
    let view_parser = inspect_view_parser::build_view_parser_inspect_for_run(store, run)?;
    let view_parser_summary = auv_view::memory::summarize_view_parser_inspect(&view_parser);
    Ok(InspectRunEnrichment {
      command_boundary_claims: extract_command_boundary_claims_for_inspect(run),
      verifications: run_read::extract_verifications(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode verification inspect values: {error}"))?,
      observation_snapshots: run_read::extract_observation_snapshots(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode observation snapshot inspect values: {error}"))?,
      detector_recognition_lineage: run_read::extract_detector_recognition_lineage(store, run)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to encode detector recognition lineage inspect values: {error}"))?,
      view_parser,
      view_parser_summary,
    })
  }

  fn run_json_extension(
    &self,
    extension: &str,
    store: &auv_tracing_driver::store::LocalStore,
    run_id: &str,
  ) -> Result<serde_json::Value, String> {
    match extension {
      "minecraft-quality-baseline-report" => serde_json::to_value(run_read::quality_baseline_report_with_verdicts_for_run(store, run_id)?)
        .map_err(|error| format!("failed to encode minecraft quality baseline report: {error}")),
      other => Err(format!("inspect run extension {other:?} is not supported by the root projection")),
    }
  }
}

fn extract_command_boundary_claims_for_inspect(
  run: &auv_tracing_driver::store::CanonicalRun,
) -> Vec<auv_inspect_server::CommandBoundaryClaim> {
  run
    .events
    .iter()
    .filter_map(|event| match event.name.as_str() {
      "command.verification" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "verification".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      "command.known_limit" => Some(auv_inspect_server::CommandBoundaryClaim {
        span_id: event.span_id.clone(),
        kind: "known_limit".to_string(),
        message: event.message.clone().unwrap_or_default(),
      }),
      _ => None,
    })
    .collect()
}
```

In `src/main.rs`, import the adapter from the library crate:

```rust
use auv_cli::{RootInspectReadProjection, build_default_runtime, build_runtime_with_store_root};
```

Do not define `RootInspectReadProjection` in `src/main.rs`.

- [ ] **Step 5: Remove the old module export**

In `src/lib.rs`, delete:

```rust
pub mod inspect_server;
```

- [ ] **Step 6: Run focused checks**

Run:

```bash
cargo test -p auv-cli cli::tests::inspect_serve_options_parse
cargo test -p auv-cli parse_inspect_serve
cargo test -p auv-cli tests::inspect_server_target_prefers_explicit_url_and_token_file
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add Cargo.toml src/cli.rs src/main.rs src/lib.rs
git commit -m "refactor(auv-cli): call extracted inspect server crate"
```

## Task 6: Restore Root Adapter Coverage

**Files:**

- Modify: `src/main.rs`

- [ ] **Step 1: Add a root adapter regression test**

Inside the `#[cfg(test)] mod tests` module in `src/main.rs`, add:

```rust
#[tokio::test]
async fn inspect_server_root_projection_keeps_read_side_fields() {
  use axum::body::{Body, to_bytes};
  use axum::http::{Request, StatusCode};
  use tower::ServiceExt;

  let root = env::temp_dir().join(format!("auv-root-inspect-projection-{}", auv_cli::model::now_millis()));
  let store = auv_tracing_driver::store::LocalStore::new(root.clone()).expect("store should initialize");
  let run_id = auv_tracing_driver::trace::RunId::new("run_root_projection_contracts");
  write_test_run_with_read_side_contracts(&store, &root, run_id.clone());
  let app = auv_inspect_server::router_with_projection(
    store,
    Arc::new(auv_tracing_driver::BroadcastRunRecorder::new(16)),
    auv_inspect_server::InspectWriteConfig::default(),
    Arc::new(super::RootInspectReadProjection),
  );

  let response = app
    .oneshot(
      Request::builder()
        .uri("/runs/run_root_projection_contracts")
        .body(Body::empty())
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::OK);
  let body = to_bytes(response.into_body(), usize::MAX).await.expect("body should read");
  let run: serde_json::Value = serde_json::from_slice(&body).expect("run should be json");
  assert_eq!(run["run_id"], "run_root_projection_contracts");
  assert_eq!(run["command_boundary_claims"][0]["kind"], "verification");
  assert_eq!(run["command_boundary_claims"][1]["kind"], "known_limit");
  assert_eq!(run["verifications"][0]["method"]["kind"], "semantic_match");
  assert_eq!(run["observation_snapshots"][0]["snapshot_id"], "snapshot_server_test");
  assert_eq!(run["detector_recognition_lineage"][0]["status"], "ready");
  assert!(run.get("view_parser").is_some());
  assert!(run.get("view_parser_summary").is_some());
  assert!(run.get("candidate_action_execution_lineage").is_none());

  let _ = fs::remove_dir_all(root);
}
```

This test reuses the existing `write_test_run_with_read_side_contracts` helper in `src/main.rs`.

- [ ] **Step 2: Run the adapter test**

Run:

```bash
cargo test -p auv-cli inspect_server_root_projection_keeps_read_side_fields
```

Expected: PASS.

- [ ] **Step 3: Run both inspect-focused suites**

Run:

```bash
cargo test -p auv-inspect-server
cargo test -p auv-cli inspect
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src/main.rs
git commit -m "test(auv-cli): cover inspect read projection adapter"
```

## Task 7: Remove The Old Root Server Module

**Files:**

- Delete: `src/inspect_server/mod.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Verify no root references remain**

Run:

```bash
rg "auv_cli::inspect_server|crate::inspect_server|pub mod inspect_server|src/inspect_server" src crates docs/ai/references/2026-07-10-auv-inspect-server-crate-viewer-implementation-plan.md
```

Expected: matches only in historical docs or this plan. No matches in Rust source except old files being deleted.

- [ ] **Step 2: Delete the old module files**

Run:

```bash
rm -r src/inspect_server
```

- [ ] **Step 3: Remove root dependencies made obsolete by the server move**

In the root `Cargo.toml`, remove these lines from `[dependencies]` if `rg` confirms only the moved server used them in the root crate:

```toml
axum = { version = "0.8", features = ["ws"] }
libc = "0.2"
```

In root `[dev-dependencies]`, remove this line if `rg "tower::ServiceExt" src tests` only finds tests moved to `auv-inspect-server`:

```toml
tower = { version = "0.5", features = ["util"] }
```

Keep `tokio`, `tokio-util`, and `tokio-stream` in the root crate if `cargo check -p auv-cli` still needs them for CLI, API, or runtime code.

- [ ] **Step 4: Run root and server checks**

Run:

```bash
cargo check -p auv-cli
cargo test -p auv-inspect-server
cargo test -p auv-cli inspect
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add Cargo.toml src/inspect_server src/lib.rs src/main.rs src/cli.rs crates/auv-inspect-server
git commit -m "refactor: remove root inspect server module"
```

## Task 8: Add Vite, Vue, And TypeScript Viewer Project

**Files:**

- Create: `crates/auv-inspect-server/viewer/package.json`
- Create: `crates/auv-inspect-server/viewer/tsconfig.json`
- Create: `crates/auv-inspect-server/viewer/vite.config.ts`
- Create: `crates/auv-inspect-server/viewer/index.html`
- Create: `crates/auv-inspect-server/viewer/src/main.ts`
- Create: `crates/auv-inspect-server/viewer/src/App.vue`
- Create: `crates/auv-inspect-server/viewer/src/legacy/viewer.ts`
- Create: `crates/auv-inspect-server/viewer/src/styles/viewer.css`

- [ ] **Step 1: Create the package manifest**

Create `crates/auv-inspect-server/viewer/package.json`:

```json
{
  "name": "auv-inspect-viewer",
  "private": true,
  "version": "0.0.1",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "typecheck": "vue-tsc --noEmit"
  },
  "dependencies": {
    "@vitejs/plugin-vue": "^6.0.0",
    "vite": "^7.0.0",
    "vue": "^3.5.0"
  },
  "devDependencies": {
    "typescript": "^5.8.0",
    "vue-tsc": "^3.0.0"
  }
}
```

- [ ] **Step 2: Add TypeScript config**

Create `crates/auv-inspect-server/viewer/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "jsx": "preserve",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "types": ["vite/client"]
  },
  "include": ["src/**/*.ts", "src/**/*.vue", "vite.config.ts"]
}
```

- [ ] **Step 3: Add Vite config**

Create `crates/auv-inspect-server/viewer/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  base: "/viewer-assets/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    assetsDir: "assets",
    rollupOptions: {
      output: {
        entryFileNames: "assets/viewer.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  },
  server: {
    proxy: {
      "/runs": "http://127.0.0.1:8765",
      "/write": "http://127.0.0.1:8765",
      "/assets": "http://127.0.0.1:8765"
    }
  }
});
```

- [ ] **Step 4: Add Vite HTML entry**

Create `crates/auv-inspect-server/viewer/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>AUV Inspect</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 5: Add Vue app entry**

Create `crates/auv-inspect-server/viewer/src/main.ts`:

```ts
import { createApp } from "vue";
import App from "./App.vue";
import "./styles/viewer.css";

createApp(App).mount("#app");
```

- [ ] **Step 6: Create the initial Vue component**

Create `crates/auv-inspect-server/viewer/src/App.vue`:

```vue
<template>
  <main id="auv-inspect-viewer">
    <div id="legacy-viewer-root"></div>
  </main>
</template>

<script setup lang="ts">
import { onMounted } from "vue";
import { mountLegacyViewer } from "./legacy/viewer";

onMounted(() => {
  const root = document.getElementById("legacy-viewer-root");
  if (root === null) {
    throw new Error("legacy viewer root is missing");
  }
  mountLegacyViewer(root);
});
</script>
```

- [ ] **Step 7: Add the legacy viewer TypeScript module**

Create `crates/auv-inspect-server/viewer/src/legacy/viewer.ts`:

```ts
export function mountLegacyViewer(root: HTMLElement): void {
  root.innerHTML = '<section class="empty">Inspect viewer is loading.</section>';
}
```

- [ ] **Step 8: Add starter CSS**

Create `crates/auv-inspect-server/viewer/src/styles/viewer.css`:

```css
:root {
  --brand: #00c4d2;
}

body {
  margin: 0;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

.empty {
  padding: 24px;
  color: #23343b;
}
```

- [ ] **Step 9: Install frontend dependencies**

Run:

```bash
npm --prefix crates/auv-inspect-server/viewer install
```

Expected: `crates/auv-inspect-server/viewer/package-lock.json` is created and npm exits successfully.

- [ ] **Step 10: Build the starter viewer**

Run:

```bash
npm --prefix crates/auv-inspect-server/viewer run build
```

Expected: PASS and `crates/auv-inspect-server/viewer/dist/index.html` exists.

- [ ] **Step 11: Commit**

Run:

```bash
git add crates/auv-inspect-server/viewer
git commit -m "feat(auv-inspect-server): add vite vue inspect viewer"
```

## Task 9: Port The Existing Viewer Into The Vite App

**Files:**

- Modify: `crates/auv-inspect-server/viewer/src/App.vue`
- Modify: `crates/auv-inspect-server/viewer/src/legacy/viewer.ts`
- Modify: `crates/auv-inspect-server/viewer/src/styles/viewer.css`

- [ ] **Step 1: Extract the old CSS**

Open `src/inspect_server_viewer.html`. Copy the contents between the opening `<style>` tag and the closing `</style>` tag into `crates/auv-inspect-server/viewer/src/styles/viewer.css`, replacing the starter CSS from Task 8.

Keep the exact existing CSS text. Do not rename classes in this task.

- [ ] **Step 2: Extract the old body markup**

Open `src/inspect_server_viewer.html`. Copy the contents between the opening `<body>` tag and the first `<script>` tag into the `<template>` block of `crates/auv-inspect-server/viewer/src/App.vue`.

The resulting `App.vue` must keep this script block:

```vue
<script setup lang="ts">
import { onMounted } from "vue";
import { mountLegacyViewer } from "./legacy/viewer";

onMounted(() => {
  mountLegacyViewer(document);
});
</script>
```

Use `mountLegacyViewer(document)` because the existing script expects document-level element IDs.

- [ ] **Step 3: Extract the old script**

Open `src/inspect_server_viewer.html`. Copy the contents between the opening `<script>` tag and the closing `</script>` tag into `crates/auv-inspect-server/viewer/src/legacy/viewer.ts`.

Wrap the copied script with this function:

```ts
// @ts-nocheck
// NOTICE(inspect-viewer-vite-migration): this file is a mechanical port of the
// legacy inline viewer script. Internal type checking is deferred until the
// script is split into typed Vue components; the module boundary remains typed.

export function mountLegacyViewer(document: Document): void {
  const window = document.defaultView;
  if (window === null) {
    throw new Error("viewer document has no default window");
  }

  const originalAddEventListener = window.addEventListener.bind(window);
  void originalAddEventListener;

  copiedViewerMain(document, window);
}

function copiedViewerMain(document: Document, window: Window): void {
}
```

Move the copied script body inside `copiedViewerMain`. Keep the original statement order. Do not change viewer behavior in this task.

- [ ] **Step 4: Run the frontend typecheck**

Run:

```bash
npm --prefix crates/auv-inspect-server/viewer run typecheck
```

Expected: PASS.

- [ ] **Step 5: Run the frontend build**

Run:

```bash
npm --prefix crates/auv-inspect-server/viewer run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add crates/auv-inspect-server/viewer/src/App.vue crates/auv-inspect-server/viewer/src/legacy/viewer.ts crates/auv-inspect-server/viewer/src/styles/viewer.css crates/auv-inspect-server/viewer/dist
git commit -m "refactor(auv-inspect-server): port inspect viewer into vite"
```

## Task 10: Serve Built Vite Assets From Rust

**Files:**

- Create: `crates/auv-inspect-server/src/viewer_assets.rs`
- Modify: `crates/auv-inspect-server/src/lib.rs`
- Modify: `crates/auv-inspect-server/src/server.rs`

- [ ] **Step 1: Add the built asset table**

Create `crates/auv-inspect-server/src/viewer_assets.rs`:

```rust
pub const VIEWER_HTML: &str = include_str!("../viewer/dist/index.html");

pub const VIEWER_ASSETS: &[(&str, &[u8], &str)] = &[
  ("assets/viewer.js", include_bytes!("../viewer/dist/assets/viewer.js"), "text/javascript; charset=utf-8"),
  ("assets/index.css", include_bytes!("../viewer/dist/assets/index.css"), "text/css; charset=utf-8"),
];

pub fn viewer_asset(name: &str) -> Option<(&'static [u8], &'static str)> {
  if name.is_empty() || name.contains('\\') || name.contains("..") || name.starts_with('.') {
    return None;
  }
  VIEWER_ASSETS.iter().find(|(asset_name, _, _)| *asset_name == name).map(|(_, bytes, mime)| (*bytes, *mime))
}
```

- [ ] **Step 2: Export the module inside the crate**

In `crates/auv-inspect-server/src/lib.rs`, add:

```rust
mod viewer_assets;
```

- [ ] **Step 3: Replace inline viewer serving**

In `crates/auv-inspect-server/src/server.rs`, delete:

```rust
const VIEWER_HTML: &str = include_str!("../../../src/inspect_server_viewer.html");
```

Add:

```rust
use crate::viewer_assets::{VIEWER_HTML, viewer_asset};
```

Add this route before `/assets/{asset_name}`:

```rust
.route("/viewer-assets/{*asset_name}", get(serve_viewer_asset))
```

Add this handler near `serve_viewer`:

```rust
async fn serve_viewer_asset(Path(asset_name): Path<String>) -> Response {
  match viewer_asset(&asset_name) {
    Some((bytes, mime)) => {
      let mut response = Body::from(bytes).into_response();
      let content_type = HeaderValue::from_str(mime).unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
      response.headers_mut().insert(CONTENT_TYPE, content_type);
      if let Ok(cache_control) = HeaderValue::from_str("no-cache") {
        response.headers_mut().insert("cache-control", cache_control);
      }
      response
    }
    None => InspectHttpError::not_found(format!("viewer asset {asset_name:?} not found")).into_response(),
  }
}
```

- [ ] **Step 4: Update the root viewer test expectation**

In `crates/auv-inspect-server/src/server.rs`, update `root_serves_inline_viewer_html` to assert Vite output:

```rust
assert!(html.starts_with("<!doctype html>"), "expected HTML5 doctype, got prefix {:?}", &html[..32.min(html.len())]);
assert!(html.contains("/viewer-assets/assets/viewer.js"), "viewer payload should load the Vite entry asset");
```

Keep the assertions that guard removed archived action-transition text.

- [ ] **Step 5: Add a viewer asset route test**

In the server tests module, add:

```rust
#[tokio::test]
async fn viewer_asset_route_serves_vite_entry() {
  let root = temp_dir("inspect-server-vite-assets");
  let store = LocalStore::new(root.clone()).expect("store should initialize");
  let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

  let response = app
    .oneshot(
      Request::builder()
        .uri("/viewer-assets/assets/viewer.js")
        .body(Body::empty())
        .expect("request should build"),
    )
    .await
    .expect("route should respond");

  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    response.headers().get("content-type").and_then(|value| value.to_str().ok()),
    Some("text/javascript; charset=utf-8")
  );
  assert_eq!(
    response.headers().get("cache-control").and_then(|value| value.to_str().ok()),
    Some("no-cache")
  );
  let body = to_bytes(response.into_body(), usize::MAX).await.expect("body should read");
  let js = std::str::from_utf8(&body).expect("asset should be utf-8");
  assert!(js.contains("/runs"), "viewer entry should fetch the inspect runs endpoint");
  assert!(js.contains("select a run from the sidebar"), "viewer entry should include the legacy viewer shell");

  let _ = fs::remove_dir_all(root);
}
```

- [ ] **Step 6: Run frontend build and server tests**

Run:

```bash
npm --prefix crates/auv-inspect-server/viewer run build
npm --prefix crates/auv-inspect-server/viewer run smoke
cargo test -p auv-inspect-server root_serves_inline_viewer_html
cargo test -p auv-inspect-server viewer_asset_route_serves_vite_entry
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add crates/auv-inspect-server/src/viewer_assets.rs crates/auv-inspect-server/src/lib.rs crates/auv-inspect-server/src/server.rs crates/auv-inspect-server/viewer/dist
git commit -m "feat(auv-inspect-server): serve built vite viewer assets"
```

## Task 11: Remove The Old Viewer HTML

**Files:**

- Delete: `src/inspect_server_viewer.html`
- Delete: `crates/auv-inspect-server/src/inspect_server_viewer.html`
- Modify: `qodana.yaml`
- Modify: `docs/design/README.md`
- Modify: `docs/design/IMPLEMENTATION_HANDOFF.md`

- [ ] **Step 1: Verify no Rust code includes the old viewer**

Run:

```bash
rg "inspect_server_viewer.html|VIEWER_HTML" src crates/auv-inspect-server/src
```

Expected: only `crates/auv-inspect-server/src/viewer_assets.rs` refers to `VIEWER_HTML`; no code refers to either deleted legacy viewer HTML file.

- [ ] **Step 2: Delete the old viewer file**

Run:

```bash
rm src/inspect_server_viewer.html
rm crates/auv-inspect-server/src/inspect_server_viewer.html
```

- [ ] **Step 3: Run viewer marker tests**

Run:

```bash
cargo test -p auv-inspect-server viewer
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src/inspect_server_viewer.html crates/auv-inspect-server/src/inspect_server_viewer.html qodana.yaml docs/design/README.md docs/design/IMPLEMENTATION_HANDOFF.md
git commit -m "refactor(auv-inspect-server): remove legacy viewer html"
```

## Task 12: Final Validation And Documentation Index

**Files:**

- Modify: `docs/ai/references/INDEX.md`
- Modify: `docs/ai/references/2026-07-10-auv-inspect-server-crate-viewer-implementation-plan.md`

- [ ] **Step 1: Update the reference index counts**

In `docs/ai/references/INDEX.md`, increment the total reference count by one, increment `core/inspect-trace` by one, and add this plan under the `core/inspect-trace` `implementation-plan` section:

```markdown
- [`2026-07-10-auv-inspect-server-crate-viewer-implementation-plan.md`](2026-07-10-auv-inspect-server-crate-viewer-implementation-plan.md) — implementation plan for extracting `auv-inspect-server` and migrating the viewer to Vite/Vue/TypeScript
```

- [ ] **Step 2: Run full validation commands**

Run:

```bash
cargo fmt --check
cargo check
cargo test
npm --prefix crates/auv-inspect-server/viewer run build
git diff --check
cargo run --quiet -- invoke --help
```

Expected: all commands PASS. The `cargo run --quiet -- invoke --help` output should print invoke help text and exit successfully.

- [ ] **Step 3: Run final source scans**

Run:

```bash
rg "candidate_action_execution_lineage" crates/auv-inspect-server/viewer crates/auv-inspect-server/src
rg "src/inspect_server|inspect_server_viewer.html" src crates/auv-inspect-server
```

Expected: the first command returns no matches. The second command returns no matches except path text inside committed documentation if the scan includes docs.

- [ ] **Step 4: Commit final docs**

Run:

```bash
git add docs/ai/references/INDEX.md docs/ai/references/2026-07-10-auv-inspect-server-crate-viewer-implementation-plan.md
git commit -m "docs: plan inspect server crate extraction"
```

## Self-Review

Spec coverage:

- Dedicated `crates/auv-inspect-server` crate is covered by Tasks 1-7.
- Root adapter read projection is covered by Tasks 3, 5, and 6.
- Vite, Vue, and TypeScript frontend authoring is covered by Tasks 8-10.
- Static Rust serving of built Vite assets is covered by Task 10.
- Removal of old root server and legacy viewer files is covered by Tasks 7 and 11.
- Validation commands from the spec are covered by Task 12.

Implementation constraints:

- The first server extraction keeps existing behavior before the frontend migration begins.
- The server crate does not depend on `auv-cli`.
- Root-only enrichment values cross the boundary as JSON until their owning contract types move to a shared crate.
- No runtime, driver, command execution, or archived vertical behavior is included.
