//! Protobuf-over-HTTP resource routes backed by the shared control plane.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use prost::Message;
use tonic::{Code, Status};

use crate::auth::CallerId;
use crate::daemon::{Daemon, DaemonError};
use crate::server::RequestAuth;

const PROTOBUF_CONTENT_TYPE: &str = "application/protobuf";

#[derive(Clone)]
struct RestState {
  daemon: Arc<Daemon>,
  auth: RequestAuth,
}

pub(crate) fn router(daemon: Arc<Daemon>, auth: RequestAuth) -> Router {
  // TODO(rest-transcoding-v1): these handwritten routes map canonical protobuf
  // messages without creating a second Rust domain model. Replace the mapping
  // with google.api.http + Protovalidate + OpenAPI generation when that Buf
  // dependency/toolchain slice is owner-approved.
  // TODO(websocket-events): no WebSocket route is exposed until a concrete
  // non-video event consumer defines ordering, cursor, gap recovery, and
  // cancellation semantics. Video/frame streaming is a separate future slice.
  Router::new()
    .route("/apis", get(list_api_namespaces))
    .route("/apis/auv", get(get_auv_api_namespace))
    .route("/apis/auv/{group}/{version}", get(get_auv_api_group_version))
    .route("/apis/auv/daemon/v1/devices", get(list_devices))
    .route("/apis/auv/daemon/v1/devices/{device_id}", get(get_device))
    .route("/apis/auv/runtime/v1/runs", post(create_run).get(list_runs))
    .route("/apis/auv/runtime/v1/runs/{run_id}", get(get_run))
    .route("/apis/auv/runtime/v1/runs/{run_id}/stop", post(stop_run))
    .route("/apis/auv/runtime/v1/runners", post(create_runner).get(list_runners))
    .route("/apis/auv/runtime/v1/runners/{runner_id}", get(get_runner).delete(delete_runner))
    .route("/apis/auv/runtime/v1/runnerclasses", get(list_runner_classes))
    .route("/apis/auv/runtime/v1/runnerclasses/{runner_class}", get(get_runner_class))
    .with_state(RestState { daemon, auth })
}

async fn list_api_namespaces(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::ListApiNamespacesResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  Ok(Protobuf(daemon_proto::ListApiNamespacesResponse {
    namespaces: vec![daemon_proto::ApiNamespace {
      name: "auv".to_string(),
    }],
  }))
}

async fn get_auv_api_namespace(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetApiNamespaceResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  Ok(Protobuf(daemon_proto::GetApiNamespaceResponse {
    namespace: "auv".to_string(),
    groups: vec![
      daemon_proto::ApiGroup {
        name: "daemon".to_string(),
        versions: vec!["v1".to_string()],
      },
      daemon_proto::ApiGroup {
        name: "runtime".to_string(),
        versions: vec!["v1".to_string()],
      },
    ],
  }))
}

async fn get_auv_api_group_version(
  State(state): State<RestState>,
  Path((group, version)): Path<(String, String)>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetApiGroupVersionResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  let resources = match (group.as_str(), version.as_str()) {
    ("daemon", "v1") => vec![api_resource(
      "devices",
      "Device",
      &[
        daemon_proto::ApiResourceOperation::List,
        daemon_proto::ApiResourceOperation::Get,
      ],
    )],
    ("runtime", "v1") => {
      let mut runner_operations = vec![
        daemon_proto::ApiResourceOperation::List,
        daemon_proto::ApiResourceOperation::Get,
      ];
      if !state.daemon.list_runner_classes(None)?.runner_classes.is_empty() {
        runner_operations.extend([
          daemon_proto::ApiResourceOperation::Create,
          daemon_proto::ApiResourceOperation::Delete,
        ]);
      }
      let mut run_operations = vec![
        daemon_proto::ApiResourceOperation::List,
        daemon_proto::ApiResourceOperation::Get,
      ];
      run_operations.extend([
        daemon_proto::ApiResourceOperation::Create,
        daemon_proto::ApiResourceOperation::Delete,
      ]);
      vec![
        api_resource("runners", "Runner", &runner_operations),
        api_resource(
          "runnerclasses",
          "RunnerClass",
          &[
            daemon_proto::ApiResourceOperation::List,
            daemon_proto::ApiResourceOperation::Get,
          ],
        ),
        api_resource("runs", "Run", &run_operations),
      ]
    }
    _ => return Err(RestError::new(StatusCode::NOT_FOUND, "not_found", "unknown AUV API group or version")),
  };
  Ok(Protobuf(daemon_proto::GetApiGroupVersionResponse {
    namespace: "auv".to_string(),
    group,
    version,
    resources,
  }))
}

fn api_resource(name: &str, kind: &str, operations: &[daemon_proto::ApiResourceOperation]) -> daemon_proto::ApiResource {
  daemon_proto::ApiResource {
    name: name.to_string(),
    kind: kind.to_string(),
    operations: operations.iter().map(|operation| *operation as i32).collect(),
  }
}

async fn list_devices(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::ListDevicesResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  Ok(Protobuf(state.daemon.list_devices()))
}

async fn get_device(
  State(state): State<RestState>,
  Path(device_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetDeviceResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  let device = state
    .daemon
    .get_device(&device_id)
    .ok_or_else(|| RestError::new(StatusCode::NOT_FOUND, "not_found", format!("unknown Device: {device_id}")))?;
  Ok(Protobuf(daemon_proto::GetDeviceResponse {
    device: Some(device),
  }))
}

async fn create_run(State(state): State<RestState>, request: Request<Body>) -> Result<Protobuf<daemon_proto::CreateRunResponse>, RestError> {
  let caller = authenticate(&state, &request)?;
  let request = decode_protobuf_body(request).await?;
  state.daemon.create_run(&caller, request).map(Protobuf).map_err(RestError::from)
}

async fn list_runs(State(state): State<RestState>, request: Request<Body>) -> Result<Protobuf<daemon_proto::ListRunsResponse>, RestError> {
  let caller = authenticate(&state, &request)?;
  Ok(Protobuf(state.daemon.list_runs(&caller)))
}

async fn get_run(
  State(state): State<RestState>,
  Path(run_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetRunResponse>, RestError> {
  let caller = authenticate(&state, &request)?;
  state.daemon.get_run(&caller, &run_id).map(Protobuf).map_err(RestError::from)
}

async fn stop_run(
  State(state): State<RestState>,
  Path(run_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::StopRunResponse>, RestError> {
  let caller = authenticate(&state, &request)?;
  let request = decode_protobuf_body::<daemon_proto::StopRunRequest>(request).await?;
  if request.run.as_ref().is_some_and(|run| run.run_id != run_id) {
    return Err(RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", "path Run and request Run differ"));
  }
  let outcome = daemon_proto::RunOutcome::try_from(request.outcome)
    .map_err(|_| RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", "Run outcome is unknown"))?;
  state.daemon.stop_run(&caller, &run_id, outcome).await.map(Protobuf).map_err(RestError::from)
}

async fn create_runner(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::CreateRunnerResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  let request = decode_protobuf_body(request).await?;
  state.daemon.create_runner(request).await.map(Protobuf).map_err(RestError::from)
}

async fn list_runners(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::ListRunnersResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  Ok(Protobuf(state.daemon.list_runners()))
}

async fn list_runner_classes(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::ListRunnerClassesResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  state.daemon.list_runner_classes(None).map(Protobuf).map_err(RestError::from)
}

async fn get_runner_class(
  State(state): State<RestState>,
  Path(runner_class): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetRunnerClassResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  state.daemon.get_runner_class(None, &runner_class).map(Protobuf).map_err(RestError::from)
}

async fn get_runner(
  State(state): State<RestState>,
  Path(runner_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::GetRunnerResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  state.daemon.get_runner(&runner_id).map(Protobuf).map_err(RestError::from)
}

async fn delete_runner(
  State(state): State<RestState>,
  Path(runner_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<daemon_proto::DeleteRunnerResponse>, RestError> {
  let _caller = authenticate(&state, &request)?;
  state.daemon.delete_runner(&runner_id, None, false).await.map(Protobuf).map_err(RestError::from)
}

fn authenticate(state: &RestState, request: &Request<Body>) -> Result<CallerId, RestError> {
  state.auth.authenticate_http(request).map_err(RestError::from)
}

async fn decode_protobuf_body<M: Message + Default>(request: Request<Body>) -> Result<M, RestError> {
  let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
  if content_type.split(';').next() != Some(PROTOBUF_CONTENT_TYPE) {
    return Err(RestError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "request body must use application/protobuf"));
  }
  // No AUV-specific request ceiling is imposed here. Listener operators may
  // add a transport policy when they actually need one; otherwise allocation
  // and transport failures surface from Axum/the operating system.
  let bytes = to_bytes(request.into_body(), usize::MAX)
    .await
    .map_err(|error| RestError::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string()))?;
  M::decode(bytes).map_err(|error| RestError::new(StatusCode::BAD_REQUEST, "invalid_protobuf", error.to_string()))
}

struct Protobuf<M>(M);

impl<M: Message> IntoResponse for Protobuf<M> {
  fn into_response(self) -> Response {
    let mut response = self.0.encode_to_vec().into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(PROTOBUF_CONTENT_TYPE));
    response
  }
}

struct RestError {
  status: StatusCode,
  code: &'static str,
  detail: String,
}

impl RestError {
  fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> Self {
    Self {
      status,
      code,
      detail: detail.into(),
    }
  }
}

impl From<Status> for RestError {
  fn from(status: Status) -> Self {
    let (http_status, code) = match status.code() {
      Code::InvalidArgument => (StatusCode::BAD_REQUEST, "invalid_argument"),
      Code::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
      Code::PermissionDenied => (StatusCode::FORBIDDEN, "permission_denied"),
      Code::NotFound => (StatusCode::NOT_FOUND, "not_found"),
      Code::FailedPrecondition => (StatusCode::CONFLICT, "failed_precondition"),
      Code::ResourceExhausted => (StatusCode::TOO_MANY_REQUESTS, "resource_exhausted"),
      Code::Unimplemented => (StatusCode::NOT_IMPLEMENTED, "unimplemented"),
      Code::Cancelled => (StatusCode::from_u16(499).expect("valid client-closed status"), "cancelled"),
      _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    Self::new(http_status, code, status.message())
  }
}

impl From<DaemonError> for RestError {
  fn from(error: DaemonError) -> Self {
    match error {
      DaemonError::Identity(_) => Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", error.to_string()),
      DaemonError::InvalidArgument(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_argument", error.to_string()),
      DaemonError::UnknownDevice(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_argument", error.to_string()),
      DaemonError::UnknownRun(_) => Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string()),
      DaemonError::UnknownRunner(_) => Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string()),
      DaemonError::RunnerProviderUnavailable(_) => Self::new(StatusCode::NOT_IMPLEMENTED, "unimplemented", error.to_string()),
      DaemonError::RunnerOperation(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", error.to_string()),
    }
  }
}

impl IntoResponse for RestError {
  fn into_response(self) -> Response {
    let body = serde_json::json!({
      "type": format!("urn:auv:error:{}", self.code),
      "title": self.code,
      "status": self.status.as_u16(),
      "detail": self.detail,
    });
    (self.status, [(header::CONTENT_TYPE, HeaderValue::from_static("application/problem+json"))], body.to_string()).into_response()
  }
}

#[cfg(test)]
#[path = "rest_test.rs"]
mod tests;
