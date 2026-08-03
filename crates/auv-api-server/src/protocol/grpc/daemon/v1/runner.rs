//! Runner service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::runner_service_server::RunnerService;
use tonic::{Request, Response, Status};

use crate::daemon::Daemon;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct RunnerServiceGrpc {
  daemon: Arc<Daemon>,
}

impl RunnerServiceGrpc {
  pub(crate) fn new(daemon: Arc<Daemon>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl RunnerService for RunnerServiceGrpc {
  async fn create_runner(&self, request: Request<proto::CreateRunnerRequest>) -> Result<Response<proto::CreateRunnerResponse>, Status> {
    self.daemon.create_runner(request.into_inner()).await.map(Response::new).map_err(map_control_error)
  }

  async fn list_runners(&self, _request: Request<proto::ListRunnersRequest>) -> Result<Response<proto::ListRunnersResponse>, Status> {
    Ok(Response::new(self.daemon.list_runners()))
  }

  async fn get_runner(&self, request: Request<proto::GetRunnerRequest>) -> Result<Response<proto::GetRunnerResponse>, Status> {
    let runner_id = request
      .into_inner()
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    self.daemon.get_runner(&runner_id).map(Response::new).map_err(map_control_error)
  }

  async fn delete_runner(&self, request: Request<proto::DeleteRunnerRequest>) -> Result<Response<proto::DeleteRunnerResponse>, Status> {
    let request = request.into_inner();
    let runner_id = request
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    self.daemon.delete_runner(&runner_id, request.grace_period, request.force).await.map(Response::new).map_err(map_control_error)
  }
}
