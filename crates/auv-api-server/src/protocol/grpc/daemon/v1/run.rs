//! Run service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::run_service_server::RunService;
use tonic::{Request, Response, Status};

use crate::authentication;
use crate::control::Control;
use crate::protocol::domain;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct RunServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl RunServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl RunService for RunServiceGrpc {
  async fn create_run(&self, request: Request<proto::CreateRunRequest>) -> Result<Response<proto::CreateRunResponse>, Status> {
    let caller = authentication::caller(&request)?.clone();
    let request = domain::create_run(request.into_inner()).map_err(map_control_error)?;
    let run = self.daemon.create_run(&caller, request).map_err(map_control_error)?;
    Ok(Response::new(proto::CreateRunResponse {
      run: Some(domain::run(run)),
    }))
  }

  async fn list_runs(&self, request: Request<proto::ListRunsRequest>) -> Result<Response<proto::ListRunsResponse>, Status> {
    let caller = authentication::caller(&request)?;
    let runs = self.daemon.list_runs(caller).map_err(map_control_error)?.into_iter().map(domain::run).collect();
    Ok(Response::new(proto::ListRunsResponse { runs }))
  }

  async fn get_run(&self, request: Request<proto::GetRunRequest>) -> Result<Response<proto::GetRunResponse>, Status> {
    let caller = authentication::caller(&request)?.clone();
    let request = request.into_inner();
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    let run = self.daemon.get_run(&caller, &run_id).map_err(map_control_error)?;
    Ok(Response::new(proto::GetRunResponse {
      run: Some(domain::run(run)),
    }))
  }

  async fn stop_run(&self, request: Request<proto::StopRunRequest>) -> Result<Response<proto::StopRunResponse>, Status> {
    let caller = authentication::caller(&request)?.clone();
    let request = request.into_inner();
    let outcome = proto::RunOutcome::try_from(request.outcome).map_err(|_| Status::invalid_argument("Run outcome is unknown"))?;
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    let outcome = domain::run_outcome(outcome).map_err(map_control_error)?;
    let run = self.daemon.stop_run(&caller, &run_id, outcome).await.map_err(map_control_error)?;
    Ok(Response::new(proto::StopRunResponse {
      run: Some(domain::run(run)),
    }))
  }
}
