//! Device service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::device_service_server::DeviceService;
use tonic::{Request, Response, Status};

use crate::daemon::Daemon;

#[derive(Clone)]
pub(crate) struct DeviceServiceGrpc {
  daemon: Arc<Daemon>,
}

impl DeviceServiceGrpc {
  pub(crate) fn new(daemon: Arc<Daemon>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl DeviceService for DeviceServiceGrpc {
  async fn list_devices(&self, _request: Request<proto::ListDevicesRequest>) -> Result<Response<proto::ListDevicesResponse>, Status> {
    Ok(Response::new(self.daemon.list_devices()))
  }

  async fn get_device(&self, request: Request<proto::GetDeviceRequest>) -> Result<Response<proto::GetDeviceResponse>, Status> {
    let device_id = request
      .into_inner()
      .device
      .map(|device| device.device_id)
      .filter(|device_id| !device_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("device is required"))?;
    let device = self.daemon.get_device(&device_id).ok_or_else(|| Status::not_found(format!("unknown Device: {device_id}")))?;
    Ok(Response::new(proto::GetDeviceResponse {
      device: Some(device),
    }))
  }
}
