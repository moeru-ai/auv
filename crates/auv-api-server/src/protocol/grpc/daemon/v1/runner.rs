//! Runner service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::runner_service_server::RunnerService;
use tonic::{Request, Response, Status};

use crate::control::Control;
use crate::protocol::domain;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct RunnerServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl RunnerServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl RunnerService for RunnerServiceGrpc {
  async fn create_runner(&self, request: Request<proto::CreateRunnerRequest>) -> Result<Response<proto::CreateRunnerResponse>, Status> {
    let request = domain::create_runner(request.into_inner()).map_err(map_control_error)?;
    let runner = self.daemon.create_runner(request).await.map_err(map_control_error)?;
    Ok(Response::new(proto::CreateRunnerResponse {
      runner: Some(domain::runner(runner)),
    }))
  }

  async fn list_runners(&self, _request: Request<proto::ListRunnersRequest>) -> Result<Response<proto::ListRunnersResponse>, Status> {
    let runners = self.daemon.list_runners().map_err(map_control_error)?.into_iter().map(domain::runner).collect();
    Ok(Response::new(proto::ListRunnersResponse { runners }))
  }

  async fn get_runner(&self, request: Request<proto::GetRunnerRequest>) -> Result<Response<proto::GetRunnerResponse>, Status> {
    let runner_id = request
      .into_inner()
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    let runner = self.daemon.get_runner(&runner_id).map_err(map_control_error)?;
    Ok(Response::new(proto::GetRunnerResponse {
      runner: Some(domain::runner(runner)),
    }))
  }

  async fn delete_runner(&self, request: Request<proto::DeleteRunnerRequest>) -> Result<Response<proto::DeleteRunnerResponse>, Status> {
    let request = request.into_inner();
    let runner_id = request
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    let grace_period = request.grace_period.map(domain::duration_from_proto).transpose().map_err(map_control_error)?;
    let runner = self
      .daemon
      .delete_runner(
        &runner_id,
        auv::runners::StopRunner {
          grace_period,
          force: request.force,
        },
      )
      .await
      .map_err(map_control_error)?;
    Ok(Response::new(proto::DeleteRunnerResponse {
      runner: Some(domain::runner(runner)),
    }))
  }
}
