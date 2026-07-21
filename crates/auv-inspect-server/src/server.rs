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
use url::Url;

use crate::InspectResult;
use crate::run_api;
use crate::session::{InspectServerSession, write_inspect_session};
use crate::viewer_assets::{VIEWER_HTML, viewer_asset};

#[path = "artifact_api.rs"]
mod artifact_api;
use artifact_api::ArtifactApiState;

pub const DEFAULT_INSPECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_INSPECT_PORT: u16 = 8765;

pub(crate) struct InspectServerState {
  pub(crate) store: Arc<dyn RunStore>,
  pub(crate) artifacts: ArtifactApiState,
  pub(crate) artifact_origin: Option<Url>,
  /// Serializes short ordinary-commit and draft-reservation mutations.
  ///
  /// Artifact bodies are never streamed while this gate is held.
  pub(crate) mutation_gate: tokio::sync::Mutex<()>,
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

/// Builds the Inspect viewer and V1 run protocol for trusted in-process composition.
///
/// This router has no authentication layer. Transport callers must use
/// [`serve`], which enforces loopback binding, or install an independently
/// reviewed access-control boundary before exposing it.
pub fn router(store: Arc<dyn RunStore>) -> Router {
  build_router(store, None)
}

/// Builds the Inspect router with a trusted public artifact origin.
///
/// This router has no authentication layer. Transport callers must use
/// `serve`, which enforces loopback binding, or install an independently
/// reviewed access-control boundary before exposing it. The artifact origin
/// must come from that trusted composition boundary, never request headers.
pub fn router_with_artifact_origin(store: Arc<dyn RunStore>, artifact_origin: Url) -> InspectResult<Router> {
  validate_artifact_origin(&artifact_origin)?;
  Ok(build_router(store, Some(artifact_origin)))
}

fn build_router(store: Arc<dyn RunStore>, artifact_origin: Option<Url>) -> Router {
  let state = Arc::new(InspectServerState {
    store,
    artifacts: ArtifactApiState::new(),
    artifact_origin,
    mutation_gate: tokio::sync::Mutex::new(()),
  });
  Router::new()
    .route("/", get(serve_viewer))
    .route("/viewer-assets/{*asset_name}", get(serve_viewer_asset))
    .route("/assets/{asset_name}", get(serve_design_asset))
    .merge(run_api::routes())
    .merge(artifact_api::routes())
    .with_state(state)
}

fn validate_artifact_origin(origin: &Url) -> InspectResult<()> {
  if !matches!(origin.scheme(), "http" | "https")
    || !origin.username().is_empty()
    || origin.password().is_some()
    || origin.host_str().is_none()
    || origin.path() != "/"
    || origin.query().is_some()
    || origin.fragment().is_some()
  {
    return Err("artifact origin must be an absolute credential-free HTTP(S) origin with no path, query, or fragment".to_string());
  }
  Ok(())
}

/// Binds one loopback-only Inspect authority and publishes its discovery session.
pub async fn serve(store: Arc<dyn RunStore>, config: InspectServeConfig) -> InspectResult<SocketAddr> {
  let display_address = format!("{}:{}", config.host, config.port);
  let addresses = tokio::net::lookup_host((config.host.as_str(), config.port))
    .await
    .map_err(|error| format!("failed to resolve inspect server {display_address}: {error}"))?
    .collect::<Vec<_>>();
  if addresses.is_empty() {
    return Err(format!("inspect server {display_address} resolved no listen addresses"));
  }
  // NOTICE(inspect-loopback-v1): V1 defines no authentication credential.
  // Keep the standard transport loopback-only until an accepted auth contract
  // and its threat-model tests authorize remote binding.
  if let Some(address) = addresses.iter().find(|address| !address.ip().is_loopback()) {
    return Err(format!("inspect server V1 is loopback-only; {display_address} resolved non-loopback address {address}"));
  }
  let listener =
    TcpListener::bind(addresses.as_slice()).await.map_err(|error| format!("failed to bind inspect server {display_address}: {error}"))?;
  let local_address = listener.local_addr().map_err(|error| format!("failed to read inspect server address: {error}"))?;
  let artifact_origin =
    Url::parse(&format!("http://{local_address}/")).map_err(|error| format!("failed to construct inspect artifact origin: {error}"))?;
  let app = router_with_artifact_origin(store.clone(), artifact_origin)?;
  println!("inspect server: http://{local_address}");
  write_inspect_session(&InspectServerSession {
    url: format!("http://{local_address}"),
    authority_id: store.authority_id(),
    pid: process::id(),
    started_at_millis: now_millis(),
  })?;
  axum::serve(listener, app).await.map_err(|error| format!("inspect server failed: {error}"))?;
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
