//! Generated daemon JSON routes and dynamic Protobuf runner invocation.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use http_body_util::BodyExt as _;
use tonic::{Code, Status};

use crate::authentication::Authenticator;
use crate::control::Control;
use crate::middleware::authentication;
use crate::protocol::grpc::daemon::v1::{
  DeviceServiceGrpc, DiscoveryServiceGrpc, HealthServiceGrpc, PairingServiceGrpc, RunServiceGrpc, RunnerClassServiceGrpc, RunnerServiceGrpc,
};

const PROTOBUF_CONTENT_TYPE: &str = "application/protobuf";
const INVOKE_PATH: &str = "/apis/auv/runtime/v1/invoke";

// NOTICE(tonic-rest-codegen): the generator also emits a combined router. This
// adapter uses service routers so dynamic invoke routes can share their state.
#[allow(dead_code)]
mod generated {
  include!(concat!(env!("OUT_DIR"), "/daemon_rest.rs"));
}

#[derive(Clone)]
struct RestState {
  daemon: Arc<dyn Control>,
  authenticator: Authenticator,
}

pub(crate) fn router(daemon: Arc<dyn Control>, authenticator: Authenticator) -> Router {
  let pairing = Arc::new(PairingServiceGrpc::new(authenticator.pairing()));
  let discovery = Arc::new(DiscoveryServiceGrpc::new(Arc::clone(&daemon)));
  let health = Arc::new(HealthServiceGrpc);
  let devices = Arc::new(DeviceServiceGrpc::new(Arc::clone(&daemon)));
  let runs = Arc::new(RunServiceGrpc::new(Arc::clone(&daemon)));
  let runners = Arc::new(RunnerServiceGrpc::new(Arc::clone(&daemon)));
  let runner_classes = Arc::new(RunnerClassServiceGrpc::new(Arc::clone(&daemon)));

  // TODO(websocket-events): durable event subscriptions still need ordering,
  // cursor, and gap-recovery semantics. The invoke socket below is scoped to
  // one live Runner operation and does not define that separate event model.
  Router::new()
    .route("/apis/auv/runtime/v1/invoke/{service}/{method}", post(invoke_unary))
    .route(INVOKE_PATH, get(invoke_websocket))
    .with_state(RestState {
      daemon,
      authenticator,
    })
    .merge(generated::pairing_service_rest_router(pairing))
    .merge(generated::discovery_service_rest_router(discovery))
    .merge(generated::health_service_rest_router(health))
    .merge(generated::device_service_rest_router(devices))
    .merge(generated::run_service_rest_router(runs))
    .merge(generated::runner_service_rest_router(runners))
    .merge(generated::runner_class_service_rest_router(runner_classes))
}

async fn invoke_websocket(State(state): State<RestState>, upgrade: WebSocketUpgrade) -> Response {
  upgrade.on_upgrade(move |socket| crate::protocol::websocket::serve(socket, state.daemon, state.authenticator)).into_response()
}

async fn invoke_unary(
  State(state): State<RestState>,
  Path((service, method)): Path<(String, String)>,
  request: Request<Body>,
) -> Result<ProtobufBytes, RestError> {
  let (mut parts, body) = request.into_parts();

  let protobuf =
    to_bytes(body, usize::MAX).await.map_err(|error| RestError::new(StatusCode::BAD_REQUEST, "invalid_body", error.to_string()))?;
  let mut grpc_body = Vec::with_capacity(5 + protobuf.len());
  grpc_body.push(0);
  let length = u32::try_from(protobuf.len())
    .map_err(|_| RestError::new(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large", "Protobuf request exceeds the gRPC frame length"))?;

  grpc_body.extend_from_slice(&length.to_be_bytes());
  grpc_body.extend_from_slice(&protobuf);

  parts.method = axum::http::Method::POST;
  parts.uri = format!("/{service}/{method}")
    .parse()
    .map_err(|error| RestError::new(StatusCode::BAD_REQUEST, "invalid_argument", format!("invalid gRPC method path: {error}")))?;
  parts.headers.remove(header::CONTENT_LENGTH);
  parts.headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/grpc"));
  parts.headers.insert(header::TE, HeaderValue::from_static("trailers"));

  let request = Request::from_parts(parts, Body::from(grpc_body));
  let proxy = crate::server::runner_grpc_proxy::RunnerGrpcProxy::new(Arc::clone(&state.daemon));
  let response = proxy.forward(request).await;
  let (parts, body) = response.into_parts();
  let collected = body.collect().await.map_err(RestError::from)?;

  let grpc_status = parts
    .headers
    .get("grpc-status")
    .or_else(|| collected.trailers().and_then(|trailers| trailers.get("grpc-status")))
    .ok_or_else(|| RestError::new(StatusCode::BAD_GATEWAY, "invalid_grpc", "Runner response omitted grpc-status"))?
    .to_str()
    .map_err(|error| RestError::new(StatusCode::BAD_GATEWAY, "invalid_grpc", error.to_string()))?
    .parse::<i32>()
    .map_err(|error| RestError::new(StatusCode::BAD_GATEWAY, "invalid_grpc", error.to_string()))?;

  if grpc_status != 0 {
    let message = parts
      .headers
      .get("grpc-message")
      .or_else(|| collected.trailers().and_then(|trailers| trailers.get("grpc-message")))
      .and_then(|value| value.to_str().ok())
      .unwrap_or("Runner operation failed");
    return Err(RestError::from(Status::new(Code::from_i32(grpc_status), message)));
  }

  let bytes = collected.to_bytes();
  if bytes.len() < 5 || bytes[0] != 0 {
    return Err(RestError::new(StatusCode::BAD_GATEWAY, "invalid_grpc", "Runner returned an invalid unary gRPC frame"));
  }

  let length = u32::from_be_bytes(bytes[1..5].try_into().expect("five-byte gRPC prefix was checked")) as usize;
  if bytes.len() != length + 5 {
    return Err(RestError::new(StatusCode::BAD_GATEWAY, "invalid_grpc", "Runner returned more or less than one unary response"));
  }

  Ok(ProtobufBytes(bytes.slice(5..)))
}

struct ProtobufBytes(bytes::Bytes);

impl IntoResponse for ProtobufBytes {
  fn into_response(self) -> Response {
    let mut response = self.0.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(PROTOBUF_CONTENT_TYPE));
    response
  }
}

pub(crate) struct RestError {
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

/// Preserves the connection identity and authentication metadata when a
/// generated Axum handler calls the existing Tonic service implementation.
pub(crate) fn build_tonic_request<T, E>(body: T, headers: &axum::http::HeaderMap, extension: Option<E>) -> tonic::Request<T>
where
  E: Clone + Send + Sync + 'static,
{
  let mut request = tonic::Request::new(body);
  if let Some(extension) = extension {
    request.extensions_mut().insert(extension);
  }
  for name in [
    "authorization",
    "user-agent",
    "x-forwarded-for",
    "x-real-ip",
  ] {
    let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) else {
      continue;
    };
    let Ok(key) = name.parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>() else {
      continue;
    };
    if let Ok(value) = value.parse() {
      request.metadata_mut().insert(key, value);
    }
  }
  request
}

/// Registers the authentication requirements owned by the REST routes.
pub(crate) fn register_authentication(builder: &mut authentication::Builder) {
  builder.websocket_open(Method::GET, INVOKE_PATH);
  // NOTICE(tonic-rest-public-methods): tonic-rest 0.1.5 emits public paths
  // without their HTTP methods. The configured public RPCs currently use GET
  // or POST, so register both; unmatched method/path pairs have no route. Ask
  // the generator for exact method/path pairs before adding another HTTP verb.
  for &path in generated::PUBLIC_REST_PATHS {
    builder.public(Method::GET, path);
    builder.public(Method::POST, path);
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
