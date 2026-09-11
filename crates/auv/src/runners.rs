//! Runner and RunnerClass control operations.

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use auv_api_proto::auv::api::daemon::v1 as proto;

use crate::client::Client;
use crate::error::ClientError;
use crate::resource::{DeviceId, RunnerClassId, RunnerClassSelector, RunnerId, RunnerSelector};
use crate::time::Timestamp;

/// Requested lifetime policy for a Runner process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerLifecycle {
  /// Stop after the final admitted operation completes.
  Ephemeral,
  /// Stop after the configured idle interval.
  UnlessIdle,
  /// Remain available until the daemon shuts down or an explicit stop.
  UnlessShutdown,
}

/// Current Runner lifecycle phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerPhase {
  /// The daemon did not report a recognized phase.
  Unspecified,
  /// The Runner process is starting.
  Starting,
  /// The Runner can accept routed operations.
  Ready,
  /// The Runner is finishing active work before stopping.
  Draining,
  /// The Runner has stopped.
  Stopped,
  /// The Runner terminated with a failure.
  Failed,
}

/// A RunnerClass advertised by a Device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerClass {
  /// Exact RunnerClass identity.
  pub id: RunnerClassId,
  /// Device that owns this advertisement, if scoped.
  pub device: Option<DeviceId>,
  /// Human-facing name.
  pub display_name: String,
  /// Supported lifecycle policies.
  pub supported_lifecycles: Vec<RunnerLifecycle>,
  /// Whether the class can currently create a Runner.
  pub available: bool,
}

/// A daemon-managed Runner instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runner {
  /// Canonical Runner identity.
  pub id: RunnerId,
  /// Owning Device.
  pub device: DeviceId,
  /// RunnerClass used to create the instance.
  pub class: RunnerClassId,
  /// Caller-supplied labels.
  pub labels: HashMap<String, String>,
  /// Configured lifetime policy.
  pub lifecycle: RunnerLifecycle,
  /// Idle timeout for `UnlessIdle` runners.
  pub idle_timeout: Option<Duration>,
  /// Current lifecycle phase.
  pub phase: RunnerPhase,
  /// Creation time reported by the daemon.
  pub created_at: Option<Timestamp>,
  /// Host process identifier when available.
  pub process_id: Option<u32>,
  /// Number of active routed operations.
  pub active_operations: u64,
  /// Scheduled idle deadline when applicable.
  pub idle_deadline: Option<Timestamp>,
}

/// Typed input for creating a Runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateRunner {
  /// Device placement, or daemon-selected placement when absent.
  pub device: Option<DeviceId>,
  /// RunnerClass to instantiate.
  pub class: RunnerClassId,
  /// Caller-supplied labels.
  pub labels: HashMap<String, String>,
  /// Requested lifetime policy.
  pub lifecycle: RunnerLifecycle,
  /// Idle timeout for `UnlessIdle`.
  pub idle_timeout: Option<Duration>,
}

/// Options for stopping a Runner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StopRunner {
  /// Grace period before forced termination.
  pub grace_period: Option<Duration>,
  /// Whether to force termination.
  pub force: bool,
}

/// Failure from Runner or RunnerClass control.
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
  /// The daemon client request failed.
  #[error(transparent)]
  Client(#[from] ClientError),
  /// A Runner, RunnerClass, or Device identity is malformed.
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
  /// A required field is absent from the daemon response.
  #[error("Runner response omitted {0}")]
  MissingField(&'static str),
  /// No Runner matches the selector.
  #[error("unknown Runner ID {0:?}")]
  NotFound(String),
  /// More than one Runner matches the ID prefix.
  #[error("ambiguous Runner ID prefix {0:?}; provide more characters")]
  Ambiguous(String),
  /// The Runner is not owned by the root-selected Device.
  #[error("Runner is not owned by selected Device {0:?}")]
  DeviceConflict(String),
  /// A duration cannot be represented by the wire contract.
  #[error("duration exceeds the protocol range")]
  DurationRange,
}

/// Runner and RunnerClass operations bound to one selected daemon.
#[derive(Clone, Debug)]
pub struct Runners {
  client: Client,
}

impl Runners {
  pub(crate) fn new(client: Client) -> Self {
    Self { client }
  }

  /// Creates a Runner.
  pub async fn create(&self, request: CreateRunner) -> Result<Runner, RunnerError> {
    let response = self
      .client
      .grpc_client()
      .runners()
      .create_runner(proto::CreateRunnerRequest {
        device: request.device.map(|id| proto::DeviceRef {
          device_id: id.to_string(),
        }),
        runner_class: Some(proto::RunnerClassRef {
          runner_class: request.class.to_string(),
        }),
        labels: request.labels,
        lifecycle: proto::RunnerLifecycle::from(request.lifecycle) as i32,
        idle_timeout: request.idle_timeout.map(duration_to_proto).transpose()?,
      })
      .await
      .map_err(|status| ClientError::from_status("CreateRunner", status))?;
    Runner::try_from(response)
  }

  /// Lists Runner instances.
  pub async fn list(&self) -> Result<Vec<Runner>, RunnerError> {
    self
      .client
      .grpc_client()
      .runners()
      .list_runners()
      .await
      .map_err(|status| ClientError::from_status("ListRunners", status))?
      .into_iter()
      .map(Runner::try_from)
      .collect()
  }

  /// Resolves one Runner and optionally validates its owning Device.
  pub async fn get(&self, selector: &RunnerSelector, device: Option<&DeviceId>) -> Result<Runner, RunnerError> {
    let id = resolve(selector, &self.list().await?)?;
    let runner = Runner::try_from(
      self
        .client
        .grpc_client()
        .runners()
        .get_runner(id.to_string())
        .await
        .map_err(|status| ClientError::from_status("GetRunner", status))?,
    )?;
    validate_device(&runner, device)?;
    Ok(runner)
  }

  /// Stops one resolved Runner.
  pub async fn stop(&self, selector: &RunnerSelector, device: Option<&DeviceId>, options: StopRunner) -> Result<Runner, RunnerError> {
    let existing = self.get(selector, device).await?;
    let response = self
      .client
      .grpc_client()
      .runners()
      .delete_runner_with_options(existing.id.to_string(), options.grace_period.map(duration_to_proto).transpose()?, options.force)
      .await
      .map_err(|status| ClientError::from_status("DeleteRunner", status))?;
    Runner::try_from(response)
  }

  /// Lists RunnerClasses, optionally scoped to one Device.
  pub async fn classes(&self, device: Option<&DeviceId>) -> Result<Vec<RunnerClass>, RunnerError> {
    self
      .client
      .grpc_client()
      .runner_classes()
      .list_runner_classes(device.map(|id| proto::DeviceRef {
        device_id: id.to_string(),
      }))
      .await
      .map_err(|status| ClientError::from_status("ListRunnerClasses", status))?
      .into_iter()
      .map(RunnerClass::try_from)
      .collect()
  }

  /// Gets one exact RunnerClass on the selected Device, if any.
  pub async fn class(&self, selector: &RunnerClassSelector, device: Option<&DeviceId>) -> Result<RunnerClass, RunnerError> {
    let response = self
      .client
      .grpc_client()
      .runner_classes()
      .get_runner_class(
        selector.id().to_string(),
        device.map(|id| proto::DeviceRef {
          device_id: id.to_string(),
        }),
      )
      .await
      .map_err(|status| ClientError::from_status("GetRunnerClass", status))?;
    RunnerClass::try_from(response)
  }
}

fn resolve(selector: &RunnerSelector, runners: &[Runner]) -> Result<RunnerId, RunnerError> {
  let matches = runners.iter().filter(|runner| selector.matches(&runner.id)).collect::<Vec<_>>();
  match matches.as_slice() {
    [] => Err(RunnerError::NotFound(selector.as_str().to_string())),
    [runner] => Ok(runner.id.clone()),
    _ => Err(RunnerError::Ambiguous(selector.as_str().to_string())),
  }
}

fn validate_device(runner: &Runner, expected: Option<&DeviceId>) -> Result<(), RunnerError> {
  if expected.is_some_and(|expected| expected != &runner.device) {
    return Err(RunnerError::DeviceConflict(expected.expect("checked Some").to_string()));
  }
  Ok(())
}

fn duration_to_proto(value: Duration) -> Result<prost_types::Duration, RunnerError> {
  Ok(prost_types::Duration {
    seconds: i64::try_from(value.as_secs()).map_err(|_| RunnerError::DurationRange)?,
    nanos: i32::try_from(value.subsec_nanos()).expect("subsecond nanoseconds fit i32"),
  })
}

impl TryFrom<proto::Runner> for Runner {
  type Error = RunnerError;

  fn try_from(runner: proto::Runner) -> Result<Self, Self::Error> {
    let lifecycle = proto::RunnerLifecycle::try_from(runner.lifecycle).unwrap_or(proto::RunnerLifecycle::Unspecified);
    Ok(Self {
      id: RunnerId::from_str(&runner.r#ref.ok_or(RunnerError::MissingField("canonical ID"))?.runner_id)?,
      device: DeviceId::from_str(&runner.device.ok_or(RunnerError::MissingField("Device"))?.device_id)?,
      class: RunnerClassId::from_str(&runner.runner_class.ok_or(RunnerError::MissingField("RunnerClass"))?.runner_class)?,
      labels: runner.labels,
      lifecycle: RunnerLifecycle::try_from(lifecycle)?,
      idle_timeout: runner.idle_timeout.map(|value| Duration::new(value.seconds.max(0) as u64, value.nanos.max(0) as u32)),
      phase: match proto::RunnerPhase::try_from(runner.phase).unwrap_or(proto::RunnerPhase::Unspecified) {
        proto::RunnerPhase::Unspecified => RunnerPhase::Unspecified,
        proto::RunnerPhase::Starting => RunnerPhase::Starting,
        proto::RunnerPhase::Ready => RunnerPhase::Ready,
        proto::RunnerPhase::Draining => RunnerPhase::Draining,
        proto::RunnerPhase::Stopped => RunnerPhase::Stopped,
        proto::RunnerPhase::Failed => RunnerPhase::Failed,
      },
      created_at: runner.created_at.map(Into::into),
      process_id: (runner.process_id != 0).then_some(runner.process_id),
      active_operations: runner.active_operations,
      idle_deadline: runner.idle_deadline.map(Into::into),
    })
  }
}

impl TryFrom<proto::RunnerClass> for RunnerClass {
  type Error = RunnerError;

  fn try_from(class: proto::RunnerClass) -> Result<Self, Self::Error> {
    Ok(Self {
      id: RunnerClassId::from_str(&class.r#ref.ok_or(RunnerError::MissingField("RunnerClass ID"))?.runner_class)?,
      device: class.device.map(|device| DeviceId::from_str(&device.device_id)).transpose()?,
      display_name: class.display_name,
      supported_lifecycles: class
        .supported_lifecycles
        .into_iter()
        .filter_map(|value| proto::RunnerLifecycle::try_from(value).ok())
        .filter_map(|value| RunnerLifecycle::try_from(value).ok())
        .collect(),
      available: class.available,
    })
  }
}

impl TryFrom<proto::RunnerLifecycle> for RunnerLifecycle {
  type Error = RunnerError;

  fn try_from(value: proto::RunnerLifecycle) -> Result<Self, Self::Error> {
    match value {
      proto::RunnerLifecycle::Ephemeral => Ok(Self::Ephemeral),
      proto::RunnerLifecycle::UnlessIdle => Ok(Self::UnlessIdle),
      proto::RunnerLifecycle::UnlessShutdown => Ok(Self::UnlessShutdown),
      proto::RunnerLifecycle::Unspecified => Err(RunnerError::MissingField("Runner lifecycle")),
    }
  }
}

impl From<RunnerLifecycle> for proto::RunnerLifecycle {
  fn from(value: RunnerLifecycle) -> Self {
    match value {
      RunnerLifecycle::Ephemeral => Self::Ephemeral,
      RunnerLifecycle::UnlessIdle => Self::UnlessIdle,
      RunnerLifecycle::UnlessShutdown => Self::UnlessShutdown,
    }
  }
}
