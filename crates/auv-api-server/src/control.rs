//! Protocol-facing port implemented by the daemon server SDK.

use tonic::transport::Channel;

/// Authenticated caller identity supplied to daemon control operations.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CallerId(String);

impl CallerId {
  /// Returns the trusted local owner identity.
  pub fn local_owner() -> Self {
    Self("local-owner".to_string())
  }
  /// Returns an identity for one authenticated paired Device.
  pub fn paired_device(pair_id: &str) -> Self {
    Self(format!("paired-device:{pair_id}"))
  }
  /// Returns the stable identity text.
  pub fn as_str(&self) -> &str {
    &self.0
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

/// Typed daemon control port consumed by gRPC and REST protocol adapters.
#[tonic::async_trait]
pub trait Control: Send + Sync {
  /// Lists Devices.
  fn list_devices(&self) -> Result<Vec<auv::devices::Device>, ControlError>;
  /// Gets a Device by canonical identity.
  fn get_device(&self, device_id: &str) -> Result<Option<auv::devices::Device>, ControlError>;
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
