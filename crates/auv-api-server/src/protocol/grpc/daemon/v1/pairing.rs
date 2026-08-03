//! Pairing service adapter.

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::pairing_service_server::PairingService;
use tonic::{Request, Response, Status};

use crate::server::RequestAuth;

#[derive(Clone)]
pub(crate) struct PairingServiceGrpc {
  auth: RequestAuth,
}

impl PairingServiceGrpc {
  pub(crate) fn new(auth: RequestAuth) -> Self {
    Self { auth }
  }

  fn store(&self) -> Result<crate::auth::PairingStore, Status> {
    self.auth.pairing_store().ok_or_else(|| Status::failed_precondition("pairing store is not configured"))
  }
}

#[tonic::async_trait]
impl PairingService for PairingServiceGrpc {
  async fn create_pairing_token(
    &self,
    request: Request<proto::CreatePairingTokenRequest>,
  ) -> Result<Response<proto::CreatePairingTokenResponse>, Status> {
    self.auth.authenticate(&request)?;
    let request = request.into_inner();
    let lifetime = request.ttl.as_ref().map(proto_duration).transpose()?;
    let token = self.store()?.issue_token(lifetime).map_err(pairing_status)?;
    let expires_at = lifetime.map(|lifetime| {
      let deadline = std::time::SystemTime::now() + lifetime;
      let duration = deadline.duration_since(std::time::UNIX_EPOCH).expect("current time is after Unix epoch");
      prost_types::Timestamp {
        seconds: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(duration.subsec_nanos()).expect("nanoseconds fit i32"),
      }
    });
    Ok(Response::new(proto::CreatePairingTokenResponse {
      token: token.expose_once(),
      expires_at,
    }))
  }

  async fn pair_device(&self, request: Request<proto::PairDeviceRequest>) -> Result<Response<proto::PairDeviceResponse>, Status> {
    let request = request.into_inner();
    if request.token.is_empty() || request.device_id.is_empty() {
      return Err(Status::invalid_argument("token and device_id are required"));
    }
    let enrollment = self.store()?.consume_token(&request.token, request.device_id.clone(), request.label).map_err(pairing_status)?;
    Ok(Response::new(proto::PairDeviceResponse {
      device_id: request.device_id,
      device_credential: enrollment.expose_credential_once(),
    }))
  }

  async fn revoke_device_credential(
    &self,
    request: Request<proto::RevokeDeviceCredentialRequest>,
  ) -> Result<Response<proto::RevokeDeviceCredentialResponse>, Status> {
    self.auth.authenticate(&request)?;
    let device_id = request.into_inner().device_id;
    if device_id.is_empty() {
      return Err(Status::invalid_argument("device_id is required"));
    }
    let revoked = self.store()?.revoke_device_credentials(&device_id).map_err(pairing_status)?;
    Ok(Response::new(proto::RevokeDeviceCredentialResponse { revoked }))
  }
}

fn proto_duration(value: &prost_types::Duration) -> Result<std::time::Duration, Status> {
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(Status::invalid_argument("ttl must be a non-negative protobuf Duration"));
  }
  Ok(std::time::Duration::new(
    u64::try_from(value.seconds).expect("non-negative seconds fit u64"),
    u32::try_from(value.nanos).expect("validated nanos fit u32"),
  ))
}

fn pairing_status(error: crate::auth::PairingError) -> Status {
  match error {
    crate::auth::PairingError::InvalidPairingToken => Status::unauthenticated(error.to_string()),
    crate::auth::PairingError::InvalidTokenLifetime
    | crate::auth::PairingError::EmptyPairId
    | crate::auth::PairingError::DuplicatePairId(_) => Status::invalid_argument(error.to_string()),
    crate::auth::PairingError::UnknownPair(_) | crate::auth::PairingError::UnknownCredential => Status::not_found(error.to_string()),
    _ => Status::internal(error.to_string()),
  }
}
