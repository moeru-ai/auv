//! Raw unary gRPC aggregation for daemon-owned custom Runner services.
//!
//! The daemon owns authentication, lease admission, and routing. It does not
//! decode application-owned protobuf messages: after validating the concrete
//! service/method and the AUV lease metadata, it streams standard gRPC bodies
//! and trailers between the public connection and private IPC.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use auv_api_proto::auv::api::core::v1::RunnerLeaseRef;
use axum::body::Body as AxumBody;
use axum::http::{Method, Request, Response, header};
use prost::Message;
use tonic::body::Body as TonicBody;
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};
use tower::ServiceExt as _;

use crate::authority::ApiScope;
use crate::control_grpc::map_control_error;
use crate::handler::ApiHandler;
use crate::transport::RequestAuthority;

pub(crate) const RUNNER_LEASE_METADATA: &str = "auv-runner-lease-bin";

#[derive(Clone)]
pub(crate) struct AggregatedGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl AggregatedGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }

  pub(crate) async fn forward(&self, mut request: Request<AxumBody>) -> Response<TonicBody> {
    match self.try_forward(&mut request).await {
      Ok(response) => response,
      Err(status) => status.into_http(),
    }
  }

  async fn try_forward(&self, request: &mut Request<AxumBody>) -> Result<Response<TonicBody>, Status> {
    require_grpc_request(request)?;
    let (service, method) = grpc_method(request.uri().path())?;
    let principal = self.authority.principal_from_extensions(request.extensions(), ApiScope::OperationsExecute)?;
    let lease = runner_lease(request.headers())?;
    let (channel, permit) =
      self.handler.control_plane().admit_runner_channel(&principal, &lease, service, method).map_err(map_control_error)?;

    // The lease is daemon routing input, not application metadata. External
    // credentials and cookies must never cross the private trusted boundary.
    request.headers_mut().remove(RUNNER_LEASE_METADATA);
    request.headers_mut().remove(header::AUTHORIZATION);
    request.headers_mut().remove(header::PROXY_AUTHORIZATION);
    request.headers_mut().remove(header::COOKIE);
    request.headers_mut().remove(header::SET_COOKIE);

    let forwarded = std::mem::replace(request, Request::new(AxumBody::empty())).map(TonicBody::new);
    let response = channel.oneshot(forwarded).await.map_err(|error| Status::unavailable(format!("Runner transport failed: {error}")))?;
    Ok(response.map(|body| TonicBody::new(PermitBody::new(body, permit))))
  }
}

struct PermitBody {
  inner: TonicBody,
  permit: Option<crate::control_plane::OperationPermit>,
}

impl PermitBody {
  fn new(inner: TonicBody, permit: crate::control_plane::OperationPermit) -> Self {
    Self {
      inner,
      permit: Some(permit),
    }
  }
}

impl http_body::Body for PermitBody {
  type Data = <TonicBody as http_body::Body>::Data;
  type Error = Status;

  fn poll_frame(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
    let poll = Pin::new(&mut self.inner).poll_frame(context);
    if matches!(poll, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
      self.permit.take();
    }
    poll
  }

  fn is_end_stream(&self) -> bool {
    self.inner.is_end_stream()
  }

  fn size_hint(&self) -> http_body::SizeHint {
    self.inner.size_hint()
  }
}

fn require_grpc_request(request: &Request<AxumBody>) -> Result<(), Status> {
  if request.method() != Method::POST {
    return Err(Status::unimplemented("aggregated gRPC methods require POST"));
  }
  let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
  if !content_type.starts_with("application/grpc") {
    return Err(Status::new(Code::Unimplemented, "unknown HTTP resource"));
  }
  if request.uri().query().is_some() {
    return Err(Status::unimplemented("aggregated gRPC method paths do not accept a query"));
  }
  Ok(())
}

fn grpc_method(path: &str) -> Result<(&str, &str), Status> {
  let mut segments = path.strip_prefix('/').unwrap_or(path).split('/');
  let service = segments.next().unwrap_or_default();
  let method = segments.next().unwrap_or_default();
  if service.is_empty() || method.is_empty() || segments.next().is_some() {
    return Err(Status::unimplemented("unknown gRPC method path"));
  }
  Ok((service, method))
}

fn runner_lease(headers: &http::HeaderMap) -> Result<RunnerLeaseRef, Status> {
  let metadata = MetadataMap::from_headers(headers.clone());
  let values = metadata.get_all_bin(RUNNER_LEASE_METADATA);
  let mut values = values.iter();
  let encoded = values
    .next()
    .ok_or_else(|| Status::invalid_argument(format!("{RUNNER_LEASE_METADATA} metadata is required")))?
    .to_bytes()
    .map_err(|_| Status::invalid_argument(format!("{RUNNER_LEASE_METADATA} metadata is not valid binary gRPC metadata")))?;
  if values.next().is_some() {
    return Err(Status::invalid_argument(format!("{RUNNER_LEASE_METADATA} metadata must appear exactly once")));
  }
  let lease = RunnerLeaseRef::decode(encoded).map_err(|_| Status::invalid_argument("Runner lease metadata is not a RunnerLeaseRef"))?;
  if lease.lease_id.is_empty() {
    return Err(Status::invalid_argument("Runner lease metadata omitted lease_id"));
  }
  Ok(lease)
}

#[cfg(test)]
mod tests {
  use super::*;
  use tonic::metadata::MetadataValue;

  #[test]
  fn parses_exact_grpc_method_path() {
    assert_eq!(grpc_method("/auv.example.v1.ExampleService/Get").unwrap(), ("auv.example.v1.ExampleService", "Get"));
    assert!(grpc_method("/auv.example.v1.ExampleService").is_err());
    assert!(grpc_method("/auv.example.v1.ExampleService/Get/extra").is_err());
  }

  #[test]
  fn binary_metadata_round_trips_runner_lease_ref() {
    let expected = RunnerLeaseRef {
      lease_id: "lease_test".to_string(),
      ..Default::default()
    };
    let mut metadata = MetadataMap::new();
    metadata.insert_bin(RUNNER_LEASE_METADATA, MetadataValue::from_bytes(&expected.encode_to_vec()));
    assert_eq!(runner_lease(&metadata.into_headers()).unwrap(), expected);
  }

  #[test]
  fn lease_metadata_rejects_missing_and_empty_refs() {
    assert_eq!(runner_lease(&http::HeaderMap::new()).unwrap_err().code(), Code::InvalidArgument);
    let mut metadata = MetadataMap::new();
    metadata.insert_bin(RUNNER_LEASE_METADATA, MetadataValue::from_bytes(&RunnerLeaseRef::default().encode_to_vec()));
    assert_eq!(runner_lease(&metadata.into_headers()).unwrap_err().code(), Code::InvalidArgument);

    let mut metadata = MetadataMap::new();
    let value = MetadataValue::from_bytes(
      &RunnerLeaseRef {
        lease_id: "lease_test".to_string(),
        ..Default::default()
      }
      .encode_to_vec(),
    );
    metadata.append_bin(RUNNER_LEASE_METADATA, value.clone());
    metadata.append_bin(RUNNER_LEASE_METADATA, value);
    assert_eq!(runner_lease(&metadata.into_headers()).unwrap_err().code(), Code::InvalidArgument);
  }
}
