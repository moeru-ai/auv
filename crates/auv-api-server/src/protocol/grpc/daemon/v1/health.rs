//! Daemon health service adapter.

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::health_service_server::HealthService;
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub(crate) struct HealthServiceGrpc {
  pub id: String,
}

#[tonic::async_trait]
impl HealthService for HealthServiceGrpc {
  async fn check(&self, _request: Request<proto::CheckRequest>) -> Result<Response<proto::CheckResponse>, Status> {
    Ok(Response::new(proto::CheckResponse {
      id: self.id.clone(),
      status: proto::HealthStatus::Serving.into(),
    }))
  }
}
