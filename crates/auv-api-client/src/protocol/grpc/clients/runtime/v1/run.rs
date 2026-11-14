//! run gRPC service implementation.

use auv_api_proto::auv::api::runtime::v1 as runtime_proto;
use auv_api_proto::auv::api::runtime::v1::run_service_client::RunServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the run gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: RunServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: RunServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn create_run(&mut self, request: runtime_proto::CreateRunRequest) -> Result<runtime_proto::Run, tonic::Status> {
    self.inner.create_run(request).await?.into_inner().run.ok_or_else(|| tonic::Status::internal("CreateRun response omitted Run"))
  }

  pub async fn list_runs(&mut self) -> Result<Vec<runtime_proto::Run>, tonic::Status> {
    Ok(self.inner.list_runs(runtime_proto::ListRunsRequest {}).await?.into_inner().runs)
  }

  pub async fn get_run(&mut self, run_id: impl Into<String>) -> Result<runtime_proto::Run, tonic::Status> {
    self
      .inner
      .get_run(runtime_proto::GetRunRequest {
        run: Some(runtime_proto::RunRef {
          run_id: run_id.into(),
        }),
      })
      .await?
      .into_inner()
      .run
      .ok_or_else(|| tonic::Status::internal("GetRun response omitted Run"))
  }

  pub async fn stop_run(
    &mut self,
    run_id: impl Into<String>,
    outcome: runtime_proto::RunOutcome,
  ) -> Result<runtime_proto::Run, tonic::Status> {
    self
      .inner
      .stop_run(runtime_proto::StopRunRequest {
        run: Some(runtime_proto::RunRef {
          run_id: run_id.into(),
        }),
        outcome: outcome as i32,
      })
      .await?
      .into_inner()
      .run
      .ok_or_else(|| tonic::Status::internal("StopRun response omitted Run"))
  }
}
