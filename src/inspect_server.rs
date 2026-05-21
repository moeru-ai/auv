use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::model::AuvResult;
use crate::recording::BroadcastRunEventSink;
use crate::store::LocalStore;

pub const DEFAULT_INSPECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_INSPECT_PORT: u16 = 8765;

#[derive(Clone)]
struct InspectServerState {
  store: Arc<LocalStore>,
  event_sink: Arc<BroadcastRunEventSink>,
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

/// Single-payload HTML viewer served at `GET /`. Inlines CSS + JS + the
/// pixel-art logo SVG; consumes the same `/runs` JSON contract any other
/// client would. Visual tokens match `docs/design/colors_and_type.css`;
/// when the canonical tokens drift, sync the inlined `:root` block in the
/// embedded HTML.
const VIEWER_HTML: &str = include_str!("inspect_server_viewer.html");

pub fn router(store: LocalStore, event_sink: Arc<BroadcastRunEventSink>) -> Router {
  let state = InspectServerState {
    store: Arc::new(store),
    event_sink,
  };
  Router::new()
    .route("/", get(serve_viewer))
    .route("/runs", get(list_runs))
    .route("/runs/{run_id}", get(get_run))
    .route("/runs/{run_id}/spans", get(get_spans))
    .route("/runs/{run_id}/events", get(get_events))
    .route("/runs/{run_id}/artifacts", get(get_artifacts))
    .route("/runs/{run_id}/artifacts/{artifact_id}", get(get_artifact))
    .route("/runs/{run_id}/stream", get(stream_run))
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

pub async fn serve(
  store: LocalStore,
  event_sink: Arc<BroadcastRunEventSink>,
  config: InspectServeConfig,
) -> AuvResult<SocketAddr> {
  let address = format!("{}:{}", config.host, config.port)
    .parse::<SocketAddr>()
    .map_err(|error| format!("invalid inspect server address: {error}"))?;
  let listener = TcpListener::bind(address)
    .await
    .map_err(|error| format!("failed to bind inspect server {address}: {error}"))?;
  let local_address = listener
    .local_addr()
    .map_err(|error| format!("failed to read inspect server address: {error}"))?;
  println!("inspect server: http://{local_address}");
  axum::serve(listener, router(store, event_sink))
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
  Ok(Json(run.run).into_response())
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
) -> Result<Response, InspectHttpError> {
  let (artifact, path) = state
    .store
    .artifact_file(&run_id, &artifact_id)
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
      .on_upgrade(move |socket| stream_run_events(socket, state.event_sink, run_id))
      .into_response(),
  )
}

fn ensure_stream_run_exists(store: &LocalStore, run_id: &str) -> Result<(), InspectHttpError> {
  store
    .read_run(run_id)
    .map(|_| ())
    .map_err(InspectHttpError::from_store)
}

async fn stream_run_events(
  mut socket: WebSocket,
  event_sink: Arc<BroadcastRunEventSink>,
  run_id: String,
) {
  let mut receiver = event_sink.subscribe();
  while let Some(payload) = next_stream_payload(&mut receiver, &run_id).await {
    if socket.send(Message::Text(payload.into())).await.is_err() {
      break;
    }
  }
}

async fn next_stream_payload(
  receiver: &mut broadcast::Receiver<crate::recording::RunStreamEvent>,
  run_id: &str,
) -> Option<String> {
  loop {
    match receiver.recv().await {
      Ok(event) if event.run_id().as_str() == run_id => match serde_json::to_string(&event) {
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
}

impl InspectHttpError {
  fn from_store(error: String) -> Self {
    let status = if error.contains("invalid run id") {
      StatusCode::BAD_REQUEST
    } else if error.contains("escapes run directory") {
      StatusCode::FORBIDDEN
    } else if error.contains("failed to read") || error.contains("not found") {
      StatusCode::NOT_FOUND
    } else {
      StatusCode::INTERNAL_SERVER_ERROR
    };
    Self {
      status,
      message: error,
    }
  }

  fn not_found(message: String) -> Self {
    Self {
      status: StatusCode::NOT_FOUND,
      message,
    }
  }
}

impl IntoResponse for InspectHttpError {
  fn into_response(self) -> Response {
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
  use std::sync::Arc;

  use axum::body::{Body, to_bytes};
  use axum::http::{Request, StatusCode};
  use tower::ServiceExt;

  use super::{ensure_stream_run_exists, next_stream_payload, router};
  use crate::model::now_millis;
  use crate::recording::{BroadcastRunEventSink, RunEventSink, RunStreamEvent};
  use crate::store::{CanonicalRun, LocalStore};
  use crate::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
    EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

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

    let app = router(store, Arc::new(BroadcastRunEventSink::new(16)));
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
  async fn root_serves_inline_viewer_html() {
    let root = temp_dir("inspect-server-viewer");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let app = router(store, Arc::new(BroadcastRunEventSink::new(16)));

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
    let app = router(store, Arc::new(BroadcastRunEventSink::new(16)));

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
    let app = router(store, Arc::new(BroadcastRunEventSink::new(16)));

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
  async fn stream_payload_filters_events_by_run_id() {
    let run_a = RunId::new("run_stream_a");
    let run_b = RunId::new("run_stream_b");
    let sink = BroadcastRunEventSink::new(16);
    let mut receiver = sink.subscribe();
    sink.on_event(RunStreamEvent::EventAppended {
      run_id: run_b.clone(),
      event: test_event("event_stream_b"),
    });
    sink.on_event(RunStreamEvent::EventAppended {
      run_id: run_a.clone(),
      event: test_event("event_stream_a"),
    });

    let payload = tokio::time::timeout(
      std::time::Duration::from_secs(2),
      next_stream_payload(&mut receiver, run_a.as_str()),
    )
    .await
    .expect("matching run event should arrive")
    .expect("matching run event should serialize");
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

    let app = router(store, Arc::new(BroadcastRunEventSink::new(16)));
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

  fn temp_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
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
          run_id,
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
