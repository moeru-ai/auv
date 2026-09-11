//! Run control operations and resource-specific validation.

use std::collections::HashMap;
use std::str::FromStr;

use auv_api_proto::auv::api::daemon::v1 as proto;

use crate::client::Client;
use crate::error::ClientError;
use crate::resource::{DeviceId, RunId, RunSelector};
use crate::time::Timestamp;

/// Lifecycle phase of a Run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPhase {
  /// The daemon did not report a recognized phase.
  Unspecified,
  /// The Run exists but execution has not begun.
  Pending,
  /// The Run is active.
  Running,
  /// The Run completed successfully.
  Succeeded,
  /// The Run completed with failure.
  Failed,
  /// The Run was canceled.
  Canceled,
}

/// Terminal outcome requested when stopping a Run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
  /// Mark the Run successful.
  Succeeded,
  /// Mark the Run failed.
  Failed,
  /// Mark the Run canceled.
  Canceled,
}

/// Durable Run metadata returned by the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
  /// Canonical Run identity.
  pub id: RunId,
  /// Current lifecycle phase.
  pub phase: RunPhase,
  /// Devices associated with the Run.
  pub devices: Vec<DeviceId>,
  /// Caller-supplied labels.
  pub labels: HashMap<String, String>,
  /// Creation time reported by the daemon.
  pub created_at: Option<Timestamp>,
  /// Completion time, once terminal.
  pub completed_at: Option<Timestamp>,
}

/// Typed input for creating a Run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreateRun {
  /// Devices participating in the Run.
  pub devices: Vec<DeviceId>,
  /// Caller-supplied labels.
  pub labels: HashMap<String, String>,
}

/// Failure from Run control or selection.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
  /// The daemon client request failed.
  #[error(transparent)]
  Client(#[from] ClientError),
  /// A Run or Device identity is malformed.
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
  /// The daemon omitted the canonical Run identity.
  #[error("Run response omitted its canonical ID")]
  MissingIdentity,
  /// No Run matches the selector.
  #[error("unknown Run ID {0:?}")]
  NotFound(String),
  /// More than one Run matches the ID prefix.
  #[error("ambiguous Run ID prefix {0:?}; provide more characters")]
  Ambiguous(String),
  /// The selected Run does not include the root-selected Device.
  #[error("Run {run_id:?} does not include selected Device {device_id:?}")]
  DeviceConflict {
    /// Run selected by the operation.
    run_id: String,
    /// Device selected by the frontend root.
    device_id: String,
  },
  /// The operation target differs from the root-selected Run.
  #[error("Run {actual:?} conflicts with selected Run {selected:?}")]
  SelectionConflict {
    /// Run selected by the operation.
    actual: String,
    /// Run selected by the frontend root.
    selected: String,
  },
}

/// Run control operations bound to one selected daemon.
#[derive(Clone, Debug)]
pub struct Runs {
  client: Client,
}

impl Runs {
  pub(crate) fn new(client: Client) -> Self {
    Self { client }
  }

  /// Creates a Run.
  pub async fn create(&self, request: CreateRun) -> Result<Run, RunError> {
    let response = self
      .client
      .grpc_client()
      .runs()
      .create_run(proto::CreateRunRequest {
        devices: request
          .devices
          .into_iter()
          .map(|id| proto::DeviceRef {
            device_id: id.to_string(),
          })
          .collect(),
        labels: request.labels,
      })
      .await
      .map_err(|status| ClientError::from_status("CreateRun", status))?;
    Run::try_from(response)
  }

  /// Lists Runs visible through the selected daemon.
  pub async fn list(&self) -> Result<Vec<Run>, RunError> {
    self
      .client
      .grpc_client()
      .runs()
      .list_runs()
      .await
      .map_err(|status| ClientError::from_status("ListRuns", status))?
      .into_iter()
      .map(Run::try_from)
      .collect()
  }

  /// Resolves and returns one Run.
  pub async fn get(&self, selector: &RunSelector) -> Result<Run, RunError> {
    let id = resolve(selector, &self.list().await?)?;
    let response =
      self.client.grpc_client().runs().get_run(id.to_string()).await.map_err(|status| ClientError::from_status("GetRun", status))?;
    Run::try_from(response)
  }

  /// Stops one resolved Run with the requested outcome.
  pub async fn stop(&self, selector: &RunSelector, outcome: RunOutcome) -> Result<Run, RunError> {
    let id = resolve(selector, &self.list().await?)?;
    let response = self
      .client
      .grpc_client()
      .runs()
      .stop_run(id.to_string(), outcome.into())
      .await
      .map_err(|status| ClientError::from_status("StopRun", status))?;
    Run::try_from(response)
  }
}

fn resolve(selector: &RunSelector, runs: &[Run]) -> Result<RunId, RunError> {
  let matches = runs.iter().filter(|run| selector.matches(&run.id)).collect::<Vec<_>>();
  match matches.as_slice() {
    [] => Err(RunError::NotFound(selector.as_str().to_string())),
    [run] => Ok(run.id.clone()),
    _ => Err(RunError::Ambiguous(selector.as_str().to_string())),
  }
}

impl Run {
  /// Validates that this resource is the Run independently selected by a
  /// frontend root context.
  pub fn validate_selection(&self, selected: Option<&Run>) -> Result<(), RunError> {
    if let Some(selected) = selected
      && selected.id != self.id
    {
      return Err(RunError::SelectionConflict {
        actual: self.id.to_string(),
        selected: selected.id.to_string(),
      });
    }
    Ok(())
  }

  /// Validates that an independently selected Device belongs to this Run.
  pub fn validate_device(&self, device: Option<&DeviceId>) -> Result<(), RunError> {
    if let Some(device) = device
      && !self.devices.contains(device)
    {
      return Err(RunError::DeviceConflict {
        run_id: self.id.to_string(),
        device_id: device.to_string(),
      });
    }
    Ok(())
  }
}

impl TryFrom<proto::Run> for Run {
  type Error = RunError;

  fn try_from(run: proto::Run) -> Result<Self, Self::Error> {
    let id = RunId::from_str(&run.r#ref.ok_or(RunError::MissingIdentity)?.run_id)?;
    Ok(Self {
      id,
      phase: match proto::RunPhase::try_from(run.phase).unwrap_or(proto::RunPhase::Unspecified) {
        proto::RunPhase::Unspecified => RunPhase::Unspecified,
        proto::RunPhase::Pending => RunPhase::Pending,
        proto::RunPhase::Running => RunPhase::Running,
        proto::RunPhase::Succeeded => RunPhase::Succeeded,
        proto::RunPhase::Failed => RunPhase::Failed,
        proto::RunPhase::Canceled => RunPhase::Canceled,
      },
      devices: run.devices.into_iter().map(|device| DeviceId::from_str(&device.device_id)).collect::<Result<_, _>>()?,
      labels: run.labels,
      created_at: run.created_at.map(Into::into),
      completed_at: run.completed_at.map(Into::into),
    })
  }
}

impl From<RunOutcome> for proto::RunOutcome {
  fn from(value: RunOutcome) -> Self {
    match value {
      RunOutcome::Succeeded => Self::Succeeded,
      RunOutcome::Failed => Self::Failed,
      RunOutcome::Canceled => Self::Canceled,
    }
  }
}
