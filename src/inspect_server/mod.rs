//! HTTP/WebSocket inspection server for recorded runs.
//!
//! The inspect server serves a single-page HTML viewer plus JSON endpoints for
//! runs/spans/events/artifacts, and a WebSocket stream for live updates.
//! Optionally it can accept *write* updates/artifacts (guarded by config/token)
//! to support remote run recording.
//!
//! Boundary: this is a viewer-facing storage API. It does not execute commands
//! or perform UI automation; those live in `runtime`, drivers, and recipes.

pub mod session;

pub use session::{
  InspectServerSession, default_session_path, read_inspect_session, write_inspect_session,
};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::contract::{ObservationSnapshot, VerificationResult};
use crate::model::AuvResult;
use crate::recording::{BroadcastRunRecorder, RunRecorder, RunUpdate, WireUpdate};
use crate::run_read::AppValidationLineage;
use crate::store::{CanonicalRun, LocalStore};
use crate::trace::{RunId, RunRecordV1Alpha1, TraceState};

pub const DEFAULT_INSPECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_INSPECT_PORT: u16 = 8765;
const MAX_ARTIFACT_UPLOAD_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone)]
struct InspectServerState {
  store: Arc<LocalStore>,
  recorder: Arc<BroadcastRunRecorder>,
  write: InspectWriteConfig,
  write_locks: RunWriteLocks,
}

#[derive(Clone, Debug, Default)]
pub struct InspectWriteConfig {
  pub enabled: bool,
  pub token: Option<String>,
  pub no_token: bool,
}

#[derive(Clone, Default)]
struct RunWriteLocks {
  locks: Arc<std::sync::Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl RunWriteLocks {
  async fn lock(&self, run_id: &str) -> OwnedMutexGuard<()> {
    let lock = {
      let mut locks = self
        .locks
        .lock()
        .expect("run write locks should not poison");
      locks
        .entry(run_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
    };
    lock.lock_owned().await
  }
}

#[derive(Clone, Debug)]
pub struct InspectServeConfig {
  pub host: String,
  pub port: u16,
  pub store_root: Option<PathBuf>,
  pub write: InspectWriteConfig,
}

impl Default for InspectServeConfig {
  fn default() -> Self {
    Self {
      host: DEFAULT_INSPECT_HOST.to_string(),
      port: DEFAULT_INSPECT_PORT,
      store_root: None,
      write: InspectWriteConfig::default(),
    }
  }
}

impl InspectServeConfig {
  pub fn validate_write_security(&self) -> AuvResult<()> {
    if !self.write.enabled {
      return Ok(());
    }
    if self.write.no_token && self.write.token.is_some() {
      return Err("--no-write-token cannot be combined with a write token".to_string());
    }
    if self.write.no_token && !is_loopback_host(&self.host) {
      return Err("non-loopback inspect server write requires a token".to_string());
    }
    if !is_loopback_host(&self.host) && self.write.token.is_none() {
      return Err("non-loopback inspect server write requires a token".to_string());
    }
    Ok(())
  }
}

fn is_loopback_host(host: &str) -> bool {
  matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// Single-payload HTML viewer served at `GET /`. Inlines CSS + JS; SVG
/// assets used by the viewer are served separately under `/assets/:name`
/// from the design-system asset library (see [`design_asset`]). Visual
/// tokens match `docs/design/colors_and_type.css`; when the canonical
/// tokens drift, sync the inlined `:root` block in the embedded HTML.
const VIEWER_HTML: &str = include_str!("../inspect_server_viewer.html");

/// Compile-time map of design-system asset filename -> bytes, mounted at
/// `GET /assets/:name`. Each entry is pulled in via `include_bytes!`
/// from `docs/design/assets/` so the bundle ships a single binary and
/// the viewer payload itself stays under ~40 KB even as more sprites
/// land in C.3b and beyond.
///
/// To add a new asset: drop the SVG into `docs/design/assets/`, then
/// add a `(filename, bytes, mime)` row here. SVG is the default; binary
/// or raster assets can land alongside with their actual mime type.
const DESIGN_ASSETS: &[(&str, &[u8], &str)] = &[
  (
    "logo-mark.svg",
    include_bytes!("../../docs/design/assets/logo-mark.svg"),
    "image/svg+xml",
  ),
  (
    "sparkle.svg",
    include_bytes!("../../docs/design/assets/sparkle.svg"),
    "image/svg+xml",
  ),
  (
    "icon-png.svg",
    include_bytes!("../../docs/design/assets/icon-png.svg"),
    "image/svg+xml",
  ),
  (
    "icon-json.svg",
    include_bytes!("../../docs/design/assets/icon-json.svg"),
    "image/svg+xml",
  ),
  (
    "icon-bin.svg",
    include_bytes!("../../docs/design/assets/icon-bin.svg"),
    "image/svg+xml",
  ),
  (
    "sprite-inspector.svg",
    include_bytes!("../../docs/design/assets/sprite-inspector.svg"),
    "image/svg+xml",
  ),
  (
    "cursor-auv.svg",
    include_bytes!("../../docs/design/assets/cursor-auv.svg"),
    "image/svg+xml",
  ),
  (
    "cursor-auv-click.svg",
    include_bytes!("../../docs/design/assets/cursor-auv-click.svg"),
    "image/svg+xml",
  ),
  (
    "cursor-you.svg",
    include_bytes!("../../docs/design/assets/cursor-you.svg"),
    "image/svg+xml",
  ),
];
pub fn router(store: LocalStore, recorder: Arc<BroadcastRunRecorder>) -> Router {
  router_with_config(store, recorder, InspectWriteConfig::default())
}

fn router_with_config(
  store: LocalStore,
  recorder: Arc<BroadcastRunRecorder>,
  write: InspectWriteConfig,
) -> Router {
  let state = InspectServerState {
    store: Arc::new(store),
    recorder,
    write,
    write_locks: RunWriteLocks::default(),
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
    .route("/runs/{run_id}/stream", get(stream_run))
    .route("/write/runs/{run_id}/updates", post(write_updates))
    .route(
      "/write/runs/{run_id}/artifacts/{artifact_id}",
      post(write_artifact),
    )
    .with_state(state)
}

async fn serve_viewer() -> Response {
  let mut response = Body::from(VIEWER_HTML).into_response();
  response.headers_mut().insert(
    CONTENT_TYPE,
    HeaderValue::from_static("text/html; charset=utf-8"),
  );
  response
}

fn design_asset(name: &str) -> Option<(&'static [u8], &'static str)> {
  // Hardened against path traversal: reject anything that looks like a
  // path segment. Axum already URL-decodes the matched param, so a
  // literal `..` or slash in the name means a malformed request, not a
  // legitimate asset lookup.
  if name.is_empty()
    || name.contains('/')
    || name.contains('\\')
    || name.contains("..")
    || name.starts_with('.')
  {
    return None;
  }
  DESIGN_ASSETS
    .iter()
    .find(|(asset_name, _, _)| *asset_name == name)
    .map(|(_, bytes, mime)| (*bytes, *mime))
}

async fn serve_design_asset(Path(asset_name): Path<String>) -> Response {
  match design_asset(&asset_name) {
    Some((bytes, mime)) => {
      let mut response = Body::from(bytes).into_response();
      let content_type = HeaderValue::from_str(mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
      response.headers_mut().insert(CONTENT_TYPE, content_type);
      // Assets are bundled at compile time and never change at runtime,
      // so a one-year immutable cache is safe; clients can rely on the
      // filename being stable across server restarts of the same build.
      if let Ok(cache_control) = HeaderValue::from_str("public, max-age=31536000, immutable") {
        response
          .headers_mut()
          .insert("cache-control", cache_control);
      }
      response
    }
    None => {
      InspectHttpError::not_found(format!("design asset {asset_name:?} not found")).into_response()
    }
  }
}

pub async fn serve(
  store: LocalStore,
  recorder: Arc<BroadcastRunRecorder>,
  config: InspectServeConfig,
) -> AuvResult<SocketAddr> {
  config.validate_write_security()?;
  let address = (config.host.as_str(), config.port);
  let display_address = format!("{}:{}", config.host, config.port);
  let listener = TcpListener::bind(address)
    .await
    .map_err(|error| format!("failed to bind inspect server {display_address}: {error}"))?;
  let local_address = listener
    .local_addr()
    .map_err(|error| format!("failed to read inspect server address: {error}"))?;
  println!("inspect server: http://{local_address}");
  if config.write.enabled {
    let session = session::InspectServerSession {
      url: format!("http://{local_address}"),
      store_root: store.root().display().to_string(),
      write_enabled: true,
      write_token: config.write.token.clone(),
      pid: std::process::id(),
      started_at_millis: crate::model::now_millis(),
    };
    session::write_inspect_session(&session)?;
  }
  axum::serve(listener, router_with_config(store, recorder, config.write))
    .await
    .map_err(|error| format!("inspect server failed: {error}"))?;
  Ok(local_address)
}

async fn list_runs(State(state): State<InspectServerState>) -> Result<Response, InspectHttpError> {
  let runs = state
    .store
    .list_runs()
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(runs).into_response())
}

async fn get_run(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
) -> Result<Response, InspectHttpError> {
  let run = state
    .store
    .read_run(&run_id)
    .map_err(InspectHttpError::from_store)?;
  let verifications = crate::run_read::extract_verifications(state.store.as_ref(), &run)
    .map_err(InspectHttpError::from_store)?;
  let observation_snapshots =
    crate::run_read::extract_observation_snapshots(state.store.as_ref(), &run)
      .map_err(InspectHttpError::from_store)?;
  let validation_lineage =
    crate::run_read::extract_app_validation_lineage(state.store.as_ref(), &run)
      .map_err(InspectHttpError::from_store)?;
  Ok(
    Json(InspectRunResponse {
      run: run.run,
      verifications,
      observation_snapshots,
      validation_lineage,
    })
    .into_response(),
  )
}

async fn get_spans(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
) -> Result<Response, InspectHttpError> {
  let run = state
    .store
    .read_run(&run_id)
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(run.spans).into_response())
}

async fn get_events(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
) -> Result<Response, InspectHttpError> {
  let run = state
    .store
    .read_run(&run_id)
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(run.events).into_response())
}

async fn get_artifacts(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
) -> Result<Response, InspectHttpError> {
  let run = state
    .store
    .read_run(&run_id)
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(run.artifacts).into_response())
}

async fn get_artifact(
  State(state): State<InspectServerState>,
  Path((run_id, artifact_id)): Path<(String, String)>,
  Query(query): Query<ArtifactLookupQuery>,
) -> Result<Response, InspectHttpError> {
  let (artifact, path) = state
    .store
    .artifact_file_scoped(&run_id, &artifact_id, query.span_id.as_deref())
    .map_err(InspectHttpError::from_store)?;
  let bytes = tokio::fs::read(&path)
    .await
    .map_err(|error| InspectHttpError::not_found(format!("failed to read artifact: {error}")))?;
  let mut response = Body::from(bytes).into_response();
  let content_type = HeaderValue::from_str(&artifact.mime_type)
    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
  response.headers_mut().insert(CONTENT_TYPE, content_type);
  Ok(response)
}

async fn stream_run(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
  websocket: WebSocketUpgrade,
) -> Result<Response, InspectHttpError> {
  ensure_stream_run_exists(&state.store, &run_id)?;
  Ok(
    websocket
      .on_upgrade(move |socket| stream_run_events(socket, state.recorder, run_id))
      .into_response(),
  )
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteUpdatesRequest {
  updates: Vec<WireUpdate>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteUpdatesResponse {
  accepted: usize,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StructuredErrorBody {
  error: StructuredError,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StructuredError {
  code: String,
  message: String,
  run_id: Option<String>,
  conflict_kind: Option<String>,
  resolution: Option<String>,
  retryable: bool,
}

async fn write_updates(
  State(state): State<InspectServerState>,
  Path(run_id): Path<String>,
  headers: HeaderMap,
  Json(request): Json<WriteUpdatesRequest>,
) -> Result<Response, InspectHttpError> {
  authorize_write(&headers, &state.write)?;
  let _write_guard = state.write_locks.lock(&run_id).await;
  let mut snapshot = state.store.read_run(&run_id).ok();
  let updates = request
    .updates
    .into_iter()
    .map(|wire| wire.0)
    .collect::<Vec<_>>();

  for update in &updates {
    validate_update_run_ids(&run_id, update)?;
    apply_update(&mut snapshot, update).map_err(InspectHttpError::conflict)?;
  }

  let Some(snapshot) = snapshot else {
    return Err(InspectHttpError::bad_request(
      "first update for a run must be runStarted".to_string(),
    ));
  };
  state
    .store
    .replace_run_snapshot(&snapshot)
    .map_err(InspectHttpError::from_store)?;

  let accepted = updates.len();
  for update in updates {
    state
      .recorder
      .record(update)
      .map_err(InspectHttpError::from_store)?;
  }
  Ok(Json(WriteUpdatesResponse { accepted }).into_response())
}

async fn write_artifact(
  State(state): State<InspectServerState>,
  Path((run_id, artifact_id)): Path<(String, String)>,
  Query(query): Query<ArtifactLookupQuery>,
  headers: HeaderMap,
  body: Body,
) -> Result<Response, InspectHttpError> {
  authorize_write(&headers, &state.write)?;
  let bytes = to_bytes(body, MAX_ARTIFACT_UPLOAD_BYTES)
    .await
    .map_err(|error| {
      InspectHttpError::payload_too_large(format!("artifact upload rejected: {error}"))
    })?;
  let artifact = state
    .store
    .write_artifact_bytes_scoped(&run_id, &artifact_id, query.span_id.as_deref(), &bytes)
    .map_err(InspectHttpError::from_store)?;
  Ok(Json(artifact).into_response())
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactLookupQuery {
  span_id: Option<String>,
}

#[derive(serde::Serialize)]
struct InspectRunResponse {
  #[serde(flatten)]
  run: RunRecordV1Alpha1,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  verifications: Vec<VerificationResult>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  observation_snapshots: Vec<ObservationSnapshot>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  validation_lineage: Vec<AppValidationLineage>,
}

#[allow(clippy::result_large_err)]
fn authorize_write(
  headers: &HeaderMap,
  write: &InspectWriteConfig,
) -> Result<(), InspectHttpError> {
  if !write.enabled {
    return Err(InspectHttpError::forbidden(
      "inspect server write is disabled".to_string(),
    ));
  }
  if write.no_token {
    return Ok(());
  }
  let Some(expected) = &write.token else {
    return Err(InspectHttpError::forbidden(
      "inspect server write token is required".to_string(),
    ));
  };
  let actual = headers
    .get(AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));
  if actual == Some(expected.as_str()) {
    Ok(())
  } else {
    Err(InspectHttpError::forbidden(
      "invalid inspect server write token".to_string(),
    ))
  }
}

#[allow(clippy::result_large_err)]
fn validate_update_run_ids(run_id: &str, update: &RunUpdate) -> Result<(), InspectHttpError> {
  if update.run_id().as_str() != run_id {
    return Err(InspectHttpError::bad_request(format!(
      "update runId {} does not match request runId {run_id}",
      update.run_id()
    )));
  }
  match update {
    RunUpdate::RunStarted { run, .. } | RunUpdate::RunFinished { run, .. }
      if run.run_id.as_str() != run_id =>
    {
      Err(InspectHttpError::bad_request(format!(
        "nested runId {} does not match request runId {run_id}",
        run.run_id
      )))
    }
    _ => Ok(()),
  }
}

fn apply_update(
  snapshot: &mut Option<CanonicalRun>,
  update: &RunUpdate,
) -> Result<(), RunConflict> {
  match update {
    RunUpdate::RunStarted { run, .. } => {
      if let Some(existing) = snapshot {
        if existing.run != *run {
          return Err(RunConflict::new(&run.run_id, "runMetadataMismatch"));
        }
        return Ok(());
      }
      *snapshot = Some(CanonicalRun {
        run: run.clone(),
        spans: Vec::new(),
        events: Vec::new(),
        artifacts: Vec::new(),
      });
      Ok(())
    }
    RunUpdate::SpanStarted { run_id, span } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      if let Some(parent) = &span.parent_span_id
        && !snapshot
          .spans
          .iter()
          .any(|existing| existing.span_id == *parent)
        && snapshot.run.root_span_id != *parent
      {
        return Err(RunConflict::new(run_id, "missingParentSpan"));
      }
      if let Some(existing) = snapshot
        .spans
        .iter()
        .find(|existing| existing.span_id == span.span_id)
      {
        if existing != span {
          return Err(RunConflict::new(run_id, "duplicateSpanMismatch"));
        }
        return Ok(());
      }
      snapshot.spans.push(span.clone());
      Ok(())
    }
    RunUpdate::SpanFinished { run_id, span } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      let Some(existing) = snapshot
        .spans
        .iter_mut()
        .find(|existing| existing.span_id == span.span_id)
      else {
        return Err(RunConflict::new(run_id, "missingParentSpan"));
      };
      if span_immutable_metadata_differs(existing, span) {
        return Err(RunConflict::new(run_id, "duplicateSpanMismatch"));
      }
      if existing.state == TraceState::Ended && existing != span {
        return Err(RunConflict::new(run_id, "duplicateSpanMismatch"));
      }
      *existing = span.clone();
      Ok(())
    }
    RunUpdate::EventAppended { run_id, event } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      if let Some(existing) = snapshot
        .events
        .iter()
        .find(|existing| existing.event_id == event.event_id)
      {
        if existing != event {
          return Err(RunConflict::new(run_id, "duplicateEventMismatch"));
        }
        return Ok(());
      }
      snapshot.events.push(event.clone());
      Ok(())
    }
    RunUpdate::ArtifactCreated { run_id, artifact } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if snapshot.run.state == TraceState::Ended {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      if let Some(existing) = snapshot
        .artifacts
        .iter()
        .find(|existing| existing.artifact_id == artifact.artifact_id)
      {
        if existing != artifact {
          return Err(RunConflict::new(run_id, "duplicateArtifactMismatch"));
        }
        return Ok(());
      }
      snapshot.artifacts.push(artifact.clone());
      Ok(())
    }
    RunUpdate::RunFinished { run_id, run } => {
      let snapshot = snapshot
        .as_mut()
        .ok_or_else(|| RunConflict::new(run_id, "missingRunStarted"))?;
      if run_immutable_metadata_differs(&snapshot.run, run) {
        return Err(RunConflict::new(run_id, "runMetadataMismatch"));
      }
      if snapshot.run.state == TraceState::Ended && snapshot.run != *run {
        return Err(RunConflict::new(run_id, "runAlreadyFinished"));
      }
      snapshot.run = run.clone();
      Ok(())
    }
  }
}

fn run_immutable_metadata_differs(
  existing: &crate::trace::RunRecordV1Alpha1,
  next: &crate::trace::RunRecordV1Alpha1,
) -> bool {
  existing.api_version != next.api_version
    || existing.run_id != next.run_id
    || existing.trace_id != next.trace_id
    || existing.run_type != next.run_type
    || existing.started_at_millis != next.started_at_millis
    || existing.root_span_id != next.root_span_id
    || existing.attributes != next.attributes
}

fn span_immutable_metadata_differs(
  existing: &crate::trace::SpanRecordV1Alpha1,
  next: &crate::trace::SpanRecordV1Alpha1,
) -> bool {
  existing.api_version != next.api_version
    || existing.span_id != next.span_id
    || existing.parent_span_id != next.parent_span_id
    || existing.name != next.name
    || existing.started_at_millis != next.started_at_millis
    || existing.attributes != next.attributes
}

#[derive(Debug)]
struct RunConflict {
  run_id: String,
  kind: String,
}

impl RunConflict {
  fn new(run_id: &RunId, kind: &str) -> Self {
    Self {
      run_id: run_id.to_string(),
      kind: kind.to_string(),
    }
  }
}

#[allow(clippy::result_large_err)]
fn ensure_stream_run_exists(store: &LocalStore, run_id: &str) -> Result<(), InspectHttpError> {
  store
    .read_run(run_id)
    .map(|_| ())
    .map_err(InspectHttpError::from_store)
}

async fn stream_run_events(
  mut socket: WebSocket,
  recorder: Arc<BroadcastRunRecorder>,
  run_id: String,
) {
  let mut receiver = recorder.subscribe();
  while let Some(payload) = next_stream_payload(&mut receiver, &run_id).await {
    if socket.send(Message::Text(payload.into())).await.is_err() {
      break;
    }
  }
}

async fn next_stream_payload(
  receiver: &mut broadcast::Receiver<RunUpdate>,
  run_id: &str,
) -> Option<String> {
  loop {
    match receiver.recv().await {
      Ok(update) if update.run_id().as_str() == run_id => match serde_json::to_string(&update) {
        Ok(payload) => return Some(payload),
        Err(_) => continue,
      },
      Ok(_) => {}
      Err(broadcast::error::RecvError::Lagged(_)) => {}
      Err(broadcast::error::RecvError::Closed) => return None,
    }
  }
}

#[derive(Debug)]
struct InspectHttpError {
  status: StatusCode,
  message: String,
  structured: Option<StructuredErrorBody>,
}

impl InspectHttpError {
  fn from_store(error: String) -> Self {
    let status = if error.contains("invalid run id") || error.contains("specify span_id") {
      StatusCode::BAD_REQUEST
    } else if error.contains("escapes run directory") || error.contains("symlink artifact path") {
      StatusCode::FORBIDDEN
    } else if error.contains("failed to read") || error.contains("not found") {
      StatusCode::NOT_FOUND
    } else {
      StatusCode::INTERNAL_SERVER_ERROR
    };
    Self {
      status,
      message: error,
      structured: None,
    }
  }

  fn not_found(message: String) -> Self {
    Self {
      status: StatusCode::NOT_FOUND,
      message,
      structured: None,
    }
  }

  fn bad_request(message: String) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      message,
      structured: None,
    }
  }

  fn forbidden(message: String) -> Self {
    Self {
      status: StatusCode::FORBIDDEN,
      message,
      structured: None,
    }
  }

  fn payload_too_large(message: String) -> Self {
    Self {
      status: StatusCode::PAYLOAD_TOO_LARGE,
      message,
      structured: None,
    }
  }

  fn conflict(conflict: RunConflict) -> Self {
    let message = format!(
      "run {} rejected update conflict {}",
      conflict.run_id, conflict.kind
    );
    Self {
      status: StatusCode::CONFLICT,
      message: message.clone(),
      structured: Some(StructuredErrorBody {
        error: StructuredError {
          code: "runConflict".to_string(),
          message,
          run_id: Some(conflict.run_id),
          conflict_kind: Some(conflict.kind),
          resolution: Some("startNewRun".to_string()),
          retryable: false,
        },
      }),
    }
  }
}

impl IntoResponse for InspectHttpError {
  fn into_response(self) -> Response {
    if let Some(body) = self.structured {
      return (self.status, Json(body)).into_response();
    }
    (
      self.status,
      Json(serde_json::json!({
        "error": self.message,
      })),
    )
      .into_response()
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::path::Path;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use axum::Router;
  use axum::body::{Body, to_bytes};
  use axum::http::{Request, StatusCode};
  use tower::ServiceExt;

  use super::{ensure_stream_run_exists, next_stream_payload, router, router_with_config};
  use crate::app::{
    AppIdentity, AppValidatedCandidate, AppValidation, AppValidationStatus, AppVerificationMode,
  };
  use crate::contract::{
    OBSERVATION_SNAPSHOT_API_VERSION, OPERATION_RESULT_API_VERSION, ObservationSnapshot,
    ObservationSource, OperationOutput, OperationResult, OperationStatus, RecognitionScope,
    RecognitionSurface, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
  };
  use crate::model::now_millis;
  use crate::recording::{BroadcastRunRecorder, RunRecorder, RunUpdate};
  use crate::scroll_scan::{
    CollectionObservation, CompletenessClaim, HookDecisionRecord, ObservationCluster,
    ScanPageRecord, ScanRegion, ScanTarget, ScrollBoundaryCandidate, ScrollScanArtifact,
    SectionCandidate, StopEvidence, StopPolicy, StopReason,
  };
  use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
  use crate::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
    EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  const NATIVE_TEXT_LEGACY_TAXONOMY_ID: &str =
    "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text";

  #[test]
  fn write_config_rejects_no_token_on_non_loopback() {
    let error = super::InspectServeConfig {
      host: "0.0.0.0".to_string(),
      port: 8765,
      store_root: None,
      write: super::InspectWriteConfig {
        enabled: true,
        token: None,
        no_token: true,
      },
    }
    .validate_write_security()
    .expect_err("non-loopback write without token should reject");

    assert!(error.contains("non-loopback"));
  }

  #[test]
  fn write_config_allows_no_token_on_loopback() {
    super::InspectServeConfig {
      host: "127.0.0.1".to_string(),
      port: 8765,
      store_root: None,
      write: super::InspectWriteConfig {
        enabled: true,
        token: None,
        no_token: true,
      },
    }
    .validate_write_security()
    .expect("loopback write without token should be allowed");
  }

  #[test]
  fn write_config_rejects_token_with_no_token() {
    let error = super::InspectServeConfig {
      host: "127.0.0.1".to_string(),
      port: 8765,
      store_root: None,
      write: super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: true,
      },
    }
    .validate_write_security()
    .expect_err("token and no-token should conflict");

    assert!(error.contains("--no-write-token"));
  }

  #[tokio::test]
  async fn write_updates_rejects_when_write_disabled() {
    let root = temp_dir("inspect-write-disabled");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig::default(),
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/updates")
          .header("content-type", "application/json")
          .body(Body::from(r#"{"updates":[]}"#))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_artifact_rejects_when_write_disabled() {
    let root = temp_dir("inspect-write-artifact-disabled");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig::default(),
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/artifacts/artifact_write_test")
          .body(Body::from("artifact body"))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn write_updates_payload_deserializes_camel_case_records() {
    let body = serde_json::json!({
      "updates": [{
        "type": "runStarted",
        "runId": "run_write_test",
        "run": test_run_json("run_write_test")
      }]
    });

    let request: super::WriteUpdatesRequest =
      serde_json::from_value(body).expect("write payload should deserialize");

    assert_eq!(request.updates.len(), 1);
  }

  #[tokio::test]
  async fn write_updates_requires_configured_token() {
    let root = temp_dir("inspect-write-token-required");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/updates")
          .header("content-type", "application/json")
          .body(Body::from(r#"{"updates":[]}"#))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_invalid_token() {
    let root = temp_dir("inspect-write-token-invalid");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/updates")
          .header("authorization", "Bearer wrong")
          .header("content-type", "application/json")
          .body(Body::from(r#"{"updates":[]}"#))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_artifact_requires_configured_token() {
    let root = temp_dir("inspect-write-artifact-token-required");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/artifacts/artifact_write_test")
          .body(Body::from("artifact body"))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_artifact_persists_bytes_when_authorized() {
    let root = temp_dir("inspect-write-artifact-authorized");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_write_artifact");
    write_test_run(&store, run_id.clone(), Some("uploaded.txt"));
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_artifact/artifacts/artifact_server_test")
          .header("authorization", "Bearer secret")
          .body(Body::from("artifact body"))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("artifact json");
    assert_eq!(value["artifact_id"], "artifact_server_test");
    let (_, artifact_path) = store
      .artifact_file("run_write_artifact", "artifact_server_test")
      .expect("artifact file should resolve");
    assert_eq!(
      fs::read_to_string(artifact_path).expect("artifact should read"),
      "artifact body"
    );
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn artifact_endpoint_requires_span_id_when_artifact_id_is_ambiguous() {
    let root = temp_dir("inspect-artifact-ambiguous");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_ambiguous_artifact");
    write_test_run_with_duplicate_artifacts(&store, run_id.clone());
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .clone()
      .oneshot(
        Request::builder()
          .uri("/runs/run_ambiguous_artifact/artifacts/artifact_dup")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let scoped = app
      .oneshot(
        Request::builder()
          .uri("/runs/run_ambiguous_artifact/artifacts/artifact_dup?spanId=0000000000000002")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(scoped.status(), StatusCode::OK);
    let body = to_bytes(scoped.into_body(), usize::MAX)
      .await
      .expect("body should read");
    assert_eq!(body.as_ref(), b"second artifact");

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_artifact_uses_span_id_to_target_duplicate_artifact_ids() {
    let root = temp_dir("inspect-write-artifact-ambiguous");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_write_ambiguous_artifact");
    write_test_run_with_duplicate_artifacts(&store, run_id.clone());
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_ambiguous_artifact/artifacts/artifact_dup?spanId=0000000000000002")
          .header("authorization", "Bearer secret")
          .body(Body::from("updated second"))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let (_, first_path) = store
      .artifact_file_scoped(
        "run_write_ambiguous_artifact",
        "artifact_dup",
        Some("0000000000000001"),
      )
      .expect("first artifact should resolve");
    let (_, second_path) = store
      .artifact_file_scoped(
        "run_write_ambiguous_artifact",
        "artifact_dup",
        Some("0000000000000002"),
      )
      .expect("second artifact should resolve");
    assert_eq!(
      fs::read_to_string(first_path).expect("first artifact should read"),
      "first artifact"
    );
    assert_eq!(
      fs::read_to_string(second_path).expect("second artifact should read"),
      "updated second"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_accepts_run_started_and_persists_snapshot() {
    let root = temp_dir("inspect-write-accept");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: Some("secret".to_string()),
        no_token: false,
      },
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "runStarted",
        "runId": "run_write_test",
        "run": test_run_json("run_write_test")
      }]
    });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_test/updates")
          .header("authorization", "Bearer secret")
          .header("content-type", "application/json")
          .body(Body::from(body.to_string()))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(store.read_run("run_write_test").is_ok());
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_accepts_incremental_updates_for_existing_run() {
    let root = temp_dir("inspect-write-incremental");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );

    let response = post_write_updates(
      app.clone(),
      "run_write_incremental",
      serde_json::json!({
        "updates": [{
          "type": "runStarted",
          "runId": "run_write_incremental",
          "run": test_run_json("run_write_incremental")
        }]
      }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = post_write_updates(
      app,
      "run_write_incremental",
      serde_json::json!({
        "updates": [{
          "type": "spanStarted",
          "runId": "run_write_incremental",
          "span": test_span_json("0000000000000002", Some("0000000000000001"), "debug.step", "running")
        }]
      }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let snapshot = store
      .read_run("run_write_incremental")
      .expect("incremental run should persist");
    assert_eq!(snapshot.spans.len(), 1);
    assert_eq!(snapshot.spans[0].span_id.as_str(), "0000000000000002");
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_replaces_existing_finished_snapshot_idempotently() {
    let root = temp_dir("inspect-write-finished-replace");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let mut run = test_run_json("run_finished_replace");
    run["state"] = serde_json::Value::from("ended");
    run["statusCode"] = serde_json::Value::from("ok");
    run["finishedAtMillis"] = serde_json::Value::from(200);
    let mut canonical_run = run.clone();
    crate::recording::wire::camel_case_keys_to_snake(&mut canonical_run);
    store
      .write_run_snapshot(&CanonicalRun {
        run: serde_json::from_value(canonical_run).expect("run record should decode"),
        spans: Vec::new(),
        events: Vec::new(),
        artifacts: Vec::new(),
      })
      .expect("finished snapshot should pre-exist");
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );

    let response = post_write_updates(
      app,
      "run_finished_replace",
      serde_json::json!({
        "updates": [{
          "type": "runFinished",
          "runId": "run_finished_replace",
          "run": run
        }]
      }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
      store
        .read_run("run_finished_replace")
        .expect("run should remain readable")
        .run
        .state,
      TraceState::Ended
    );
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_allows_no_token_when_configured() {
    let root = temp_dir("inspect-write-no-token");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: None,
        no_token: true,
      },
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "runStarted",
        "runId": "run_write_no_token",
        "run": test_run_json("run_write_no_token")
      }]
    });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_no_token/updates")
          .header("content-type", "application/json")
          .body(Body::from(body.to_string()))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(store.read_run("run_write_no_token").is_ok());
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_conflicting_run_metadata() {
    let root = temp_dir("inspect-write-conflict");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_test_run(&store, RunId::new("run_write_conflict"), None);
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      super::InspectWriteConfig {
        enabled: true,
        token: None,
        no_token: true,
      },
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "runStarted",
        "runId": "run_write_conflict",
        "run": test_run_json("run_write_conflict")
      }]
    });

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_conflict/updates")
          .header("content-type", "application/json")
          .body(Body::from(body.to_string()))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json error");
    assert_eq!(value["error"]["code"], "runConflict");
    assert_eq!(value["error"]["conflictKind"], "runMetadataMismatch");
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn write_lock_serializes_same_run_sections() {
    let locks = super::RunWriteLocks::default();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let first_locks = locks.clone();
    let first_active = active.clone();
    let first_peak = peak.clone();
    let second_locks = locks.clone();
    let second_active = active.clone();
    let second_peak = peak.clone();

    let first = tokio::spawn(async move {
      let _guard = first_locks.lock("run_serialized").await;
      let current = first_active.fetch_add(1, Ordering::SeqCst) + 1;
      first_peak.fetch_max(current, Ordering::SeqCst);
      tokio::time::sleep(std::time::Duration::from_millis(25)).await;
      first_active.fetch_sub(1, Ordering::SeqCst);
    });
    let second = tokio::spawn(async move {
      let _guard = second_locks.lock("run_serialized").await;
      let current = second_active.fetch_add(1, Ordering::SeqCst) + 1;
      second_peak.fetch_max(current, Ordering::SeqCst);
      tokio::time::sleep(std::time::Duration::from_millis(25)).await;
      second_active.fetch_sub(1, Ordering::SeqCst);
    });

    first.await.expect("first section should finish");
    second.await.expect("second section should finish");

    assert_eq!(peak.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn write_updates_rejects_nested_run_started_run_id_mismatch() {
    let root = temp_dir("inspect-write-nested-start-mismatch");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let recorder = Arc::new(BroadcastRunRecorder::new(16));
    let mut receiver = recorder.subscribe();
    let app = router_with_config(store.clone(), recorder, write_without_token());
    let body = serde_json::json!({
      "updates": [{
        "type": "runStarted",
        "runId": "run_outer",
        "run": test_run_json("run_inner")
      }]
    });

    let response = post_write_updates(app, "run_outer", body).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(store.read_run("run_outer").is_err());
    assert!(store.read_run("run_inner").is_err());
    assert!(receiver.try_recv().is_err());
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_nested_run_finished_run_id_mismatch() {
    let root = temp_dir("inspect-write-nested-finish-mismatch");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_running_test_run(&store, RunId::new("run_outer"));
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );
    let mut run = test_run_json("run_inner");
    run["state"] = serde_json::json!("ended");
    run["statusCode"] = serde_json::json!("ok");
    run["finishedAtMillis"] = serde_json::json!(101);
    let body = serde_json::json!({
      "updates": [{
        "type": "runFinished",
        "runId": "run_outer",
        "run": run
      }]
    });

    let response = post_write_updates(app, "run_outer", body).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(store.read_run("run_inner").is_err());
    let outer = store
      .read_run("run_outer")
      .expect("outer run should remain");
    assert_eq!(outer.run.state, TraceState::Running);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_event_after_run_finished() {
    let root = temp_dir("inspect-write-event-after-finished");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_test_run(&store, RunId::new("run_after_finished_event"), None);
    let recorder = Arc::new(BroadcastRunRecorder::new(16));
    let mut receiver = recorder.subscribe();
    let app = router_with_config(store.clone(), recorder, write_without_token());
    let body = serde_json::json!({
      "updates": [{
        "type": "eventAppended",
        "runId": "run_after_finished_event",
        "event": test_event_json("event_after_finished")
      }]
    });

    let response = post_write_updates(app, "run_after_finished_event", body).await;

    assert_conflict_kind(response, "runAlreadyFinished").await;
    let snapshot = store
      .read_run("run_after_finished_event")
      .expect("run should remain");
    assert!(
      snapshot
        .events
        .iter()
        .all(|event| event.event_id.as_str() != "event_after_finished")
    );
    assert!(receiver.try_recv().is_err());
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_artifact_after_run_finished() {
    let root = temp_dir("inspect-write-artifact-after-finished");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_test_run(&store, RunId::new("run_after_finished_artifact"), None);
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "artifactCreated",
        "runId": "run_after_finished_artifact",
        "artifact": test_artifact_json("artifact_after_finished")
      }]
    });

    let response = post_write_updates(app, "run_after_finished_artifact", body).await;

    assert_conflict_kind(response, "runAlreadyFinished").await;
    let snapshot = store
      .read_run("run_after_finished_artifact")
      .expect("run should remain");
    assert!(
      snapshot
        .artifacts
        .iter()
        .all(|artifact| artifact.artifact_id.as_str() != "artifact_after_finished")
    );
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_span_finish_after_run_finished() {
    let root = temp_dir("inspect-write-span-after-finished");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_test_run(&store, RunId::new("run_after_finished_span"), None);
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "spanFinished",
        "runId": "run_after_finished_span",
        "span": test_span_json("0000000000000001", None, "auv.inspect.server", "ended")
      }]
    });

    let response = post_write_updates(app, "run_after_finished_span", body).await;

    assert_conflict_kind(response, "runAlreadyFinished").await;
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_run_finished_immutable_metadata_mismatch() {
    let root = temp_dir("inspect-write-run-finish-metadata");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_running_test_run(&store, RunId::new("run_finish_metadata"));
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );
    let mut run = test_run_json("run_finish_metadata");
    run["traceId"] = serde_json::json!("00000000000000000000000000000002");
    run["state"] = serde_json::json!("ended");
    run["statusCode"] = serde_json::json!("ok");
    run["finishedAtMillis"] = serde_json::json!(101);
    let body = serde_json::json!({
      "updates": [{
        "type": "runFinished",
        "runId": "run_finish_metadata",
        "run": run
      }]
    });

    let response = post_write_updates(app, "run_finish_metadata", body).await;

    assert_conflict_kind(response, "runMetadataMismatch").await;
    let snapshot = store
      .read_run("run_finish_metadata")
      .expect("run should remain");
    assert_eq!(
      snapshot.run.trace_id.as_str(),
      "00000000000000000000000000000001"
    );
    assert_eq!(snapshot.run.state, TraceState::Running);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn write_updates_rejects_span_finished_immutable_metadata_mismatch() {
    let root = temp_dir("inspect-write-span-finish-metadata");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    write_running_test_run(&store, RunId::new("run_span_finish_metadata"));
    let app = router_with_config(
      store.clone(),
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );
    let body = serde_json::json!({
      "updates": [{
        "type": "spanFinished",
        "runId": "run_span_finish_metadata",
        "span": test_span_json("0000000000000001", None, "mutated.name", "ended")
      }]
    });

    let response = post_write_updates(app, "run_span_finish_metadata", body).await;

    assert_conflict_kind(response, "duplicateSpanMismatch").await;
    let snapshot = store
      .read_run("run_span_finish_metadata")
      .expect("run should remain");
    assert_eq!(snapshot.spans[0].name, "auv.inspect.server");
    assert_eq!(snapshot.spans[0].state, TraceState::Running);
    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn routes_return_canonical_records_and_artifact_bytes() {
    let root = temp_dir("inspect-server-routes");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_inspect_server_test");
    write_test_run(&store, run_id.clone(), Some("artifact_server_test.txt"));
    let artifact_path = root
      .join("runs")
      .join(run_id.as_str())
      .join("artifacts")
      .join("artifact_server_test.txt");
    fs::write(&artifact_path, "artifact body").expect("artifact should write");

    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));
    let run_response = app
      .clone()
      .oneshot(
        Request::builder()
          .uri("/runs/run_inspect_server_test")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(run_response.status(), StatusCode::OK);
    let run_body = to_bytes(run_response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let run: serde_json::Value = serde_json::from_slice(&run_body).expect("run should be json");
    assert_eq!(run["run_id"], "run_inspect_server_test");
    assert!(
      run.get("spans").is_none(),
      "/runs/{run_id} should return run metadata only"
    );

    let spans_response = app
      .clone()
      .oneshot(
        Request::builder()
          .uri("/runs/run_inspect_server_test/spans")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(spans_response.status(), StatusCode::OK);
    let spans_body = to_bytes(spans_response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let spans: serde_json::Value =
      serde_json::from_slice(&spans_body).expect("spans should be json");
    assert_eq!(spans[0]["name"], "auv.inspect.server");

    let artifact_response = app
      .oneshot(
        Request::builder()
          .uri("/runs/run_inspect_server_test/artifacts/artifact_server_test")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(artifact_response.status(), StatusCode::OK);
    assert_eq!(
      artifact_response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok()),
      Some("text/plain")
    );
    let artifact_body = to_bytes(artifact_response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    assert_eq!(&artifact_body[..], b"artifact body");

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn run_route_includes_read_side_verifications_and_observation_snapshots() {
    let root = temp_dir("inspect-server-run-read-side");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_inspect_server_contracts");
    write_test_run_with_read_side_contracts(&store, &root, run_id.clone());

    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));
    let run_response = app
      .oneshot(
        Request::builder()
          .uri("/runs/run_inspect_server_contracts")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(run_response.status(), StatusCode::OK);
    let run_body = to_bytes(run_response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let run: serde_json::Value = serde_json::from_slice(&run_body).expect("run should be json");
    assert_eq!(run["run_id"], "run_inspect_server_contracts");
    assert_eq!(run["verifications"][0]["method"]["kind"], "semantic_match");
    assert_eq!(
      run["observation_snapshots"][0]["snapshot_id"],
      "snapshot_server_test"
    );
    assert_eq!(
      run["validation_lineage"][0]["canonical_taxonomy_id"],
      "native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text"
    );
    assert_eq!(run["validation_lineage"][0]["legacy_taxonomy_alias"], true);
    assert_eq!(
      run["validation_lineage"][0]["observed_consumer"],
      "contract-candidate"
    );
    assert_eq!(
      run["validation_lineage"][0]["observed_candidate_local_id"],
      "native-text-focus-ax"
    );
    assert!(
      run.get("spans").is_none(),
      "/runs/{run_id} should not inline spans even when enriched"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_serves_inline_viewer_html() {
    let root = temp_dir("inspect-server-viewer");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
      response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok()),
      Some("text/html; charset=utf-8"),
    );
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");
    // Sanity: payload starts with a doctype, references /runs, and includes
    // the brand cyan token so it stays in sync with docs/design/.
    assert!(
      html.starts_with("<!doctype html>"),
      "expected HTML5 doctype, got prefix {:?}",
      &html[..32.min(html.len())]
    );
    assert!(
      html.contains("/runs"),
      "viewer payload should reference the /runs JSON endpoint"
    );
    assert!(
      html.contains("--brand: #00c4d2"),
      "viewer payload should inline the canonical brand token"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_payload_includes_span_tree_markers() {
    let root = temp_dir("inspect-server-viewer-span-tree");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");

    assert!(
      html.contains("span · name / step_id"),
      "viewer payload should include the C.2 span-tree header"
    );
    assert!(
      html.contains("loadRunDetail(runId)"),
      "viewer payload should fetch /runs/:id and /runs/:id/spans on selection"
    );
    assert!(
      html.contains("@keyframes auv-pulse"),
      "viewer payload should include running-span pulse animation"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_payload_includes_events_rail_markers() {
    let root = temp_dir("inspect-server-viewer-events-rail");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");

    // Layout shell for the events rail.
    assert!(
      html.contains("Events · events.jsonl"),
      "viewer payload should include the C.3a events rail header"
    );
    assert!(
      html.contains("id=\"events-rail\""),
      "viewer payload should mount the events rail container"
    );
    assert!(
      html.contains("id=\"span-detail\""),
      "viewer payload should mount the span detail panel above the rail"
    );
    assert!(
      html.contains("Select a span to inspect its attributes."),
      "viewer payload should include the empty-state span detail copy"
    );
    // Fetch wiring: events come in alongside spans on run selection.
    assert!(
      html.contains("/runs/:id/events"),
      "viewer payload should document the events endpoint"
    );
    assert!(
      html.contains("/events\")"),
      "viewer payload should fetch /runs/:id/events on selection"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_payload_includes_websocket_stream_markers() {
    let root = temp_dir("inspect-server-viewer-stream");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");

    // The viewer should open the documented /stream endpoint when a
    // running run is selected, and react to all five RunStreamEvent
    // tag values.
    assert!(
      html.contains("/runs/\" + encodeURIComponent(runId) + \"/stream"),
      "viewer payload should open ws on /runs/:id/stream"
    );
    for tag in [
      "span_started",
      "span_finished",
      "event_appended",
      "artifact_created",
      "run_finished",
    ] {
      assert!(
        html.contains(tag),
        "viewer payload should handle RunStreamEvent variant {tag}"
      );
    }
    // The "live" tint reserved in C.3a is now produced by streamed
    // events.
    assert!(
      html.contains("event-row.live") && html.contains("_live = true"),
      "viewer payload should tag streamed events as live"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_payload_includes_artifact_panel_markers() {
    let root = temp_dir("inspect-server-viewer-artifact-panel");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");

    assert!(
      html.contains("Artifacts · /artifacts"),
      "viewer payload should include the C.3b artifact panel header"
    );
    assert!(
      html.contains("id=\"artifact-panel\""),
      "viewer payload should mount the artifact panel container"
    );
    assert!(
      html.contains("id=\"artifact-list\""),
      "viewer payload should mount the artifact list"
    );
    assert!(
      html.contains("id=\"artifact-preview\""),
      "viewer payload should mount the artifact preview surface"
    );
    assert!(
      html.contains("/artifacts\")"),
      "viewer payload should fetch /runs/:id/artifacts on selection"
    );
    assert!(
      html.contains("encodeURIComponent(artifact.artifact_id)") && html.contains("spanId"),
      "viewer payload should reference the per-artifact bytes endpoint with span scoping"
    );
    assert!(
      html.contains("sprite-inspector.svg"),
      "viewer payload should use the inspector sprite for the empty preview state"
    );
    assert!(
      html.contains("click.overlay")
        && html.contains("click.overlay.annotation")
        && html.contains("evidence-summary"),
      "viewer payload should include click overlay evidence-aware preview helpers"
    );
    assert!(
      html.contains("defaultArtifactKey")
        && html.contains("artifactKey")
        && html.contains("preferredArtifactKeyForSpan")
        && html.contains("findClickOverlayAnnotationArtifact")
        && html.contains("loadEvidenceSummary")
        && html.contains("primary_error")
        && html.contains("payload.decision"),
      "viewer payload should prioritize click overlay artifacts, sync them to span selection, and render decision-aware annotation summaries"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn root_payload_includes_surface_node_preview_markers() {
    let root = temp_dir("inspect-server-viewer-surface-nodes");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    let response = app
      .oneshot(
        Request::builder()
          .uri("/")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let html = std::str::from_utf8(&body).expect("viewer payload should be utf-8");

    assert!(
      html.contains("surface-nodes"),
      "viewer payload should include the lightweight surface-node preview shell"
    );
    assert!(
      html.contains("renderSurfaceNodesPanel")
        && html.contains("surface-node-meta")
        && html.contains("node_ref"),
      "viewer payload should include the surface-node preview helper and shape accessors"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn assets_route_serves_known_design_svgs_with_svg_mime() {
    let root = temp_dir("inspect-server-assets-route");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    for name in [
      "logo-mark.svg",
      "sparkle.svg",
      "icon-png.svg",
      "icon-json.svg",
      "icon-bin.svg",
      "sprite-inspector.svg",
      "cursor-auv.svg",
      "cursor-auv-click.svg",
      "cursor-you.svg",
    ] {
      let response = app
        .clone()
        .oneshot(
          Request::builder()
            .uri(format!("/assets/{name}"))
            .body(Body::empty())
            .expect("request should build"),
        )
        .await
        .expect("route should respond");
      assert_eq!(response.status(), StatusCode::OK, "asset {name} should 200");
      assert_eq!(
        response
          .headers()
          .get("content-type")
          .and_then(|value| value.to_str().ok()),
        Some("image/svg+xml"),
        "asset {name} should serve as image/svg+xml",
      );
      let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
      assert!(
        body.starts_with(b"<svg"),
        "asset {name} should be an SVG; got prefix {:?}",
        &body[..16.min(body.len())]
      );
    }

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn assets_route_rejects_unknown_and_traversal_names() {
    let root = temp_dir("inspect-server-assets-route-deny");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));

    for bad in [
      "/assets/does-not-exist.svg",
      "/assets/..%2Fsecrets.toml",
      "/assets/.hidden",
    ] {
      let response = app
        .clone()
        .oneshot(
          Request::builder()
            .uri(bad)
            .body(Body::empty())
            .expect("request should build"),
        )
        .await
        .expect("route should respond");
      assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "{bad} should 404 (not traverse, not collide)"
      );
    }

    let _ = fs::remove_dir_all(root);
  }

  #[tokio::test]
  async fn stream_payload_filters_events_by_run_id() {
    let run_a = RunId::new("run_stream_a");
    let run_b = RunId::new("run_stream_b");
    let recorder = BroadcastRunRecorder::new(16);
    let mut receiver = recorder.subscribe();
    recorder
      .record(RunUpdate::EventAppended {
        run_id: run_b.clone(),
        event: test_event("event_stream_b"),
      })
      .expect("record should publish");
    recorder
      .record(RunUpdate::EventAppended {
        run_id: run_a.clone(),
        event: test_event("event_stream_a"),
      })
      .expect("record should publish");

    let payload = tokio::time::timeout(
      std::time::Duration::from_secs(2),
      next_stream_payload(&mut receiver, run_a.as_str()),
    )
    .await
    .expect("matching run event should arrive")
    .expect("matching run event should serialize");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("stream payload is json");
    assert_eq!(value["type"], "event_appended");
    assert!(payload.contains("run_stream_a"));
    assert!(payload.contains("event_stream_a"));
    assert!(!payload.contains("run_stream_b"));
  }

  #[tokio::test]
  async fn stream_rejects_missing_run_before_upgrade() {
    let root = temp_dir("inspect-server-missing-stream");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let error =
      ensure_stream_run_exists(&store, "run_missing").expect_err("missing run should reject");
    assert_eq!(error.status, StatusCode::NOT_FOUND);
    let _ = fs::remove_dir_all(root);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn artifact_endpoint_rejects_symlink_escape() {
    let root = temp_dir("inspect-server-symlink");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_symlink_escape");
    write_test_run(&store, run_id.clone(), Some("escape.txt"));
    let outside = root.join("outside.txt");
    fs::write(&outside, "secret").expect("outside file should write");
    let link = root
      .join("runs")
      .join(run_id.as_str())
      .join("artifacts")
      .join("escape.txt");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&outside, &link).expect("symlink should write");

    let app = router(store, Arc::new(BroadcastRunRecorder::new(16)));
    let response = app
      .oneshot(
        Request::builder()
          .uri("/runs/run_symlink_escape/artifacts/artifact_server_test")
          .body(Body::empty())
          .expect("request should build"),
      )
      .await
      .expect("route should respond");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let _ = fs::remove_dir_all(root);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn write_artifact_rejects_symlink_target() {
    let root = temp_dir("inspect-write-artifact-symlink");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = RunId::new("run_write_symlink_escape");
    write_test_run(&store, run_id.clone(), Some("escape.txt"));
    let outside = root.join("outside.txt");
    fs::write(&outside, "secret").expect("outside file should write");
    let link = root
      .join("runs")
      .join(run_id.as_str())
      .join("artifacts")
      .join("escape.txt");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&outside, &link).expect("symlink should write");
    let app = router_with_config(
      store,
      Arc::new(BroadcastRunRecorder::new(16)),
      write_without_token(),
    );

    let response = app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/write/runs/run_write_symlink_escape/artifacts/artifact_server_test")
          .body(Body::from("artifact body"))
          .expect("request should build"),
      )
      .await
      .expect("route should respond");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
      fs::read_to_string(outside).expect("outside file should remain untouched"),
      "secret"
    );
    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn test_run_json(run_id: &str) -> serde_json::Value {
    serde_json::json!({
      "apiVersion": RUN_API_VERSION,
      "runId": run_id,
      "traceId": "00000000000000000000000000000001",
      "runType": "execute",
      "state": "running",
      "statusCode": "unset",
      "startedAtMillis": 100,
      "finishedAtMillis": null,
      "rootSpanId": "0000000000000001",
      "attributes": {},
      "summary": null,
      "failure": null
    })
  }

  fn test_span_json(
    span_id: &str,
    parent_span_id: Option<&str>,
    name: &str,
    state: &str,
  ) -> serde_json::Value {
    serde_json::json!({
      "apiVersion": SPAN_API_VERSION,
      "spanId": span_id,
      "parentSpanId": parent_span_id,
      "name": name,
      "state": state,
      "statusCode": if state == "ended" { "ok" } else { "unset" },
      "startedAtMillis": 100,
      "finishedAtMillis": if state == "ended" {
        serde_json::Value::from(101)
      } else {
        serde_json::Value::Null
      },
      "attributes": {},
      "summary": null,
      "failure": null
    })
  }

  fn test_event_json(event_id: &str) -> serde_json::Value {
    serde_json::json!({
      "apiVersion": EVENT_API_VERSION,
      "eventId": event_id,
      "spanId": "0000000000000001",
      "name": "inspect.event",
      "timestampMillis": 101,
      "attributes": {},
      "message": null,
      "artifactIds": []
    })
  }

  fn test_artifact_json(artifact_id: &str) -> serde_json::Value {
    serde_json::json!({
      "apiVersion": ARTIFACT_API_VERSION,
      "artifactId": artifact_id,
      "spanId": "0000000000000001",
      "eventId": null,
      "role": "driver.output",
      "mimeType": "text/plain",
      "path": "artifacts/test.txt",
      "sha256": null,
      "attributes": {},
      "summary": null
    })
  }

  fn write_without_token() -> super::InspectWriteConfig {
    super::InspectWriteConfig {
      enabled: true,
      token: None,
      no_token: true,
    }
  }

  async fn post_write_updates(
    app: Router,
    run_id: &str,
    body: serde_json::Value,
  ) -> axum::response::Response {
    app
      .oneshot(
        Request::builder()
          .method("POST")
          .uri(format!("/write/runs/{run_id}/updates"))
          .header("content-type", "application/json")
          .body(Body::from(body.to_string()))
          .expect("request should build"),
      )
      .await
      .expect("route should respond")
  }

  async fn assert_conflict_kind(response: axum::response::Response, kind: &str) {
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), usize::MAX)
      .await
      .expect("body should read");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json error");
    assert_eq!(value["error"]["code"], "runConflict");
    assert_eq!(value["error"]["conflictKind"], kind);
  }

  fn write_running_test_run(store: &LocalStore, run_id: RunId) {
    let span_id = SpanId::new("0000000000000001");
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("00000000000000000000000000000001"),
          run_type: RunType::Execute,
          state: TraceState::Running,
          status_code: TraceStatusCode::Unset,
          started_at_millis: 100,
          finished_at_millis: None,
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: None,
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id: span_id.clone(),
          parent_span_id: None,
          name: "auv.inspect.server".to_string(),
          state: TraceState::Running,
          status_code: TraceStatusCode::Unset,
          started_at_millis: 100,
          finished_at_millis: None,
          attributes: BTreeMap::new(),
          summary: None,
          failure: None,
        }],
        events: Vec::new(),
        artifacts: Vec::new(),
      })
      .expect("run should persist");
  }

  fn write_test_run(store: &LocalStore, run_id: RunId, artifact_name: Option<&str>) {
    let span_id = SpanId::new("0000000000000001");
    let artifact_id = ArtifactId::new("artifact_server_test");
    let artifacts = artifact_name
      .map(|name| {
        vec![ArtifactRecordV1Alpha1 {
          api_version: ARTIFACT_API_VERSION.to_string(),
          artifact_id: artifact_id.clone(),
          span_id: span_id.clone(),
          event_id: None,
          role: "driver.output".to_string(),
          mime_type: "text/plain".to_string(),
          path: format!("artifacts/{name}"),
          sha256: None,
          attributes: BTreeMap::new(),
          summary: None,
        }]
      })
      .unwrap_or_default();
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("00000000000000000000000000000001"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 100,
          finished_at_millis: Some(101),
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: Some("done".to_string()),
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id: span_id.clone(),
          parent_span_id: None,
          name: "auv.inspect.server".to_string(),
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 100,
          finished_at_millis: Some(101),
          attributes: BTreeMap::new(),
          summary: None,
          failure: None,
        }],
        events: vec![EventRecordV1Alpha1 {
          api_version: EVENT_API_VERSION.to_string(),
          event_id: EventId::new("event_server_test"),
          span_id,
          name: "inspect.event".to_string(),
          timestamp_millis: 100,
          attributes: BTreeMap::new(),
          message: None,
          artifact_ids: artifacts
            .iter()
            .map(|artifact| artifact.artifact_id.clone())
            .collect(),
        }],
        artifacts,
      })
      .expect("run should persist");
  }

  fn write_test_run_with_read_side_contracts(store: &LocalStore, root: &Path, run_id: RunId) {
    let span_id = SpanId::new("0000000000000001");
    let run = RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: run_id.clone(),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(101),
      root_span_id: span_id.clone(),
      attributes: BTreeMap::new(),
      summary: Some("done".to_string()),
      failure: None,
    };
    let span = SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: span_id.clone(),
      parent_span_id: None,
      name: "auv.inspect.server".to_string(),
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(101),
      attributes: BTreeMap::new(),
      summary: None,
      failure: None,
    };
    let verification = VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("Now Playing".to_string()),
    };
    let operation_result = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run_id.clone(),
      status: OperationStatus::Completed,
      operation_id: "music.result.play".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Verification {
        verification: Box::new(verification.clone()),
      },
      verifications: vec![verification],
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let observation_snapshot = ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_server_test".to_string(),
      run_id: run_id.clone(),
      span_id: span_id.clone(),
      captured_at_millis: 100,
      source: ObservationSource::Visual,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      capture_contract_ref: None,
      evidence: Vec::new(),
      nodes: Vec::new(),
      detail: serde_json::json!({"producer": "scroll_scan"}),
      known_limits: vec!["visual only".to_string()],
    };
    let scroll_scan_artifact = ScrollScanArtifact {
      scan_id: "scan_server_test".to_string(),
      target: ScanTarget {
        application_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        region: ScanRegion {
          left_ratio: 0.1,
          top_ratio: 0.2,
          right_ratio: 0.9,
          bottom_ratio: 0.8,
        },
      },
      stop_policy: StopPolicy::Bounded {
        max_pages: 1,
        max_scrolls: 0,
      },
      pages: Vec::<ScanPageRecord>::new(),
      observations: Vec::<CollectionObservation>::new(),
      nodes: Vec::new(),
      snapshots: vec![observation_snapshot],
      clusters: Vec::<ObservationCluster>::new(),
      section_candidates: Vec::<SectionCandidate>::new(),
      scroll_boundary_candidates: Vec::<ScrollBoundaryCandidate>::new(),
      hook_decisions: Vec::<HookDecisionRecord>::new(),
      stop_evidence: StopEvidence {
        reason: StopReason::MaxPages,
        message: "bounded for test".to_string(),
        page_index: 0,
      },
      completeness_claim: CompletenessClaim::PartialMaxPages,
      warnings: Vec::new(),
    };
    let artifacts = vec![
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        0,
        "operation-result",
        "music-result-play.json",
        &operation_result,
      ),
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        1,
        "scroll-scan",
        "scroll-scan.json",
        &scroll_scan_artifact,
      ),
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        2,
        "validation.output",
        "validation.json",
        &AppValidation {
          validate_version: "v0".to_string(),
          created_at_millis: 0,
          source_distillation_path: "/tmp/distillation.json".into(),
          source_analysis_path: "/tmp/analysis.json".into(),
          app_identity: AppIdentity {
            bundle_id: "com.example.music".to_string(),
            app_name: "Example Music".to_string(),
            app_path: None,
            main_executable_path: None,
            version: "1.0".to_string(),
            build_version: "100".to_string(),
            url_schemes: Vec::new(),
            apple_script_addressable: false,
            launch_services_resolved: true,
            resolution_notes: Vec::new(),
          },
          candidates: vec![AppValidatedCandidate {
            recipe_id: "macos.textedit.native_text_candidate.v0".to_string(),
            taxonomy_id: NATIVE_TEXT_LEGACY_TAXONOMY_ID.to_string(),
            status: AppValidationStatus::Validated,
            verification_mode: AppVerificationMode::MachineAsserted,
            rationale: "test".to_string(),
            used_annotation_ids: Vec::new(),
            recipe_path: "/tmp/native-text.recipe.json".into(),
            case_matrix_path: "/tmp/native-text.cases.json".into(),
            selected_case_count: 1,
            observed_consumer: Some("contract-candidate".to_string()),
            observed_candidate_local_id: Some("native-text-focus-ax".to_string()),
            unresolved_inputs: Vec::new(),
            failure_message: None,
            resolved_inputs: BTreeMap::new(),
          }],
          known_boundaries: Vec::new(),
        },
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run should persist");
  }

  fn stage_json_artifact<T: serde::Serialize>(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    value: &T,
  ) -> ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    let rendered =
      serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
    fs::write(&source_path, rendered).expect("artifact source should write");
    store
      .stage_artifact_file(
        run_id,
        index,
        span_id,
        None,
        ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }

  fn write_test_run_with_duplicate_artifacts(store: &LocalStore, run_id: RunId) {
    let span_a = SpanId::new("0000000000000001");
    let span_b = SpanId::new("0000000000000002");
    let artifact_a = ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id: ArtifactId::new("artifact_dup"),
      span_id: span_a.clone(),
      event_id: Some(EventId::new("event_dup_a")),
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/artifact_dup_first.txt".to_string(),
      sha256: None,
      attributes: BTreeMap::new(),
      summary: Some("first".to_string()),
    };
    let artifact_b = ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id: ArtifactId::new("artifact_dup"),
      span_id: span_b.clone(),
      event_id: Some(EventId::new("event_dup_b")),
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/artifact_dup_second.txt".to_string(),
      sha256: None,
      attributes: BTreeMap::new(),
      summary: Some("second".to_string()),
    };
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("00000000000000000000000000000001"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 100,
          finished_at_millis: Some(101),
          root_span_id: span_a.clone(),
          attributes: BTreeMap::new(),
          summary: Some("done".to_string()),
          failure: None,
        },
        spans: vec![
          SpanRecordV1Alpha1 {
            api_version: SPAN_API_VERSION.to_string(),
            span_id: span_a.clone(),
            parent_span_id: None,
            name: "auv.inspect.server.first".to_string(),
            state: TraceState::Ended,
            status_code: TraceStatusCode::Ok,
            started_at_millis: 100,
            finished_at_millis: Some(101),
            attributes: BTreeMap::new(),
            summary: None,
            failure: None,
          },
          SpanRecordV1Alpha1 {
            api_version: SPAN_API_VERSION.to_string(),
            span_id: span_b.clone(),
            parent_span_id: None,
            name: "auv.inspect.server.second".to_string(),
            state: TraceState::Ended,
            status_code: TraceStatusCode::Ok,
            started_at_millis: 100,
            finished_at_millis: Some(101),
            attributes: BTreeMap::new(),
            summary: None,
            failure: None,
          },
        ],
        events: vec![
          EventRecordV1Alpha1 {
            api_version: EVENT_API_VERSION.to_string(),
            event_id: EventId::new("event_dup_a"),
            span_id: span_a.clone(),
            name: "inspect.event".to_string(),
            timestamp_millis: 100,
            attributes: BTreeMap::new(),
            message: None,
            artifact_ids: vec![artifact_a.artifact_id.clone()],
          },
          EventRecordV1Alpha1 {
            api_version: EVENT_API_VERSION.to_string(),
            event_id: EventId::new("event_dup_b"),
            span_id: span_b.clone(),
            name: "inspect.event".to_string(),
            timestamp_millis: 101,
            attributes: BTreeMap::new(),
            message: None,
            artifact_ids: vec![artifact_b.artifact_id.clone()],
          },
        ],
        artifacts: vec![artifact_a, artifact_b],
      })
      .expect("run should persist");
    let run_dir = store
      .run_dir(run_id.as_str())
      .expect("run dir should resolve");
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("artifact dir should create");
    fs::write(
      artifacts_dir.join("artifact_dup_first.txt"),
      "first artifact",
    )
    .expect("first artifact should write");
    fs::write(
      artifacts_dir.join("artifact_dup_second.txt"),
      "second artifact",
    )
    .expect("second artifact should write");
  }

  fn test_event(event_id: &str) -> EventRecordV1Alpha1 {
    EventRecordV1Alpha1 {
      api_version: EVENT_API_VERSION.to_string(),
      event_id: EventId::new(event_id),
      span_id: SpanId::new("0000000000000001"),
      name: "inspect.event".to_string(),
      timestamp_millis: 100,
      attributes: BTreeMap::new(),
      message: None,
      artifact_ids: Vec::new(),
    }
  }
}
