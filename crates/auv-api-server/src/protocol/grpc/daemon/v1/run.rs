//! Run service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::run_service_server::RunService;
use tonic::{Request, Response, Status};

use super::caller;
use crate::daemon::Daemon;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct RunServiceGrpc {
  daemon: Arc<Daemon>,
}

impl RunServiceGrpc {
  pub(crate) fn new(daemon: Arc<Daemon>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl RunService for RunServiceGrpc {
  async fn create_run(&self, request: Request<proto::CreateRunRequest>) -> Result<Response<proto::CreateRunResponse>, Status> {
    let caller = caller(&request)?;
    self.daemon.create_run(&caller, request.into_inner()).map(Response::new).map_err(map_control_error)
  }

  async fn list_runs(&self, request: Request<proto::ListRunsRequest>) -> Result<Response<proto::ListRunsResponse>, Status> {
    let caller = caller(&request)?;
    Ok(Response::new(self.daemon.list_runs(&caller)))
  }

  async fn get_run(&self, request: Request<proto::GetRunRequest>) -> Result<Response<proto::GetRunResponse>, Status> {
    let caller = caller(&request)?;
    let request = request.into_inner();
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    self.daemon.get_run(&caller, &run_id).map(Response::new).map_err(map_control_error)
  }

  async fn stop_run(&self, request: Request<proto::StopRunRequest>) -> Result<Response<proto::StopRunResponse>, Status> {
    let caller = caller(&request)?;
    let request = request.into_inner();
    let outcome = proto::RunOutcome::try_from(request.outcome).map_err(|_| Status::invalid_argument("Run outcome is unknown"))?;
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    self.daemon.stop_run(&caller, &run_id, outcome).await.map(Response::new).map_err(map_control_error)
  }
}
