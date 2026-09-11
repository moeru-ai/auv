//! Paired-Device enrollment, credentials, and authentication.

mod persistence;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq as _;

use self::persistence::{FileStore, PairingTokenRecord, StoreFile};
use auv_api_server::control::CallerId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
  Active,
  Revoked,
}

/// Stable paired-Device identity with revocable opaque bearer digests.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PairingRecord {
  pub pair_id: String,
  pub label: String,
  pub enabled: bool,
  #[serde(default)]
  pub device_credentials: Vec<DeviceCredential>,
}

/// Persisted digest of one opaque long-lived bearer credential.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DeviceCredential {
  pub credential_sha256: String,
  pub state: CredentialState,
}

/// A short-lived enrollment token. The plaintext is returned only by
/// [`PairingStore::issue_token`] and is never persisted or included in store
/// listings.
pub struct PairingToken(String);

impl PairingToken {
  pub fn expose_once(self) -> String {
    self.0
  }
}

/// Result of consuming a bootstrap token. The bearer plaintext is returned
/// once while only its digest is persisted.
pub struct PairedDeviceEnrollment {
  pub device: PairingRecord,
  credential: String,
}

impl PairedDeviceEnrollment {
  pub fn expose_credential_once(self) -> String {
    self.credential
  }
}

impl std::fmt::Debug for PairedDeviceEnrollment {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("PairedDeviceEnrollment").field("device", &self.device).field("credential", &"[REDACTED]").finish()
  }
}

impl std::fmt::Debug for PairingToken {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str("PairingToken([REDACTED])")
  }
}

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
  #[error("pair_id must not be empty")]
  EmptyPairId,
  #[error("pairing token lifetime must be greater than zero")]
  InvalidTokenLifetime,
  #[error("pairing token is invalid, expired, or has already been consumed")]
  InvalidPairingToken,
  #[error("system clock cannot represent a pairing token deadline")]
  InvalidTokenClock,
  #[error("failed to generate a secure pairing token: {0}")]
  TokenGeneration(String),
  #[error("paired device was not found: {0}")]
  UnknownPair(String),
  #[error("paired Device label {label:?} is ambiguous; candidate IDs: {pair_ids}")]
  AmbiguousPairLabel { label: String, pair_ids: String },
  #[error("Device credential was not found")]
  UnknownCredential,
  #[error("Device credential is not paired or has been revoked")]
  Unauthenticated,
  #[error("duplicate paired-device ID: {0}")]
  DuplicatePairId(String),
  #[error("Device credential digest is already assigned to paired Device {0}")]
  DuplicateCredential(String),
  #[error("failed to read pairing store {path}: {source}")]
  Read {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("invalid pairing store {path}: {source}")]
  Decode {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("unsupported pairing store version {version} in {path}")]
  UnsupportedVersion { version: u32, path: PathBuf },
  #[error("failed to update pairing store {path}: {message}")]
  Update { path: PathBuf, message: String },
  #[error("pairing store revision {revision} committed, but directory durability could not be confirmed: {message}")]
  CommittedButDurabilityUnknown { revision: u64, message: String },
}

/// Process-owned pairing store with in-memory authentication reads.
#[derive(Clone)]
pub struct PairingStore {
  inner: Arc<FileStore>,
}

impl PairingStore {
  pub fn open(path: PathBuf) -> Result<Self, PairingError> {
    Ok(Self {
      inner: Arc::new(FileStore::open(path)?),
    })
  }

  pub fn list(&self) -> Vec<PairingRecord> {
    self.inner.devices()
  }

  /// Resolves an exact stable ID, an unambiguous human-facing label, or an
  /// unambiguous stable-ID prefix, in that order.
  pub fn resolve_pair_id(&self, selector: &str) -> Result<String, PairingError> {
    let selector = selector.trim();
    if selector.is_empty() {
      return Err(PairingError::EmptyPairId);
    }
    let records = self.list();
    if records.iter().any(|record| record.pair_id == selector) {
      return Ok(selector.to_string());
    }
    let label_matches = records.iter().filter(|record| record.label == selector).map(|record| record.pair_id.clone()).collect::<Vec<_>>();
    match label_matches.as_slice() {
      [pair_id] => return Ok(pair_id.clone()),
      [_, _, ..] => {
        return Err(PairingError::AmbiguousPairLabel {
          label: selector.to_string(),
          pair_ids: label_matches.join(", "),
        });
      }
      [] => {}
    }
    let id_matches =
      records.iter().filter(|record| record.pair_id.starts_with(selector)).map(|record| record.pair_id.clone()).collect::<Vec<_>>();
    match id_matches.as_slice() {
      [] => Err(PairingError::UnknownPair(selector.to_string())),
      [pair_id] => Ok(pair_id.clone()),
      _ => Err(PairingError::AmbiguousPairLabel {
        label: selector.to_string(),
        pair_ids: id_matches.join(", "),
      }),
    }
  }

  /// Creates a cryptographically random, short-lived, one-time enrollment
  /// token. Only its SHA-256 digest is persisted.
  pub fn issue_token(&self, lifetime: Option<Duration>) -> Result<PairingToken, PairingError> {
    self.issue_token_at(lifetime, SystemTime::now())
  }

  fn issue_token_at(&self, lifetime: Option<Duration>, now: SystemTime) -> Result<PairingToken, PairingError> {
    if lifetime.is_some_and(|lifetime| lifetime.is_zero()) {
      return Err(PairingError::InvalidTokenLifetime);
    }
    let expires_at = lifetime
      .map(|lifetime| {
        now
          .duration_since(UNIX_EPOCH)
          .ok()
          .and_then(|now| now.checked_add(lifetime))
          .map(|deadline| deadline.as_secs())
          .ok_or(PairingError::InvalidTokenClock)
      })
      .transpose()?;
    let mut secret = [0_u8; 16];
    getrandom::fill(&mut secret).map_err(|error| PairingError::TokenGeneration(error.to_string()))?;
    let plaintext = hex::encode(secret);
    let digest = token_digest(&plaintext);
    self.update(|store| {
      store.tokens.push(PairingTokenRecord { digest, expires_at });
      Ok(())
    })?;
    Ok(PairingToken(plaintext))
  }

  /// Atomically consumes one enrollment token and creates the stable paired
  /// Device identity. Reusing the same token always fails.
  pub fn consume_token(&self, token: &str, pair_id: String, label: String) -> Result<PairedDeviceEnrollment, PairingError> {
    self.consume_token_at(token, pair_id, label, SystemTime::now())
  }

  fn consume_token_at(&self, token: &str, pair_id: String, label: String, now: SystemTime) -> Result<PairedDeviceEnrollment, PairingError> {
    let now = now.duration_since(UNIX_EPOCH).map_err(|_| PairingError::InvalidTokenClock)?.as_secs();
    let digest = token_digest(token);
    let mut paired = None;
    let mut credential_bytes = [0_u8; 32];
    getrandom::fill(&mut credential_bytes).map_err(|error| PairingError::TokenGeneration(error.to_string()))?;
    let credential = hex::encode(credential_bytes);
    let credential_sha256 = token_digest(&credential);
    self.update(|store| {
      let index =
        store.tokens.iter().position(|candidate| digest_matches(&candidate.digest, &digest)).ok_or(PairingError::InvalidPairingToken)?;
      if store.tokens[index].expires_at.is_some_and(|deadline| deadline <= now) {
        return Err(PairingError::InvalidPairingToken);
      }
      if store.devices.iter().any(|device| device.pair_id == pair_id) {
        return Err(PairingError::DuplicatePairId(pair_id.clone()));
      }
      store.tokens.remove(index);
      let record = PairingRecord {
        pair_id: pair_id.clone(),
        label: label.clone(),
        enabled: true,
        device_credentials: vec![DeviceCredential {
          credential_sha256,
          state: CredentialState::Active,
        }],
      };
      store.devices.push(record.clone());
      paired = Some(record);
      Ok(())
    })?;
    Ok(PairedDeviceEnrollment {
      device: paired.expect("successful token consumption creates a paired Device"),
      credential,
    })
  }

  /// Resolves a long-lived bearer against the current immutable snapshot so
  /// disable/revoke mutations affect the next RPC without a CRL or cache TTL.
  pub fn authenticate_bearer(&self, credential: &str) -> Result<CallerId, PairingError> {
    let digest = token_digest(credential);
    self.inner.with_snapshot(|snapshot| {
      let record = snapshot
        .devices
        .iter()
        .find(|record| record.device_credentials.iter().any(|candidate| digest_matches(&candidate.credential_sha256, &digest)))
        .ok_or(PairingError::Unauthenticated)?;
      let credential = record
        .device_credentials
        .iter()
        .find(|candidate| digest_matches(&candidate.credential_sha256, &digest))
        .expect("matched credential exists");
      if !record.enabled || credential.state != CredentialState::Active {
        return Err(PairingError::Unauthenticated);
      }
      Ok(CallerId::paired_device(&record.pair_id))
    })
  }

  /// Revokes every long-lived bearer owned by one stable paired Device.
  pub fn revoke_device_credentials(&self, pair_id: &str) -> Result<bool, PairingError> {
    self.update(|store| {
      let record =
        store.devices.iter_mut().find(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.into()))?;
      let mut changed = false;
      for credential in &mut record.device_credentials {
        changed |= credential.state != CredentialState::Revoked;
        credential.state = CredentialState::Revoked;
      }
      if !changed {
        return Err(PairingError::UnknownCredential);
      }
      Ok(())
    })?;
    Ok(true)
  }

  pub fn set_enabled(&self, pair_id: &str, enabled: bool) -> Result<bool, PairingError> {
    self.update(|store| {
      let record =
        store.devices.iter_mut().find(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      let changed = record.enabled != enabled;
      record.enabled = enabled;
      Ok(changed)
    })
  }

  /// Removes one paired Device and all credentials owned by its stable ID.
  pub fn remove_pair(&self, pair_id: &str) -> Result<(), PairingError> {
    self.update(|store| {
      let index =
        store.devices.iter().position(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      store.devices.remove(index);
      Ok(())
    })
  }

  fn update<T>(&self, mutate: impl FnOnce(&mut StoreFile) -> Result<T, PairingError>) -> Result<T, PairingError> {
    self.inner.update(mutate)
  }
}

fn token_digest(token: &str) -> String {
  hex::encode(Sha256::digest(token.as_bytes()))
}

fn digest_matches(stored_hex: &str, candidate_hex: &str) -> bool {
  stored_hex.len() == candidate_hex.len() && bool::from(stored_hex.as_bytes().ct_eq(candidate_hex.as_bytes()))
}

impl auv_api_server::control::Pairing for PairingStore {
  fn authenticate_bearer(&self, credential: &str) -> Result<CallerId, auv_api_server::control::PairingError> {
    PairingStore::authenticate_bearer(self, credential).map_err(control_error)
  }

  fn issue_token(&self, lifetime: Option<Duration>) -> Result<auv_api_server::control::PairingToken, auv_api_server::control::PairingError> {
    Ok(auv_api_server::control::PairingToken {
      token: PairingStore::issue_token(self, lifetime).map_err(control_error)?.expose_once(),
    })
  }

  fn enroll(
    &self,
    token: &str,
    device_id: String,
    label: String,
  ) -> Result<auv_api_server::control::Enrollment, auv_api_server::control::PairingError> {
    let enrollment = PairingStore::consume_token(self, token, device_id, label).map_err(control_error)?;
    let device_id = enrollment.device.pair_id.clone();
    Ok(auv_api_server::control::Enrollment {
      device_id,
      credential: enrollment.expose_credential_once(),
    })
  }

  fn revoke_device_credentials(&self, selector: &str) -> Result<bool, auv_api_server::control::PairingError> {
    let pair_id = self.resolve_pair_id(selector).map_err(control_error)?;
    PairingStore::revoke_device_credentials(self, &pair_id).map_err(control_error)
  }

  fn set_enabled(&self, selector: &str, enabled: bool) -> Result<bool, auv_api_server::control::PairingError> {
    let pair_id = self.resolve_pair_id(selector).map_err(control_error)?;
    PairingStore::set_enabled(self, &pair_id, enabled).map_err(control_error)
  }

  fn unpair(&self, selector: &str) -> Result<bool, auv_api_server::control::PairingError> {
    let pair_id = self.resolve_pair_id(selector).map_err(control_error)?;
    PairingStore::remove_pair(self, &pair_id).map_err(control_error)?;
    Ok(true)
  }
}

fn control_error(error: PairingError) -> auv_api_server::control::PairingError {
  use auv_api_server::control::PairingError as Control;
  match error {
    PairingError::InvalidPairingToken => Control::InvalidToken,
    PairingError::Unauthenticated => Control::Unauthenticated,
    PairingError::UnknownPair(value) => Control::NotFound(value),
    PairingError::UnknownCredential => Control::NotFound("Device credential".to_string()),
    PairingError::AmbiguousPairLabel { label, pair_ids } => Control::Ambiguous(format!("{label:?}; candidate IDs: {pair_ids}")),
    PairingError::EmptyPairId
    | PairingError::InvalidTokenLifetime
    | PairingError::DuplicatePairId(_)
    | PairingError::DuplicateCredential(_) => Control::Invalid(error.to_string()),
    other => Control::Persistence(other.to_string()),
  }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
