//! HTTP composition root for one canonical Inspect [`RunStore`] authority.

use std::net::SocketAddr;
use std::process;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use auv_tracing::RunStore;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, MutexGuard};

use crate::InspectResult;
use crate::run_api;
use crate::session::{InspectServerSession, write_inspect_session};
use crate::viewer_assets::{VIEWER_HTML, viewer_asset};

pub const DEFAULT_INSPECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_INSPECT_PORT: u16 = 8765;

pub(crate) struct InspectServerState {
  pub(crate) store: Arc<dyn RunStore>,
  // NOTICE(run-api-replay-status): RunStore returns a commit without whether
  // it was newly accepted or replayed. Serialize HTTP lookup+commit pairs to
  // preserve exact 201/200 responses; remove this when the port returns that
  // disposition atomically.
  commit_status_lock: Mutex<()>,
}

impl InspectServerState {
  pub(crate) async fn commit_status_lock(&self) -> MutexGuard<'_, ()> {
    self.commit_status_lock.lock().await
  }
}

#[derive(Clone, Debug)]
pub struct InspectServeConfig {
  pub host: String,
  pub port: u16,
}

impl Default for InspectServeConfig {
  fn default() -> Self {
    Self {
      host: DEFAULT_INSPECT_HOST.to_string(),
      port: DEFAULT_INSPECT_PORT,
    }
  }
}

const DESIGN_ASSETS: &[(&str, &[u8], &str)] = &[
  ("logo-mark.svg", include_bytes!("../../../docs/design/assets/logo-mark.svg"), "image/svg+xml"),
  ("sparkle.svg", include_bytes!("../../../docs/design/assets/sparkle.svg"), "image/svg+xml"),
  ("icon-png.svg", include_bytes!("../../../docs/design/assets/icon-png.svg"), "image/svg+xml"),
  ("icon-json.svg", include_bytes!("../../../docs/design/assets/icon-json.svg"), "image/svg+xml"),
  ("icon-bin.svg", include_bytes!("../../../docs/design/assets/icon-bin.svg"), "image/svg+xml"),
];

/// Builds the Inspect viewer and V1 run protocol around one authority store.
pub fn router(store: Arc<dyn RunStore>) -> Router {
  router_with_projection(store)
}

/// Builds the server after the legacy projection boundary has been replaced by snapshot projection.
pub fn router_with_projection(store: Arc<dyn RunStore>) -> Router {
  let state = Arc::new(InspectServerState {
    store,
    commit_status_lock: Mutex::new(()),
  });
  Router::new()
    .route("/", get(serve_viewer))
    .route("/viewer-assets/{*asset_name}", get(serve_viewer_asset))
    .route("/assets/{asset_name}", get(serve_design_asset))
    .merge(run_api::routes())
    .with_state(state)
}

/// Binds and serves one Inspect authority, publishing its discovery session.
pub async fn serve(store: Arc<dyn RunStore>, config: InspectServeConfig) -> InspectResult<SocketAddr> {
  let address = (config.host.as_str(), config.port);
  let display_address = format!("{}:{}", config.host, config.port);
  let listener = TcpListener::bind(address).await.map_err(|error| format!("failed to bind inspect server {display_address}: {error}"))?;
  let local_address = listener.local_addr().map_err(|error| format!("failed to read inspect server address: {error}"))?;
  println!("inspect server: http://{local_address}");
  write_inspect_session(&InspectServerSession {
    url: format!("http://{local_address}"),
    authority_id: store.authority_id(),
    pid: process::id(),
    started_at_millis: now_millis(),
  })?;
  axum::serve(listener, router(store)).await.map_err(|error| format!("inspect server failed: {error}"))?;
  Ok(local_address)
}

async fn serve_viewer() -> Response {
  response_with_content(Body::from(VIEWER_HTML), "text/html; charset=utf-8")
}

async fn serve_viewer_asset(Path(asset_name): Path<String>) -> Response {
  match viewer_asset(&asset_name) {
    Some((bytes, mime)) => response_with_content(Body::from(bytes), mime),
    None => StatusCode::NOT_FOUND.into_response(),
  }
}

async fn serve_design_asset(State(_state): State<Arc<InspectServerState>>, Path(asset_name): Path<String>) -> Response {
  if asset_name.is_empty() || asset_name.contains(['/', '\\']) || asset_name.contains("..") || asset_name.starts_with('.') {
    return StatusCode::NOT_FOUND.into_response();
  }
  match DESIGN_ASSETS.iter().find(|(name, _, _)| *name == asset_name) {
    Some((_, bytes, mime)) => response_with_content(Body::from(*bytes), mime),
    None => StatusCode::NOT_FOUND.into_response(),
  }
}

fn response_with_content(body: Body, content_type: &'static str) -> Response {
  let mut response = body.into_response();
  response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
  response
}

fn now_millis() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}
