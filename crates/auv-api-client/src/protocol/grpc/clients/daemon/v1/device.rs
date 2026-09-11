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
}
