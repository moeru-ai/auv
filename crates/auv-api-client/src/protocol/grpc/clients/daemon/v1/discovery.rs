//! discovery gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::discovery_service_client::DiscoveryServiceClient;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the discovery gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: DiscoveryServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: DiscoveryServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn list_api_namespaces(&mut self) -> Result<Vec<daemon_proto::ApiNamespace>, tonic::Status> {
    Ok(self.inner.list_api_namespaces(daemon_proto::ListApiNamespacesRequest {}).await?.into_inner().namespaces)
  }
}
