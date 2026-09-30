//! Protocol-facing port implemented by the daemon server SDK.

use std::hash::{Hash, Hasher};

use tonic::transport::Channel;

/// Authenticated caller identity supplied to daemon control operations.
#[derive(Clone)]
pub struct CallerId {
  identity: String,
  // This digest is authentication context only. Run ownership and audit use
  // the stable identity, and Debug must never print credential-derived data.
  credential_sha256: Option<String>,
}

impl std::fmt::Debug for CallerId {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_tuple("CallerId").field(&self.identity).finish()
  }
}

impl PartialEq for CallerId {
  fn eq(&self, other: &Self) -> bool {
    self.identity == other.identity
  }
}

impl Eq for CallerId {}

impl Hash for CallerId {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.identity.hash(state);
  }
}

impl CallerId {
  /// Returns the trusted local owner identity.
  pub fn local_owner() -> Self {
    Self {
      identity: "local-owner".to_string(),
      credential_sha256: None,
    }
  }

  /// Returns a stable paired Device identity without bearer proof. Device
  /// entry requires the credential-bearing constructor below.
  pub fn paired_device(pair_id: &str) -> Self {
    Self {
      identity: format!("paired-device:{pair_id}"),
      credential_sha256: None,
    }
  }

  /// Carries an authenticated bearer's opaque digest to later policy checks.
  /// The stable paired Device ID remains the Run and audit identity.
  /// Call only after the pairing store has authenticated the bearer.
  pub fn authenticated_paired_device(pair_id: &str, credential_sha256: String) -> Self {
    Self {
      identity: format!("paired-device:{pair_id}"),
      credential_sha256: Some(credential_sha256),
    }
  }

  /// Returns the stable paired Device ID, if this caller used pairing.
  pub fn paired_device_id(&self) -> Option<&str> {
    self.identity.strip_prefix("paired-device:")
  }

  /// Returns only the private authentication proof for live reauthorization.
  pub fn credential_sha256(&self) -> Option<&str> {
    self.credential_sha256.as_deref()
  }
  /// Returns the stable identity text.
  pub fn as_str(&self) -> &str {
    &self.identity
  }
}

/// Routing metadata for one Runner capability operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerRoute {
  /// Optional Device placement.
  pub device_id: Option<String>,
  /// Optional Run association.
  pub run_id: Option<String>,
  /// Required RunnerClass route.
  pub runner_class: String,
}

/// Lifetime permit retained while a routed operation is active.
pub trait OperationPermit: Send {}
impl<T: Send> OperationPermit for T {}

/// Admitted child-runner channel and its operation lifetime permit.
pub struct RoutedOperation {
  /// Connected child-runner channel.
  pub channel: Channel,
  /// Permit that releases admission state when dropped.
  pub permit: Box<dyn OperationPermit>,
}

/// Failure reported by the daemon control implementation.
#[derive(Debug, thiserror::Error)]
pub enum ControlError {
  /// The backend could not generate a control-plane resource identity.
  #[error("failed to generate control-plane identity: {0}")]
  Identity(String),
  /// A request contained an invalid control-plane argument.
  #[error("invalid control-plane argument: {0}")]
  InvalidArgument(&'static str),
  /// The selected Device does not exist.
  #[error("unknown Device: {0}")]
  UnknownDevice(String),
  /// The selected Run does not exist.
  #[error("unknown Run: {0}")]
  UnknownRun(String),
  /// The selected Runner does not exist.
  #[error("unknown Runner: {0}")]
  UnknownRunner(String),
  /// No provider can create the requested RunnerClass.
  #[error("no RunnerProvider is registered for RunnerClass: {0}")]
  RunnerProviderUnavailable(String),
  /// A routed Runner operation failed.
  #[error("Runner operation failed: {0}")]
  RunnerOperation(String),
}

/// Failure reported by the daemon pairing implementation.
#[derive(Debug, thiserror::Error)]
pub enum PairingError {
  /// The server has no pairing backend.
  #[error("pairing is not configured")]
  NotConfigured,
  /// The pairing request is malformed or violates a pairing invariant.
  #[error("pairing request is invalid: {0}")]
  Invalid(String),
  /// The enrollment token is invalid, expired, or already consumed.
  #[error("pairing token is invalid, expired, or has already been consumed")]
  InvalidToken,
  /// No paired Device matches the selector.
  #[error("paired Device was not found: {0}")]
  NotFound(String),
  /// More than one paired Device matches the selector.
  #[error("paired Device selector is ambiguous: {0}")]
  Ambiguous(String),
  /// The bearer credential is unknown, disabled, or revoked.
  #[error("Device credential is not paired or has been revoked")]
  Unauthenticated,
  /// Durable pairing state could not be read or updated.
  #[error("pairing persistence failed: {0}")]
  Persistence(String),
}

/// Newly issued one-time pairing token.
pub struct PairingToken {
  /// Opaque token value.
  pub token: String,
}

/// Credential material returned by successful enrollment.
pub struct Enrollment {
  /// Canonical remote Device identity.
  pub device_id: String,
  /// Opaque bearer credential.
  pub credential: String,
}

/// Pairing authentication and persistence port required by protocol adapters.
pub trait Pairing: Send + Sync {
  /// Authenticates one bearer credential.
  fn authenticate_bearer(&self, credential: &str) -> Result<CallerId, PairingError>;
  /// Rechecks live authority for a paired Device after an operation has queued.
  fn is_active_caller(&self, caller: &CallerId) -> bool;
  /// Issues a one-time enrollment token.
  fn issue_token(&self, lifetime: Option<std::time::Duration>) -> Result<PairingToken, PairingError>;
  /// Consumes a token and enrolls one paired Device.
  fn enroll(&self, token: &str, device_id: String, label: String) -> Result<Enrollment, PairingError>;
  /// Revokes bearer credentials while retaining the pairing record.
  fn revoke_device_credentials(&self, selector: &str) -> Result<bool, PairingError>;
  /// Enables or disables a paired Device record.
  fn set_enabled(&self, selector: &str, enabled: bool) -> Result<bool, PairingError>;
  /// Removes a paired Device and its credentials.
  fn unpair(&self, selector: &str) -> Result<bool, PairingError>;
}

#[cfg(test)]
mod caller_tests {
  use super::CallerId;

  #[test]
  fn bearer_proof_is_redacted_and_does_not_change_stable_caller_identity() {
    let digest = "private-digest".to_string();
    let authenticated = CallerId::authenticated_paired_device("tablet", digest.clone());
    let stable = CallerId::paired_device("tablet");

    assert_eq!(authenticated, stable);
    assert_eq!(authenticated.as_str(), "paired-device:tablet");
    assert!(!format!("{authenticated:?}").contains(&digest));
  }
}

/// Typed daemon control port consumed by gRPC and REST protocol adapters.
#[tonic::async_trait]
pub trait Control: Send + Sync {
  /// Lists Devices.
  fn list_devices(&self) -> Result<Vec<auv::devices::Device>, ControlError>;
  /// Gets a Device by canonical identity.
  fn get_device(&self, device_id: &str) -> Result<Option<auv::devices::Device>, ControlError>;
  /// Lists current OS login instances after target-local entry policy checks.
  fn list_user_sessions(&self, caller: &CallerId) -> Result<Vec<auv::devices::UserSession>, auv::devices::DeviceEntryErrorReason>;
  /// Resolves one current OS login instance by its opaque selector.
  fn get_user_session(
    &self,
    caller: &CallerId,
    session_selector: &str,
  ) -> Result<auv::devices::UserSession, auv::devices::DeviceEntryErrorReason>;
  /// Requests entry for a selected OS account or login instance.
  async fn ensure_user_session_unlocked(
    &self,
    caller: &CallerId,
    target: auv::devices::UserSessionTarget,
  ) -> Result<auv::devices::EnsureUserSessionUnlockedEffect, auv::devices::DeviceEntryErrorReason>;
  /// Requests a verified lock of one existing usable OS login instance.
  async fn ensure_user_session_locked(
    &self,
    caller: &CallerId,
    target: auv::devices::UserSessionTarget,
  ) -> Result<auv::devices::EnsureUserSessionLockedEffect, auv::devices::DeviceEntryErrorReason>;
  /// Creates a Run owned by the caller.
  fn create_run(&self, caller: &CallerId, request: auv::runs::CreateRun) -> Result<auv::runs::Run, ControlError>;
  /// Stops a caller-owned Run.
  async fn stop_run(&self, caller: &CallerId, run_id: &str, outcome: auv::runs::RunOutcome) -> Result<auv::runs::Run, ControlError>;
  /// Lists Runs visible to the caller.
  fn list_runs(&self, caller: &CallerId) -> Result<Vec<auv::runs::Run>, ControlError>;
  /// Gets one Run visible to the caller.
  fn get_run(&self, caller: &CallerId, run_id: &str) -> Result<auv::runs::Run, ControlError>;
  /// Lists Runner instances.
  fn list_runners(&self) -> Result<Vec<auv::runners::Runner>, ControlError>;
  /// Creates a Runner.
  async fn create_runner(&self, request: auv::runners::CreateRunner) -> Result<auv::runners::Runner, ControlError>;
  /// Gets one Runner by canonical identity.
  fn get_runner(&self, runner_id: &str) -> Result<auv::runners::Runner, ControlError>;
  /// Lists RunnerClasses, optionally scoped to one Device.
  fn list_runner_classes(&self, device_id: Option<&str>) -> Result<Vec<auv::runners::RunnerClass>, ControlError>;
  /// Gets one RunnerClass.
  fn get_runner_class(&self, device_id: Option<&str>, runner_class: &str) -> Result<auv::runners::RunnerClass, ControlError>;
  /// Stops one Runner.
  async fn delete_runner(&self, runner_id: &str, options: auv::runners::StopRunner) -> Result<auv::runners::Runner, ControlError>;
  /// Admits and routes one capability RPC to a child Runner.
  async fn admit_routed_channel(
    &self,
    caller: &CallerId,
    route: RunnerRoute,
    service: &str,
    method: &str,
  ) -> Result<RoutedOperation, ControlError>;
  /// Shuts down daemon-owned Runner processes.
  async fn shutdown(&self);
  /// Returns whether any Runner remains live.
  fn has_live_runners(&self) -> bool;
}
