//! pairing gRPC service implementation.

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::daemon::v1::pairing_service_client::PairingServiceClient;
use tonic::transport::Endpoint;

use crate::protocol::grpc::client::ApiTransport;

/// Client for the pairing gRPC service.
#[derive(Clone, Debug)]
pub struct Client {
  inner: PairingServiceClient<ApiTransport>,
}

impl Client {
  pub(in crate::protocol::grpc) fn new(inner: PairingServiceClient<ApiTransport>) -> Self {
    Self { inner }
  }

  pub async fn create_pairing_token(
    &mut self,
    request: daemon_proto::CreatePairingTokenRequest,
  ) -> Result<daemon_proto::CreatePairingTokenResponse, tonic::Status> {
    Ok(self.inner.create_pairing_token(request).await?.into_inner())
  }

  pub async fn revoke_device_credential(&mut self, device_id: impl Into<String>) -> Result<bool, tonic::Status> {
    Ok(
      self
        .inner
        .revoke_device_credential(daemon_proto::RevokeDeviceCredentialRequest {
          device_id: device_id.into(),
        })
        .await?
        .into_inner()
        .revoked,
    )
  }

  pub async fn set_enabled(&mut self, device_selector: impl Into<String>, enabled: bool) -> Result<bool, tonic::Status> {
    Ok(
      self
        .inner
        .set_paired_device_enabled(daemon_proto::SetPairedDeviceEnabledRequest {
          device_selector: device_selector.into(),
          enabled,
        })
        .await?
        .into_inner()
        .changed,
    )
  }

  pub async fn unpair(&mut self, device_selector: impl Into<String>) -> Result<bool, tonic::Status> {
    Ok(
      self
        .inner
        .unpair_device(daemon_proto::UnpairDeviceRequest {
          device_selector: device_selector.into(),
        })
        .await?
        .into_inner()
        .removed,
    )
  }

  pub async fn pair_device(
    endpoint: http::Uri,
    request: daemon_proto::PairDeviceRequest,
  ) -> Result<daemon_proto::PairDeviceResponse, tonic::Status> {
    let endpoint = Endpoint::from_shared(endpoint.to_string()).map_err(|error| tonic::Status::invalid_argument(error.to_string()))?;
    Ok(PairingServiceClient::new(endpoint.connect_lazy()).pair_device(request).await?.into_inner())
  }
}
