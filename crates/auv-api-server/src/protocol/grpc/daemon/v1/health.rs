//! Daemon health service adapter.

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::health_service_server::HealthService;
use tonic::{Request, Response, Status};

#[derive(Clone, Default)]
pub(crate) struct HealthServiceGrpc;

#[tonic::async_trait]
impl HealthService for HealthServiceGrpc {
  async fn check(&self, _request: Request<proto::CheckRequest>) -> Result<Response<proto::CheckResponse>, Status> {
    Ok(Response::new(proto::CheckResponse {
      status: proto::HealthStatus::Serving.into(),
    }))
  }
}
