//! runner class gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::runner_class_service_client::RunnerClassServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the runner class gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: RunnerClassServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: RunnerClassServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn list_runner_classes(
    &mut self,
    device: Option<daemon_proto::DeviceRef>,
  ) -> Result<Vec<daemon_proto::RunnerClass>, tonic::Status> {
    Ok(self.inner.list_runner_classes(daemon_proto::ListRunnerClassesRequest { device }).await?.into_inner().runner_classes)
  }

  pub async fn get_runner_class(
    &mut self,
    runner_class: impl Into<String>,
    device: Option<daemon_proto::DeviceRef>,
  ) -> Result<daemon_proto::RunnerClass, tonic::Status> {
    self
      .inner
      .get_runner_class(daemon_proto::GetRunnerClassRequest {
        device,
        runner_class: Some(daemon_proto::RunnerClassRef {
          runner_class: runner_class.into(),
        }),
      })
      .await?
      .into_inner()
      .runner_class
      .ok_or_else(|| tonic::Status::internal("GetRunnerClass response omitted RunnerClass"))
  }
}
