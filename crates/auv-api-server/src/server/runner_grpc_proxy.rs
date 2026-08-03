//! Raw gRPC proxying for daemon-owned custom Runner services.
//!
//! The daemon owns authentication, route admission, and routing. It does not
//! decode application-owned protobuf messages: after validating the concrete
//! service/method and AUV routing metadata, it streams standard gRPC bodies
//! and trailers between the public connection and private IPC.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body as AxumBody;
use axum::http::{Method, Request, Response, header};
use tonic::body::Body as TonicBody;
use tonic::{Code, Status};
use tower::ServiceExt as _;

use super::RequestAuth;
use crate::daemon::{Daemon, RunnerRoute};
use crate::protocol::grpc::status::map_control_error;

pub(crate) const ROUTE_DEVICE_METADATA: &str = "auv-device-id";
pub(crate) const ROUTE_RUN_METADATA: &str = "auv-run-id";
pub(crate) const ROUTE_RUNNER_CLASS_METADATA: &str = "auv-runner-class";

#[derive(Clone)]
pub(crate) struct RunnerGrpcProxy {
  daemon: Arc<Daemon>,
  auth: RequestAuth,
}

impl RunnerGrpcProxy {
  pub(crate) fn new(daemon: Arc<Daemon>, auth: RequestAuth) -> Self {
    Self { daemon, auth }
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
    reject_daemon_namespace(service)?;
    let caller = self.auth.authenticate_http(&request)?;
    let route = runner_route(request.headers())?;
    let (channel, permit) = self.daemon.admit_routed_channel(&caller, route, service, method).await.map_err(map_control_error)?;

    // Route fields are daemon input rather than application metadata.
    request.headers_mut().remove(ROUTE_DEVICE_METADATA);
    request.headers_mut().remove(ROUTE_RUN_METADATA);
    request.headers_mut().remove(ROUTE_RUNNER_CLASS_METADATA);

    let forwarded = std::mem::replace(request, Request::new(AxumBody::empty())).map(TonicBody::new);
    let response = channel.oneshot(forwarded).await.map_err(|error| Status::unavailable(format!("Runner transport failed: {error}")))?;
    Ok(response.map(|body| TonicBody::new(PermitBody::new(body, permit))))
  }
}

struct PermitBody {
  inner: TonicBody,
  permit: Option<crate::daemon::OperationPermit>,
}

impl PermitBody {
  fn new(inner: TonicBody, permit: crate::daemon::OperationPermit) -> Self {
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
    return Err(Status::unimplemented("proxied Runner gRPC methods require POST"));
  }
  let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
  if !content_type.starts_with("application/grpc") {
    return Err(Status::new(Code::Unimplemented, "unknown HTTP resource"));
  }
  if request.uri().query().is_some() {
    return Err(Status::unimplemented("proxied Runner gRPC method paths do not accept a query"));
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

fn reject_daemon_namespace(service: &str) -> Result<(), Status> {
  if service.starts_with("auv.api.daemon.") {
    return Err(Status::unimplemented("unknown daemon gRPC method"));
  }
  Ok(())
}

fn runner_route(headers: &http::HeaderMap) -> Result<RunnerRoute, Status> {
  let value = |name: &'static str, required: bool| -> Result<Option<String>, Status> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
      return Err(Status::invalid_argument(format!("{name} metadata must appear at most once")));
    }
    let value = first
      .map(|value| value.to_str().map(str::to_string).map_err(|_| Status::invalid_argument(format!("{name} metadata is not valid ASCII"))))
      .transpose()?;
    if required && value.as_deref().is_none_or(str::is_empty) {
      return Err(Status::invalid_argument(format!("{name} metadata is required")));
    }
    Ok(value.filter(|value| !value.is_empty()))
  };
  Ok(RunnerRoute {
    device_id: value(ROUTE_DEVICE_METADATA, false)?,
    run_id: value(ROUTE_RUN_METADATA, false)?,
    runner_class: value(ROUTE_RUNNER_CLASS_METADATA, true)?.expect("required route metadata was checked"),
  })
}

#[cfg(test)]
#[path = "runner_grpc_proxy_test.rs"]
mod tests;
