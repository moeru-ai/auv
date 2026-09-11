//! Pairing service adapter.

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::pairing_service_server::PairingService;
use tonic::{Request, Response, Status};

use crate::control::Pairing;

#[derive(Clone)]
pub(crate) struct PairingServiceGrpc {
  pairing: Option<std::sync::Arc<dyn Pairing>>,
}

impl PairingServiceGrpc {
  pub(crate) fn new(pairing: Option<std::sync::Arc<dyn Pairing>>) -> Self {
    Self { pairing }
  }

  fn pairing(&self) -> Result<std::sync::Arc<dyn crate::control::Pairing>, Status> {
    self.pairing.clone().ok_or_else(|| Status::unimplemented("pairing is not configured"))
  }
}

#[tonic::async_trait]
impl PairingService for PairingServiceGrpc {
  async fn create_pairing_token(
    &self,
    request: Request<proto::CreatePairingTokenRequest>,
  ) -> Result<Response<proto::CreatePairingTokenResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::create_token(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn pair_device(&self, request: Request<proto::PairDeviceRequest>) -> Result<Response<proto::PairDeviceResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::pair_device(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn revoke_device_credential(
    &self,
    request: Request<proto::RevokeDeviceCredentialRequest>,
  ) -> Result<Response<proto::RevokeDeviceCredentialResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::revoke_device_credential(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn set_paired_device_enabled(
    &self,
    request: Request<proto::SetPairedDeviceEnabledRequest>,
  ) -> Result<Response<proto::SetPairedDeviceEnabledResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::set_enabled(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn unpair_device(&self, request: Request<proto::UnpairDeviceRequest>) -> Result<Response<proto::UnpairDeviceResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::unpair(self.pairing()?.as_ref(), request.into_inner())?))
  }
}
