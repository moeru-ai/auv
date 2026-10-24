use crate::{discovery, profile};

/// Resolved, non-secret context inherited by an AUV plugin invocation.
///
/// This value is passed inline through `AUV_CONTEXT`. It intentionally stores
/// only stable references; credentials are resolved by the selected client
/// profile and must never be serialized into this process contract.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AuvContext {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub run_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub daemon_endpoint: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_profile: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub invocation_id: Option<String>,
}

impl AuvContext {
  pub fn from_env() -> Result<Self, ContextError> {
    let value = std::env::var("AUV_CONTEXT").map_err(ContextError::Environment)?;
    serde_json::from_str(&value).map_err(ContextError::Decode)
  }
}

/// Set by the daemon on a Runner whose stdin carries the daemon credential.
pub const CREDENTIAL_STDIN_ENV: &str = "AUV_CREDENTIAL_STDIN";

/// The credential a daemon handed this Runner on its stdin, read once per
/// process. `None` when the daemon sent none. Call it before anything else
/// reads stdin, then pass it to [`crate::Client::from_context_with_credential`].
/// The daemon uses stdin because other processes of the same user can read
/// the environment, where `AUV_CONTEXT` lives.
pub fn inherited_credential() -> Result<Option<crate::selection::Credential>, ContextError> {
  static CREDENTIAL: std::sync::Mutex<Option<Option<crate::selection::Credential>>> = std::sync::Mutex::new(None);
  if std::env::var_os(CREDENTIAL_STDIN_ENV).is_none() {
    return Ok(None);
  }
  let mut cached = CREDENTIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
  if let Some(credential) = cached.as_ref() {
    return Ok(credential.clone());
  }
  let mut line = zeroize::Zeroizing::new(String::new());
  std::io::stdin().read_line(&mut line).map_err(ContextError::CredentialStdin)?;
  let value = line.trim_end_matches(['\r', '\n']);
  let credential = (!value.is_empty()).then(|| crate::selection::Credential::new(value.to_owned()));
  *cached = Some(credential.clone());
  Ok(credential)
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
  #[error("failed to read the credential the daemon sent on stdin: {0}")]
  CredentialStdin(std::io::Error),
  #[error("AUV_CONTEXT is unavailable or is not valid Unicode: {0}")]
  Environment(std::env::VarError),
  #[error("AUV_CONTEXT is not valid JSON: {0}")]
  Decode(serde_json::Error),
  #[error("could not resolve an AUV daemon endpoint from the context or local discovery")]
  EndpointNotDiscovered,
  #[error("context daemon endpoint {context:?} does not match paired Device profile endpoint {profile:?}")]
  ProfileEndpointMismatch { context: String, profile: String },
  #[error(transparent)]
  Profile(#[from] profile::ProfileError),
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
  #[error("paired daemon ListDevices failed: {0}")]
  RemoteDeviceList(crate::error::ClientError),
  #[error("paired daemon did not expose configured canonical Device ID {0:?}")]
  CanonicalDeviceMissing(String),
  #[error("Device selection {selector:?} is ambiguous across local and paired profiles; candidate IDs: {candidate_ids}")]
  DeviceSelectionAmbiguous {
    selector: String,
    candidate_ids: String,
  },
  #[error("Device selection does not match the local daemon or a paired Device profile")]
  DeviceNotConfigured,
  #[error("Run {0:?} was not found on the local daemon or any configured paired Device")]
  RunNotFound(String),
  #[error("Run {run_id:?} exists on more than one daemon: {locations}")]
  RunAmbiguous { run_id: String, locations: String },
  #[error("failed to look up Run on {location}: {error}")]
  RunLookup {
    location: String,
    error: crate::error::ClientError,
  },
  #[error(transparent)]
  Discovery(#[from] discovery::DiscoveryError),
  #[error("failed to connect to AUV API server: {0}")]
  Connect(String),
  #[error("failed to connect to paired AUV API server: {0}")]
  PairedConnect(String),
}

#[cfg(test)]
#[path = "context_test.rs"]
mod tests;
