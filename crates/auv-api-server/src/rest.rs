//! Protobuf-over-HTTP resource routes backed by the shared control plane.

use std::sync::Arc;

use auv_api_proto::auv::api::core::v1 as core_proto;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use prost::Message;
use tonic::{Code, Status};

use crate::authority::ApiScope;
use crate::authority::PrincipalId;
use crate::control_plane::ControlPlaneError;
use crate::handler::ApiHandler;
use crate::transport::RequestAuthority;

// NOTICE(rest-body-limit): RGB frames travel as protobuf bytes in this slice,
// so the transport admits images larger than Axum's small default body limit.
// Replace this fixed ceiling with operation metadata when typed capture and
// artifact-upload APIs define their own payload limits.
const MAX_PROTOBUF_BODY_BYTES: usize = 256 * 1024 * 1024;
const MAX_CONTROL_PROTOBUF_BODY_BYTES: usize = 64 * 1024;
const PROTOBUF_CONTENT_TYPE: &str = "application/protobuf";

#[derive(Clone)]
struct RestState {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

pub(crate) fn router(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Router {
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
    .route("/apis/auv/core/v1/services", get(list_services))
    .route("/apis/auv/{group}/{version}", get(get_auv_api_group_version))
    .route("/apis/auv/core/v1/devices", get(list_devices))
    .route("/apis/auv/core/v1/devices/{device_id}", get(get_device))
    .route("/apis/auv/runtime/v1/runs", post(create_run).get(list_runs))
    .route("/apis/auv/runtime/v1/runs/{run_id}", get(get_run))
    .route("/apis/auv/runtime/v1/runs/{run_id}/stop", post(stop_run))
    .route("/apis/auv/runtime/v1/runs/{run_id}/runnerleases", post(claim_runner))
    .route("/apis/auv/runtime/v1/runs/{run_id}/runnerleases/{lease_id}", delete(release_runner_lease_for_run))
    .route("/apis/auv/runtime/v1/runners", post(create_runner).get(list_runners))
    .route("/apis/auv/runtime/v1/runners/{runner_id}", get(get_runner).delete(delete_runner))
    .route("/apis/auv/runtime/v1/runnerclasses", get(list_runner_classes))
    .route("/apis/auv/runtime/v1/runnerclasses/{runner_class}", get(get_runner_class))
    .with_state(RestState { handler, authority })
}

async fn list_api_namespaces(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ListApiNamespacesResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  Ok(Protobuf(core_proto::ListApiNamespacesResponse {
    namespaces: vec![core_proto::ApiNamespace {
      name: "auv".to_string(),
    }],
  }))
}

async fn get_auv_api_namespace(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetApiNamespaceResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  Ok(Protobuf(core_proto::GetApiNamespaceResponse {
    namespace: "auv".to_string(),
    groups: vec![
      core_proto::ApiGroup {
        name: "core".to_string(),
        versions: vec!["v1".to_string()],
      },
      core_proto::ApiGroup {
        name: "runtime".to_string(),
        versions: vec!["v1".to_string()],
      },
    ],
  }))
}

async fn list_services(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ListServicesResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  if state.authority.principal_from_extensions(request.extensions(), ApiScope::OperationsExecute).is_err() {
    return Ok(Protobuf(core_proto::ListServicesResponse {
      services: Vec::new(),
    }));
  }
  Ok(Protobuf(state.handler.control_plane().list_services()))
}

async fn get_auv_api_group_version(
  State(state): State<RestState>,
  Path((group, version)): Path<(String, String)>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetApiGroupVersionResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  let can_manage = state.authority.principal_from_extensions(request.extensions(), ApiScope::ControlManage).is_ok();
  let resources = match (group.as_str(), version.as_str()) {
    ("core", "v1") => vec![
      api_resource(
        "devices",
        "Device",
        &[
          core_proto::ApiResourceOperation::List,
          core_proto::ApiResourceOperation::Get,
        ],
      ),
      api_resource("services", "ApiService", &[core_proto::ApiResourceOperation::List]),
    ],
    ("runtime", "v1") => {
      let mut runner_operations = vec![
        core_proto::ApiResourceOperation::List,
        core_proto::ApiResourceOperation::Get,
      ];
      if can_manage && !state.handler.control_plane().list_runner_classes(None)?.runner_classes.is_empty() {
        runner_operations.extend([
          core_proto::ApiResourceOperation::Create,
          core_proto::ApiResourceOperation::Delete,
        ]);
      }
      let mut run_operations = vec![
        core_proto::ApiResourceOperation::List,
        core_proto::ApiResourceOperation::Get,
      ];
      if can_manage {
        run_operations.push(core_proto::ApiResourceOperation::Create);
      }
      vec![
        api_resource("runners", "Runner", &runner_operations),
        api_resource(
          "runnerclasses",
          "RunnerClass",
          &[
            core_proto::ApiResourceOperation::List,
            core_proto::ApiResourceOperation::Get,
          ],
        ),
        api_resource(
          "runnerleases",
          "RunnerLease",
          if can_manage {
            &[
              core_proto::ApiResourceOperation::Create,
              core_proto::ApiResourceOperation::Delete,
            ]
          } else {
            &[]
          },
        ),
        api_resource("runs", "Run", &run_operations),
      ]
    }
    _ => return Err(RestError::new(StatusCode::NOT_FOUND, "not_found", "unknown AUV API group or version")),
  };
  Ok(Protobuf(core_proto::GetApiGroupVersionResponse {
    namespace: "auv".to_string(),
    group,
    version,
    resources,
  }))
}

fn api_resource(name: &str, kind: &str, operations: &[core_proto::ApiResourceOperation]) -> core_proto::ApiResource {
  core_proto::ApiResource {
    name: name.to_string(),
    kind: kind.to_string(),
    operations: operations.iter().map(|operation| *operation as i32).collect(),
  }
}

async fn list_devices(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ListDevicesResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  Ok(Protobuf(state.handler.control_plane().list_devices()))
}

async fn get_device(
  State(state): State<RestState>,
  Path(device_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetDeviceResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  let device = state
    .handler
    .control_plane()
    .get_device(&device_id)
    .ok_or_else(|| RestError::new(StatusCode::NOT_FOUND, "not_found", format!("unknown Device: {device_id}")))?;
  Ok(Protobuf(core_proto::GetDeviceResponse {
    device: Some(device),
  }))
}

async fn create_run(State(state): State<RestState>, request: Request<Body>) -> Result<Protobuf<core_proto::CreateRunResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlManage)?;
  let request = decode_control_body(request).await?;
  state.handler.control_plane().create_run(&principal, request).map(Protobuf).map_err(RestError::from)
}

async fn list_runs(State(state): State<RestState>, request: Request<Body>) -> Result<Protobuf<core_proto::ListRunsResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  Ok(Protobuf(state.handler.control_plane().list_runs(&principal)))
}

async fn get_run(
  State(state): State<RestState>,
  Path(run_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetRunResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  state.handler.control_plane().get_run(&principal, &run_id).map(Protobuf).map_err(RestError::from)
}

async fn stop_run(
  State(state): State<RestState>,
  Path(run_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::StopRunResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlManage)?;
  let request = decode_body::<core_proto::StopRunRequest>(request).await?;
  if request.run.as_ref().is_some_and(|run| run.run_id != run_id) {
    return Err(RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", "path Run and request Run differ"));
  }
  let outcome = core_proto::RunOutcome::try_from(request.outcome).unwrap_or_default();
  state.handler.control_plane().stop_run(&principal, &run_id, outcome).await.map(Protobuf).map_err(RestError::from)
}

async fn claim_runner(
  State(state): State<RestState>,
  Path(run_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ClaimRunnerResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlManage)?;
  let mut request = decode_body::<core_proto::ClaimRunnerRequest>(request).await?;
  let claim = request.claim.as_mut().ok_or_else(|| RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", "claim is required"))?;
  if claim.run.as_ref().is_some_and(|run| run.run_id != run_id) {
    return Err(RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", "path Run and claim Run differ"));
  }
  claim.run = Some(core_proto::RunRef { run_id });
  state.handler.control_plane().claim_runner(&principal, request).await.map(Protobuf).map_err(RestError::from)
}

async fn release_runner_lease_for_run(
  State(state): State<RestState>,
  Path((run_id, lease_id)): Path<(String, String)>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ReleaseRunnerLeaseResponse>, RestError> {
  let principal = authorize(&state, &request, ApiScope::ControlManage)?;
  state
    .handler
    .control_plane()
    .release_runner_lease(
      &principal,
      &core_proto::RunnerLeaseRef {
        run: Some(core_proto::RunRef { run_id }),
        runner: None,
        lease_id,
      },
    )
    .await
    .map(Protobuf)
    .map_err(RestError::from)
}

async fn create_runner(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::CreateRunnerResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlManage)?;
  let request = decode_control_body(request).await?;
  state.handler.control_plane().create_runner(request).await.map(Protobuf).map_err(RestError::from)
}

async fn list_runners(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ListRunnersResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  Ok(Protobuf(state.handler.control_plane().list_runners()))
}

async fn list_runner_classes(
  State(state): State<RestState>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::ListRunnerClassesResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  state.handler.control_plane().list_runner_classes(None).map(Protobuf).map_err(RestError::from)
}

async fn get_runner_class(
  State(state): State<RestState>,
  Path(runner_class): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetRunnerClassResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  state.handler.control_plane().get_runner_class(None, &runner_class).map(Protobuf).map_err(RestError::from)
}

async fn get_runner(
  State(state): State<RestState>,
  Path(runner_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::GetRunnerResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlInspect)?;
  state.handler.control_plane().get_runner(&runner_id).map(Protobuf).map_err(RestError::from)
}

async fn delete_runner(
  State(state): State<RestState>,
  Path(runner_id): Path<String>,
  request: Request<Body>,
) -> Result<Protobuf<core_proto::DeleteRunnerResponse>, RestError> {
  let _principal = authorize(&state, &request, ApiScope::ControlManage)?;
  state.handler.control_plane().delete_runner(&runner_id).await.map(Protobuf).map_err(RestError::from)
}

fn authorize(state: &RestState, request: &Request<Body>, scope: ApiScope) -> Result<PrincipalId, RestError> {
  state.authority.principal_from_extensions(request.extensions(), scope).map_err(RestError::from)
}

async fn decode_body<M: Message + Default>(request: Request<Body>) -> Result<M, RestError> {
  decode_body_with_limit(request, MAX_PROTOBUF_BODY_BYTES).await
}

async fn decode_control_body<M: Message + Default>(request: Request<Body>) -> Result<M, RestError> {
  decode_body_with_limit(request, MAX_CONTROL_PROTOBUF_BODY_BYTES).await
}

async fn decode_body_with_limit<M: Message + Default>(request: Request<Body>, limit: usize) -> Result<M, RestError> {
  let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
  if content_type.split(';').next() != Some(PROTOBUF_CONTENT_TYPE) {
    return Err(RestError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "request body must use application/protobuf"));
  }
  let bytes = to_bytes(request.into_body(), limit)
    .await
    .map_err(|error| RestError::new(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large", error.to_string()))?;
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

impl From<ControlPlaneError> for RestError {
  fn from(error: ControlPlaneError) -> Self {
    match error {
      ControlPlaneError::InvalidArgument(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_argument", error.to_string()),
      ControlPlaneError::UnknownDevice(_) => Self::new(StatusCode::BAD_REQUEST, "invalid_argument", error.to_string()),
      ControlPlaneError::UnknownRun(_) => Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string()),
      ControlPlaneError::UnknownRunner(_) | ControlPlaneError::UnknownRunnerLease(_) => {
        Self::new(StatusCode::NOT_FOUND, "not_found", error.to_string())
      }
      ControlPlaneError::RunnerProviderUnavailable(_) | ControlPlaneError::RunnerCapabilityUnavailable(_) => {
        Self::new(StatusCode::NOT_IMPLEMENTED, "unimplemented", error.to_string())
      }
      ControlPlaneError::RunCapacityExhausted(_) => Self::new(StatusCode::TOO_MANY_REQUESTS, "resource_exhausted", error.to_string()),
      ControlPlaneError::RunnerOperation(_) => Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", error.to_string()),
      ControlPlaneError::RunnerRpcStatus(status) => status.into(),
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
mod tests {
  use std::sync::Arc;

  use axum::body::Body;
  use axum::http::{Request, StatusCode};
  use tower::ServiceExt;

  use crate::handler::ApiHandler;
  use crate::transport::RequestAuthority;

  use super::router;

  #[tokio::test]
  async fn generic_invoke_rest_path_is_not_registered() {
    let store = tempfile::tempdir().expect("temporary API store");
    let handler = Arc::new(ApiHandler::new(store.path().to_path_buf()).expect("API handler"));
    let authority = RequestAuthority::local(
      #[cfg(unix)]
      None,
    );
    let response = router(handler, authority)
      .oneshot(Request::post("/v1/operations:invoke").body(Body::empty()).expect("request"))
      .await
      .expect("REST response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
  }
}
