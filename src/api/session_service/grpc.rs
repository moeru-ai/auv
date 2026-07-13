//! Tonic adapter for the session API.
//!
//! This module owns protobuf RPC dispatch, handler-error mapping, and the
//! blocking/cancellation bridge. Server configuration, listen policy, and
//! lifecycle live in [`super::server`].
//!
//! `StreamSessionEvents` remains unimplemented because the application layer
//! has no event projector to provide a real stream. The adapter returns
//! `UNIMPLEMENTED` rather than fabricating an empty stream.
//!
//! NOTICE: Dropping an RPC cancels its join handle, but `spawn_blocking` work
//! already in flight cannot be forcibly interrupted. An invoke may therefore
//! finish recording after the caller receives `CANCELLED` until cooperative
//! cancellation reaches the invoke runtime.

use std::sync::Arc;

use auv_api_proto::v1::session as proto;
use auv_api_proto::v1::session::session_service_server::SessionService;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};

use super::SessionApiError;
use super::handler::SessionApiHandler;

fn map_session_error(error: SessionApiError) -> Status {
  match error {
    SessionApiError::MissingField(_) | SessionApiError::PayloadDecode(_) => Status::invalid_argument(error.to_string()),
    SessionApiError::UnknownSession(_) | SessionApiError::RunNotFound(_) => Status::not_found(error.to_string()),
    SessionApiError::PersistedOperationRequired(_) => Status::failed_precondition(error.to_string()),
    SessionApiError::OperationIdMismatch { .. } => Status::invalid_argument(error.to_string()),
    SessionApiError::Storage(_) | SessionApiError::InvokeExecution(_) => Status::internal(error.to_string()),
  }
}

/// Cancels the paired RPC token when the tonic handler future is dropped.
struct RpcCancelGuard(CancellationToken);

impl Drop for RpcCancelGuard {
  fn drop(&mut self) {
    self.0.cancel();
  }
}

async fn run_blocking_rpc<T>(
  cancel: CancellationToken,
  work: impl FnOnce() -> Result<T, SessionApiError> + Send + 'static,
) -> Result<T, Status>
where
  T: Send + 'static,
{
  let preflight = cancel.child_token();
  let mut join = tokio::task::spawn_blocking(move || {
    if preflight.is_cancelled() {
      return None;
    }
    Some(work())
  });

  tokio::select! {
    _ = cancel.cancelled() => {
      join.abort();
      Err(Status::cancelled("session API request cancelled"))
    }
    result = &mut join => {
      let Some(result) = result
        .map_err(|error| Status::internal(format!("handler task join failed: {error}")))?
      else {
        return Err(Status::cancelled("session API request cancelled"));
      };
      result.map_err(map_session_error)
    }
  }
}

/// Tonic `SessionService` adapter over the protobuf-aware application handler.
#[derive(Clone)]
pub(crate) struct SessionServiceGrpc {
  handler: Arc<SessionApiHandler>,
}

impl SessionServiceGrpc {
  pub(crate) fn new(handler: Arc<SessionApiHandler>) -> Self {
    Self { handler }
  }
}

#[tonic::async_trait]
impl SessionService for SessionServiceGrpc {
  async fn create_session(&self, request: Request<proto::CreateSessionRequest>) -> Result<Response<proto::CreateSessionResponse>, Status> {
    let cancel = CancellationToken::new();
    let _guard = RpcCancelGuard(cancel.clone());
    let handler = Arc::clone(&self.handler);
    let inner = request.into_inner();
    run_blocking_rpc(cancel, move || handler.create_session(inner)).await.map(Response::new)
  }

  async fn invoke(&self, request: Request<proto::InvokeRequest>) -> Result<Response<proto::InvokeResponse>, Status> {
    let cancel = CancellationToken::new();
    let _guard = RpcCancelGuard(cancel.clone());
    let handler = Arc::clone(&self.handler);
    let inner = request.into_inner();
    run_blocking_rpc(cancel, move || handler.invoke(inner)).await.map(Response::new)
  }

  async fn get_operation(&self, request: Request<proto::GetOperationRequest>) -> Result<Response<proto::GetOperationResponse>, Status> {
    let cancel = CancellationToken::new();
    let _guard = RpcCancelGuard(cancel.clone());
    let handler = Arc::clone(&self.handler);
    let inner = request.into_inner();
    run_blocking_rpc(cancel, move || handler.get_operation(inner)).await.map(Response::new)
  }

  type StreamSessionEventsStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<proto::SessionEvent, Status>> + Send>>;

  async fn stream_session_events(
    &self,
    _request: Request<proto::StreamSessionEventsRequest>,
  ) -> Result<Response<Self::StreamSessionEventsStream>, Status> {
    Err(Status::unimplemented("session API seam not wired: stream_session_events"))
  }
}

#[cfg(test)]
mod tests {
  use auv_api_proto::v1::session::session_service_server::SessionService;
  use tonic::Code;

  use super::*;
  use crate::api::session_service::test_fixtures::session_api_temp_store_root;

  #[test]
  fn map_session_error_maps_representative_variants() {
    assert_eq!(map_session_error(SessionApiError::MissingField("session")).code(), Code::InvalidArgument);
    assert_eq!(map_session_error(SessionApiError::UnknownSession("ghost".to_string())).code(), Code::NotFound);
    assert_eq!(map_session_error(SessionApiError::PersistedOperationRequired("run-1".to_string())).code(), Code::FailedPrecondition);
    assert_eq!(map_session_error(SessionApiError::Storage("disk".to_string())).code(), Code::Internal);
  }

  #[tokio::test]
  async fn stream_session_events_remains_unimplemented() {
    let handler = Arc::new(SessionApiHandler::new(session_api_temp_store_root("grpc-stream-events")));
    let service = SessionServiceGrpc::new(handler);
    let status = match service.stream_session_events(Request::new(proto::StreamSessionEventsRequest { session: None })).await {
      Ok(_) => panic!("streaming should remain unimplemented"),
      Err(status) => status,
    };
    assert_eq!(status.code(), Code::Unimplemented);
  }

  #[tokio::test]
  async fn run_blocking_rpc_returns_cancelled_when_token_fires() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let status = run_blocking_rpc(cancel, || Ok(42_u32)).await.expect_err("cancelled before work starts");
    assert_eq!(status.code(), Code::Cancelled);
  }
}
