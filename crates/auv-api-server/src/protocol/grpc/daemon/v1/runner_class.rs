//! Runner class service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::runner_class_service_server::RunnerClassService;
use tonic::{Request, Response, Status};

use crate::control::Control;
use crate::protocol::domain;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct RunnerClassServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl RunnerClassServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl RunnerClassService for RunnerClassServiceGrpc {
  async fn list_runner_classes(
    &self,
    request: Request<proto::ListRunnerClassesRequest>,
  ) -> Result<Response<proto::ListRunnerClassesResponse>, Status> {
    let device_id = request.get_ref().device.as_ref().map(|device| device.device_id.as_str());
    let runner_classes =
      self.daemon.list_runner_classes(device_id).map_err(map_control_error)?.into_iter().map(domain::runner_class).collect();
    Ok(Response::new(proto::ListRunnerClassesResponse { runner_classes }))
  }

  async fn get_runner_class(
    &self,
    request: Request<proto::GetRunnerClassRequest>,
  ) -> Result<Response<proto::GetRunnerClassResponse>, Status> {
    let request = request.into_inner();
    let device_id = request.device.as_ref().map(|device| device.device_id.as_str());
    let runner_class = request
      .runner_class
      .as_ref()
      .map(|runner_class| runner_class.runner_class.as_str())
      .filter(|runner_class| !runner_class.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner_class is required"))?;
    let runner_class = self.daemon.get_runner_class(device_id, runner_class).map_err(map_control_error)?;
    Ok(Response::new(proto::GetRunnerClassResponse {
      runner_class: Some(domain::runner_class(runner_class)),
    }))
  }
}
