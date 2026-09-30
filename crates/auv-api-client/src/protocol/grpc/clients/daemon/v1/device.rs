//! device gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::device_service_client::DeviceServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the device gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: DeviceServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: DeviceServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn list_devices(&mut self) -> Result<Vec<daemon_proto::Device>, tonic::Status> {
    Ok(self.inner.list_devices(daemon_proto::ListDevicesRequest {}).await?.into_inner().devices)
  }

  pub async fn get_device(&mut self, device_id: impl Into<String>) -> Result<daemon_proto::Device, tonic::Status> {
    self
      .inner
      .get_device(daemon_proto::GetDeviceRequest {
        device: Some(daemon_proto::DeviceRef {
          device_id: device_id.into(),
        }),
      })
      .await?
      .into_inner()
      .device
      .ok_or_else(|| tonic::Status::internal("GetDevice response omitted Device"))
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
