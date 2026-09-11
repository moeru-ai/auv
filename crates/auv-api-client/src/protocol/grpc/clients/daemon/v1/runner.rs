//! runner gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::runner_service_client::RunnerServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the runner gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: RunnerServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: RunnerServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn create_runner(&mut self, request: daemon_proto::CreateRunnerRequest) -> Result<daemon_proto::Runner, tonic::Status> {
    self
      .inner
      .create_runner(request)
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("CreateRunner response omitted Runner"))
  }

  pub async fn list_runners(&mut self) -> Result<Vec<daemon_proto::Runner>, tonic::Status> {
    Ok(self.inner.list_runners(daemon_proto::ListRunnersRequest {}).await?.into_inner().runners)
  }

  pub async fn get_runner(&mut self, runner_id: impl Into<String>) -> Result<daemon_proto::Runner, tonic::Status> {
    self
      .inner
      .get_runner(daemon_proto::GetRunnerRequest {
        runner: Some(daemon_proto::RunnerRef {
          runner_id: runner_id.into(),
        }),
      })
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("GetRunner response omitted Runner"))
  }

  pub async fn delete_runner(&mut self, runner_id: impl Into<String>) -> Result<daemon_proto::Runner, tonic::Status> {
    self.delete_runner_with_options(runner_id, None, false).await
  }

  pub async fn delete_runner_with_options(
    &mut self,
    runner_id: impl Into<String>,
    grace_period: Option<prost_types::Duration>,
    force: bool,
  ) -> Result<daemon_proto::Runner, tonic::Status> {
    self
      .inner
      .delete_runner(daemon_proto::DeleteRunnerRequest {
        runner: Some(daemon_proto::RunnerRef {
          runner_id: runner_id.into(),
        }),
        grace_period,
        force,
      })
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("DeleteRunner response omitted Runner"))
  }
}
