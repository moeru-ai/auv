//! Request authentication and durable paired-Device credentials.

mod pairing;
mod persistence;

pub use pairing::{CredentialState, DeviceCredential, PairedDeviceEnrollment, PairingError, PairingRecord, PairingStore, PairingToken};

/// Stable identity of the authenticated caller used for resource admission.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CallerId(String);

impl CallerId {
  pub fn local_owner() -> Self {
    Self("local-owner".to_string())
  }

  pub(crate) fn paired_device(pair_id: &str) -> Self {
    Self(format!("paired-device:{pair_id}"))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}
