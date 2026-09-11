//! Transport-independent projections for live pairing operations.

use auv_api_proto::auv::api::daemon::v1 as proto;
use tonic::Status;

use crate::control::{Pairing, PairingError};

pub(crate) fn create_token(
  pairing: &dyn Pairing,
  request: proto::CreatePairingTokenRequest,
) -> Result<proto::CreatePairingTokenResponse, Status> {
  let lifetime = request.ttl.as_ref().map(proto_duration).transpose()?;
  let token = pairing.issue_token(lifetime).map_err(pairing_status)?;
  let expires_at = lifetime.map(|lifetime| {
    let deadline = std::time::SystemTime::now() + lifetime;
    let duration = deadline.duration_since(std::time::UNIX_EPOCH).expect("current time is after Unix epoch");

    prost_types::Timestamp {
      seconds: i64::try_from(duration.as_secs()).expect("pairing deadline fits protobuf Timestamp seconds"),
      nanos: i32::try_from(duration.subsec_nanos()).expect("nanoseconds fit i32"),
    }
  });
  Ok(proto::CreatePairingTokenResponse {
    token: token.token,
    expires_at,
  })
}

pub(crate) fn pair_device(pairing: &dyn Pairing, request: proto::PairDeviceRequest) -> Result<proto::PairDeviceResponse, Status> {
  if request.token.is_empty() || request.device_id.is_empty() {
    return Err(Status::invalid_argument("token and device_id are required"));
  }
  let enrollment = pairing.enroll(&request.token, request.device_id.clone(), request.label).map_err(pairing_status)?;
  Ok(proto::PairDeviceResponse {
    device_id: enrollment.device_id,
    device_credential: enrollment.credential,
  })
}

pub(crate) fn revoke_device_credential(
  pairing: &dyn Pairing,
  request: proto::RevokeDeviceCredentialRequest,
) -> Result<proto::RevokeDeviceCredentialResponse, Status> {
  if request.device_id.is_empty() {
    return Err(Status::invalid_argument("device_id is required"));
  }

  let revoked = pairing.revoke_device_credentials(&request.device_id).map_err(pairing_status)?;
  Ok(proto::RevokeDeviceCredentialResponse { revoked })
}

pub(crate) fn set_enabled(
  pairing: &dyn Pairing,
  request: proto::SetPairedDeviceEnabledRequest,
) -> Result<proto::SetPairedDeviceEnabledResponse, Status> {
  if request.device_selector.is_empty() {
    return Err(Status::invalid_argument("device_selector is required"));
  }

  let changed = pairing.set_enabled(&request.device_selector, request.enabled).map_err(pairing_status)?;
  Ok(proto::SetPairedDeviceEnabledResponse { changed })
}

pub(crate) fn unpair(pairing: &dyn Pairing, request: proto::UnpairDeviceRequest) -> Result<proto::UnpairDeviceResponse, Status> {
  if request.device_selector.is_empty() {
    return Err(Status::invalid_argument("device_selector is required"));
  }

  let removed = pairing.unpair(&request.device_selector).map_err(pairing_status)?;
  Ok(proto::UnpairDeviceResponse { removed })
}

fn proto_duration(value: &prost_types::Duration) -> Result<std::time::Duration, Status> {
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(Status::invalid_argument("ttl must be a non-negative protobuf Duration"));
  }

  Ok(std::time::Duration::new(value.seconds as u64, value.nanos as u32))
}

fn pairing_status(error: PairingError) -> Status {
  match error {
    PairingError::InvalidToken => Status::unauthenticated(error.to_string()),
    PairingError::Invalid(_) => Status::invalid_argument(error.to_string()),
    PairingError::NotFound(_) => Status::not_found(error.to_string()),
    PairingError::Ambiguous(_) => Status::failed_precondition(error.to_string()),
    PairingError::Unauthenticated => Status::unauthenticated(error.to_string()),
    PairingError::NotConfigured => Status::unimplemented(error.to_string()),
    PairingError::Persistence(_) => Status::internal(error.to_string()),
  }
}
