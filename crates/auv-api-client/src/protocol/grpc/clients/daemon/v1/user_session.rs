//! user session gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::user_session_service_client::UserSessionServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the user session gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: UserSessionServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: UserSessionServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  /// Lists current OS login sessions on the selected Device.
  pub async fn list_user_sessions(&mut self) -> Result<daemon_proto::ListUserSessionsResponse, tonic::Status> {
    Ok(self.inner.list_user_sessions(daemon_proto::ListUserSessionsRequest {}).await?.into_inner())
  }

  /// Gets one current OS session by its opaque selector.
  pub async fn get_user_session(
    &mut self,
    session_selector: impl Into<String>,
  ) -> Result<daemon_proto::GetUserSessionResponse, tonic::Status> {
    Ok(
      self
        .inner
        .get_user_session(daemon_proto::GetUserSessionRequest {
          session_selector: session_selector.into(),
        })
        .await?
        .into_inner(),
    )
  }

  /// Requests entry for exactly one user or current OS login session.
  pub async fn ensure_user_session_unlocked(
    &mut self,
    request: daemon_proto::EnsureUserSessionUnlockedRequest,
  ) -> Result<daemon_proto::EnsureUserSessionUnlockedResponse, tonic::Status> {
    Ok(self.inner.ensure_user_session_unlocked(request).await?.into_inner())
  }

  /// Requests a verified lock of one existing OS login session.
  pub async fn ensure_user_session_locked(
    &mut self,
    request: daemon_proto::EnsureUserSessionLockedRequest,
  ) -> Result<daemon_proto::EnsureUserSessionLockedResponse, tonic::Status> {
    Ok(self.inner.ensure_user_session_locked(request).await?.into_inner())
  }
}
