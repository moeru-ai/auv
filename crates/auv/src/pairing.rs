//! Live pairing enrollment and administration operations.

use std::time::Duration;

use auv_api_proto::auv::api::daemon::v1 as proto;

use crate::client::Client;
use crate::error::{ClientError, ClientErrorKind};
use crate::profile::{DeviceProfileInput, ProfileStore};

/// Input for enrolling this client with a remote daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnrollmentRequest {
  /// Remote HTTP endpoint.
  pub endpoint: String,
  /// One-time pairing token.
  pub token: String,
  /// Optional identity for this client Device.
  pub client_device_id: Option<String>,
  /// Human-facing label stored by the remote daemon.
  pub label: String,
  /// Optional local profile name.
  pub profile: Option<String>,
}

/// Result of enrolling and persisting a paired profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrollment {
  /// Canonical identity of the remote Device.
  pub device_id: crate::resource::DeviceId,
  /// Remote Device name.
  pub device_name: String,
  /// Remote endpoint.
  pub endpoint: String,
  /// Local profile name.
  pub profile: String,
  /// Path of the profile store that received the credential.
  pub credentials_file: std::path::PathBuf,
}

/// Failure from live pairing enrollment or administration.
#[derive(Debug, thiserror::Error)]
pub enum PairingError {
  /// Pairing authentication or authorization failed.
  #[error("pairing request is unauthorized: {0}")]
  Unauthorized(#[source] ClientError),
  /// No paired Device matches the selector.
  #[error("paired Device was not found: {0}")]
  NotFound(#[source] ClientError),
  /// More than one paired Device matches the selector.
  #[error("paired Device selector is ambiguous: {0}")]
  Ambiguous(#[source] ClientError),
  /// The server rejected an invalid pairing request.
  #[error("pairing request is invalid: {0}")]
  InvalidRequest(#[source] ClientError),
  /// Current pairing state conflicts with the request.
  #[error("pairing state conflicts with the request: {0}")]
  Conflict(#[source] ClientError),
  /// The selected daemon has no pairing backend.
  #[error("pairing is not configured: {0}")]
  NotConfigured(#[source] ClientError),
  /// The pairing service could not be reached.
  #[error("pairing service is unavailable: {0}")]
  Unavailable(#[source] ClientError),
  /// The pairing transport or response failed.
  #[error("pairing protocol failed: {0}")]
  Protocol(#[source] ClientError),
  /// The paired Device selector is empty.
  #[error("paired Device selector must not be empty")]
  EmptySelector,
  /// The enrollment endpoint is not a valid URI.
  #[error("pairing endpoint is invalid: {0}")]
  InvalidEndpoint(String),
  /// The requested token lifetime cannot be represented by the protocol.
  #[error("pairing token lifetime exceeds the supported range")]
  InvalidLifetime,
  /// Generating a local client Device identity failed.
  #[error("failed to generate a client Device identity: {0}")]
  IdentityGeneration(String),
  /// The remote daemon did not expose exactly one caller-local Device.
  #[error("paired endpoint returned {0} caller-local Devices; expected exactly one")]
  InvalidRemoteDeviceCount(usize),
  /// Remote Device discovery or validation failed.
  #[error(transparent)]
  Device(#[from] crate::devices::DeviceError),
  /// Persisting the local paired profile failed.
  #[error(transparent)]
  Profile(#[from] crate::profile::ProfileError),
  /// Connecting with the newly issued credential failed.
  #[error("failed to connect to paired Device: {0}")]
  Connect(String),
}

/// Non-empty server-side selector for a paired Device record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairedDeviceSelector(String);

impl PairedDeviceSelector {
  /// Validates a paired Device ID, label, or unambiguous prefix.
  pub fn parse(value: &str) -> Result<Self, PairingError> {
    let value = value.trim();
    if value.is_empty() {
      return Err(PairingError::EmptySelector);
    }
    Ok(Self(value.to_string()))
  }

  /// Returns the selector text sent to the owning daemon.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl From<ClientError> for PairingError {
  fn from(error: ClientError) -> Self {
    match error.kind() {
      ClientErrorKind::Unauthorized => Self::Unauthorized(error),
      ClientErrorKind::NotFound => Self::NotFound(error),
      ClientErrorKind::Ambiguous => Self::Ambiguous(error),
      ClientErrorKind::InvalidRequest => Self::InvalidRequest(error),
      ClientErrorKind::Conflict => Self::Conflict(error),
      ClientErrorKind::Unavailable => Self::Unavailable(error),
      ClientErrorKind::Unsupported => Self::NotConfigured(error),
      ClientErrorKind::Protocol => Self::Protocol(error),
    }
  }
}

/// Live pairing operations bound to one daemon.
#[derive(Clone, Debug)]
pub struct Pairing {
  client: Client,
}

impl Pairing {
  pub(crate) fn new(client: Client) -> Self {
    Self { client }
  }

  /// Creates a one-time enrollment token.
  pub async fn create_token(&self, lifetime: Option<Duration>) -> Result<String, PairingError> {
    let ttl = lifetime
      .map(|duration| -> Result<prost_types::Duration, PairingError> {
        Ok(prost_types::Duration {
          seconds: i64::try_from(duration.as_secs()).map_err(|_| PairingError::InvalidLifetime)?,
          nanos: i32::try_from(duration.subsec_nanos()).expect("subsecond nanoseconds fit i32"),
        })
      })
      .transpose()?;
    Ok(
      self
        .client
        .grpc_client()
        .pairing()
        .create_pairing_token(proto::CreatePairingTokenRequest { ttl })
        .await
        .map_err(|status| ClientError::from_status("CreatePairingToken", status))?
        .token,
    )
  }

  /// Enables or disables one paired Device record.
  pub async fn set_enabled(&self, selector: &PairedDeviceSelector, enabled: bool) -> Result<bool, PairingError> {
    self
      .client
      .grpc_client()
      .pairing()
      .set_enabled(selector.as_str(), enabled)
      .await
      .map_err(|status| ClientError::from_status("SetPairedDeviceEnabled", status).into())
  }

  /// Removes one paired Device and its credentials.
  pub async fn unpair(&self, selector: &PairedDeviceSelector) -> Result<bool, PairingError> {
    self
      .client
      .grpc_client()
      .pairing()
      .unpair(selector.as_str())
      .await
      .map_err(|status| ClientError::from_status("UnpairDevice", status).into())
  }

  /// Revokes all bearer credentials issued to the selected paired Device
  /// while retaining its pairing record.
  pub async fn revoke_credentials(&self, selector: &PairedDeviceSelector) -> Result<bool, PairingError> {
    self
      .client
      .grpc_client()
      .pairing()
      .revoke_device_credential(selector.as_str())
      .await
      .map_err(|status| ClientError::from_status("RevokeDeviceCredential", status).into())
  }

  /// Enrolls with a remote daemon and persists the resulting credential in
  /// the supplied profile store.
  pub async fn enroll(request: EnrollmentRequest, profiles: &ProfileStore) -> Result<Enrollment, PairingError> {
    let endpoint = request.endpoint.parse::<http::Uri>().map_err(|error| PairingError::InvalidEndpoint(error.to_string()))?;
    let client_device_id = request.client_device_id.unwrap_or(random_identity()?);
    let response = auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
      endpoint.clone(),
      proto::PairDeviceRequest {
        token: request.token,
        device_id: client_device_id,
        label: request.label,
      },
    )
    .await
    .map_err(|status| ClientError::from_status("PairDevice", status))?;
    let grpc = auv_api_client::protocol::grpc::Client::connect_paired(auv_api_client::PairedConnectConfig {
      endpoint,
      device_credential: response.device_credential.clone(),
    })
    .await
    .map_err(|error| PairingError::Connect(error.to_string()))?;
    let client = Client::from_grpc(grpc);
    let mut remotes = client.devices().list().await?.into_iter().filter(|device| device.local).collect::<Vec<_>>();
    if remotes.len() != 1 {
      return Err(PairingError::InvalidRemoteDeviceCount(remotes.len()));
    }
    let remote = remotes.pop().expect("one caller-local Device");
    let configured = match profiles.list_devices() {
      Ok(configured) => configured,
      Err(crate::profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(error) => return Err(error.into()),
    };
    let profile = request
      .profile
      .or_else(|| {
        configured.into_iter().find(|candidate| candidate.device_id() == remote.id.as_str()).map(|value| value.config_profile().to_string())
      })
      .unwrap_or_else(|| {
        if remote.name.is_empty() {
          remote.id.short()
        } else {
          remote.name.clone()
        }
      });
    profiles.upsert(
      &profile,
      DeviceProfileInput {
        device_id: remote.id.to_string(),
        device_name: remote.name.clone(),
        endpoint: request.endpoint.clone(),
        device_credential: response.device_credential,
      },
    )?;
    Ok(Enrollment {
      device_id: remote.id,
      device_name: remote.name,
      endpoint: request.endpoint,
      profile,
      credentials_file: profiles.config_path().to_path_buf(),
    })
  }
}

fn random_identity() -> Result<String, PairingError> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| PairingError::IdentityGeneration(error.to_string()))?;
  Ok(hex::encode(bytes))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pairing_maps_authorization_and_ambiguity_to_resource_specific_errors() {
    let unauthorized = PairingError::from(ClientError::from_status("UnpairDevice", tonic::Status::permission_denied("denied")));
    assert!(matches!(unauthorized, PairingError::Unauthorized(_)));

    let ambiguous = PairingError::from(ClientError::from_status("SetPairedDeviceEnabled", tonic::Status::failed_precondition("ambiguous")));
    assert!(matches!(ambiguous, PairingError::Ambiguous(_)));

    let not_configured =
      PairingError::from(ClientError::from_status("CreatePairingToken", tonic::Status::unimplemented("pairing is not configured")));
    assert!(matches!(not_configured, PairingError::NotConfigured(_)));
  }

  #[test]
  fn paired_device_selector_rejects_blank_input() {
    assert!(matches!(PairedDeviceSelector::parse("  "), Err(PairingError::EmptySelector)));
  }
}
