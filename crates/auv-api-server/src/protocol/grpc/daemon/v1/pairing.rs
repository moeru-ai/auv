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
    // A paired bearer must not mint further enrollments: one leaked credential
    // could otherwise create replacement Devices that survive its revocation.
    if !crate::authentication::caller(&request)?.is_local_owner() {
      return Err(Status::permission_denied("pairing tokens can only be created by the daemon owner over local IPC"));
    }
    Ok(Response::new(crate::protocol::pairing::create_token(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn pair_device(&self, request: Request<proto::PairDeviceRequest>) -> Result<Response<proto::PairDeviceResponse>, Status> {
    Ok(Response::new(crate::protocol::pairing::pair_device(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn revoke_device_credential(
    &self,
    request: Request<proto::RevokeDeviceCredentialRequest>,
  ) -> Result<Response<proto::RevokeDeviceCredentialResponse>, Status> {
    authorize_administration(&request, &request.get_ref().device_id)?;
    Ok(Response::new(crate::protocol::pairing::revoke_device_credential(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn set_paired_device_enabled(
    &self,
    request: Request<proto::SetPairedDeviceEnabledRequest>,
  ) -> Result<Response<proto::SetPairedDeviceEnabledResponse>, Status> {
    authorize_administration(&request, &request.get_ref().device_selector)?;
    Ok(Response::new(crate::protocol::pairing::set_enabled(self.pairing()?.as_ref(), request.into_inner())?))
  }

  async fn unpair_device(&self, request: Request<proto::UnpairDeviceRequest>) -> Result<Response<proto::UnpairDeviceResponse>, Status> {
    authorize_administration(&request, &request.get_ref().device_selector)?;
    Ok(Response::new(crate::protocol::pairing::unpair(self.pairing()?.as_ref(), request.into_inner())?))
  }
}

/// The local owner administers every paired Device; a paired Device may only
/// revoke, disable, or unpair itself. Trust lists stay owned by the daemon
/// host, as in RustDesk, Sunshine, and Parsec, so a leaked bearer cannot lock
/// out or remove other Devices.
///
/// A paired caller must name its own canonical ID. The pairing store resolves
/// an exact ID before labels or prefixes, so that selector cannot reach
/// another Device.
// TODO(pairing-admin-device): an explicitly granted, default-off administrator
// Device (like RustDesk remote configuration or Tailscale signing nodes) is
// deferred until a remote-administration workflow is owner-approved.
fn authorize_administration<T>(request: &Request<T>, selector: &str) -> Result<(), Status> {
  let caller = crate::authentication::caller(request)?;
  if caller.is_local_owner() || caller.paired_device_id() == Some(selector.trim()) {
    return Ok(());
  }
  Err(Status::permission_denied("paired Devices can only administer themselves by Device ID; manage other Devices on the daemon host"))
}
