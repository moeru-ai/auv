//! Device, Run, and Runner placement above the transport client.
//!
//! This module owns selector precedence and implicit Run lifecycle. Capability
//! clients receive canonical refs and do not need to know how placement was
//! chosen.

use std::collections::HashMap;

use auv_api_proto::auv::api::core::v1 as proto;

use crate::{AuvContext, Client, ContextError, discovery, driver};

#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
  #[error(transparent)]
  Context(#[from] ContextError),
  #[error("AUV_CONTEXT is not valid Unicode: {0}")]
  ContextEnvironment(std::env::VarError),
  #[error(transparent)]
  Status(#[from] tonic::Status),
  #[error("{0}")]
  Selection(String),
  #[error("{primary}; cleanup also failed: {cleanup}")]
  Cleanup { primary: String, cleanup: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceSelector {
  pub id: Option<String>,
  pub name: Option<String>,
}

impl DeviceSelector {
  pub fn by_id(id: impl Into<String>) -> Self {
    Self {
      id: Some(id.into()),
      name: None,
    }
  }

  pub fn by_name(name: impl Into<String>) -> Self {
    Self {
      id: None,
      name: Some(name.into()),
    }
  }

  fn is_empty(&self) -> bool {
    self.id.is_none() && self.name.is_none()
  }
}

#[derive(Clone, Debug, Default)]
pub enum RunSelection {
  /// Inherit `AUV_CONTEXT.run_id` when present, otherwise create a Run.
  #[default]
  Auto,
  Existing(String),
  /// Explicitly create a Run even when inherited context names another Run.
  New,
}

#[derive(Clone, Debug, Default)]
pub struct RunOptions {
  pub selection: RunSelection,
  pub device: DeviceSelector,
  pub labels: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct RunnerOptions {
  pub device: DeviceSelector,
  pub device_match_labels: HashMap<String, String>,
  pub runner_class: String,
  pub required_capabilities: Vec<proto::RunnerCapability>,
  pub match_labels: HashMap<String, String>,
  pub reuse_policy: proto::RunnerReusePolicy,
  pub lifecycle: proto::RunnerLifecycle,
  pub idle_timeout: Option<prost_types::Duration>,
  pub operation_capacity: u32,
}

impl Default for RunnerOptions {
  fn default() -> Self {
    Self {
      device: DeviceSelector::default(),
      device_match_labels: HashMap::new(),
      runner_class: "auv.core.local".to_string(),
      required_capabilities: Vec::new(),
      match_labels: HashMap::new(),
      reuse_policy: proto::RunnerReusePolicy::PreferExisting,
      lifecycle: proto::RunnerLifecycle::Ephemeral,
      idle_timeout: None,
      operation_capacity: 1,
    }
  }
}

impl RunnerOptions {
  pub fn requiring(required_capabilities: Vec<proto::RunnerCapability>) -> Self {
    Self {
      required_capabilities,
      ..Self::default()
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PlacementConstraint {
  #[default]
  Automatic,
  LocalOnly,
}

/// High-level client that selects Device, Run, and Runner resources without
/// exposing the underlying Unix-socket, TCP, or paired-TLS transport.
#[derive(Clone, Debug)]
pub struct AuvClient {
  client: Client,
  constraint: PlacementConstraint,
}

impl AuvClient {
  pub fn from_client(client: Client) -> Self {
    Self {
      client,
      constraint: PlacementConstraint::Automatic,
    }
  }

  pub async fn from_context(context: AuvContext) -> Result<Self, PlacementError> {
    Ok(Self::from_client(Client::from_context(context).await?))
  }

  /// Uses inherited plugin context when present, otherwise discovers the
  /// current user's local daemon.
  pub async fn from_env_or_local() -> Result<Self, PlacementError> {
    match std::env::var("AUV_CONTEXT") {
      Ok(value) => {
        let context = serde_json::from_str(&value).map_err(|error| PlacementError::Context(ContextError::Decode(error)))?;
        Self::from_context(context).await
      }
      Err(std::env::VarError::NotPresent) => {
        let endpoint = discovery::resolve(None).map_err(ContextError::Discovery)?.ok_or(ContextError::EndpointNotDiscovered)?;
        Ok(Self::from_client(Client::connect(endpoint).await.map_err(ContextError::Connect)?))
      }
      Err(error) => Err(PlacementError::ContextEnvironment(error)),
    }
  }

  /// Constrains placement to the current user's local daemon and its local
  /// Device. It never reinterprets a paired daemon's own `Device.local` bit as
  /// caller-local placement.
  pub fn local(mut self) -> Result<Self, PlacementError> {
    if self.client.is_paired_remote() {
      return Err(PlacementError::Selection("local placement conflicts with an explicitly paired remote daemon transport".to_string()));
    }
    self.constraint = PlacementConstraint::LocalOnly;
    Ok(self)
  }

  pub async fn run(&self, options: RunOptions) -> Result<RunClient, PlacementError> {
    // TODO(distributed-run-authority): one Run currently owns one daemon
    // transport; see the accepted aggregated API design before adding peers.
    let mut client = self.client.clone();
    let devices = client.list_devices().await?;
    let context = client.context().cloned().unwrap_or_default();
    let run_id = match &options.selection {
      RunSelection::Auto => context.run_id.clone(),
      RunSelection::Existing(run_id) => Some(run_id.clone()),
      RunSelection::New => None,
    };
    let existing = match run_id {
      Some(run_id) => Some(client.get_run(run_id).await?),
      None => None,
    };
    if let Some(run) = &existing
      && run.phase != proto::RunPhase::Running as i32
    {
      return Err(PlacementError::Selection("Runner placement requires a running Run".to_string()));
    }

    let explicit = (!options.device.is_empty()).then_some(options.device.clone());
    let inherited = context_device_selector(&context);
    let selector = explicit.or(inherited);
    let allowed = existing.as_ref().map(|run| run.devices.as_slice());
    let selected_device = match selector {
      Some(selector) => Some(select_device(&devices, &selector, self.constraint, allowed)?),
      None => select_default_device(&devices, self.constraint, allowed)?,
    };

    let (run, owned) = match existing {
      Some(run) => (run, false),
      None => {
        let device =
          selected_device.as_ref().ok_or_else(|| PlacementError::Selection("creating a Run requires one unambiguous Device".to_string()))?;
        let device_ref = required_device_ref(device)?;
        (
          client
            .create_run(proto::CreateRunRequest {
              devices: vec![device_ref],
              labels: options.labels,
            })
            .await?,
          true,
        )
      }
    };

    let run_devices = devices
      .into_iter()
      .filter(|device| {
        device.r#ref.as_ref().is_some_and(|reference| run.devices.iter().any(|candidate| candidate.device_id == reference.device_id))
      })
      .collect();
    Ok(RunClient {
      auv: self.clone(),
      run,
      selected_device,
      run_devices,
      owned,
    })
  }

  /// Creates or inherits a Run, then claims a Runner using the same placement
  /// rules. The returned execution owns cleanup only when it created the Run.
  pub async fn runner(&self, options: RunnerOptions) -> Result<RunnerExecution, PlacementError> {
    self.runner_with(RunOptions::default(), options).await
  }

  pub async fn runner_with(&self, run_options: RunOptions, runner_options: RunnerOptions) -> Result<RunnerExecution, PlacementError> {
    let run = self.run(run_options).await?;
    match run.runner(runner_options).await {
      Ok(runner) => Ok(RunnerExecution { run, runner }),
      Err(primary) if run.is_owned() => match run.finish_if_owned(proto::RunOutcome::Canceled).await {
        Ok(_) => Err(primary),
        Err(cleanup) => Err(PlacementError::Cleanup {
          primary: primary.to_string(),
          cleanup: cleanup.to_string(),
        }),
      },
      Err(primary) => Err(primary),
    }
  }
}

#[derive(Debug)]
pub struct RunClient {
  auv: AuvClient,
  run: proto::Run,
  selected_device: Option<proto::Device>,
  run_devices: Vec<proto::Device>,
  owned: bool,
}

// TODO(run-abrupt-cleanup): network cleanup cannot be made reliable from
// async Drop. Add a cancellation-aware scoped helper once frontend signal
// forwarding owns a bounded cleanup deadline; normal paths must call
// `finish_if_owned` or `RunnerExecution::finish` explicitly.

#[derive(Debug)]
pub struct RunnerExecution {
  run: RunClient,
  runner: driver::RunnerClient,
}

impl RunnerExecution {
  pub fn run(&self) -> &proto::Run {
    self.run.resource()
  }

  pub fn runner(&self) -> Option<&proto::Runner> {
    self.runner.resource()
  }

  pub fn displays(&self) -> driver::DisplaysClient {
    self.runner.displays()
  }

  pub fn windows(&self) -> driver::WindowsClient {
    self.runner.windows()
  }

  pub fn input(&self) -> driver::InputClient {
    self.runner.input()
  }

  pub fn overlay(&self) -> driver::OverlayClient {
    self.runner.overlay()
  }

  pub fn macos(&self) -> driver::MacosClient {
    self.runner.macos()
  }

  pub fn inference(&self) -> driver::InferenceClient {
    self.runner.inference()
  }

  pub async fn recognize_text(
    &self,
    capture: auv_api_proto::auv::api::driver::v1::CapturedFrame,
    region: Option<auv_api_proto::auv::api::image::v1::NormalizedRect>,
    custom_words: Vec<String>,
    recognition_languages: Vec<String>,
  ) -> Result<auv_api_proto::auv::api::driver::v1::RecognizeTextResponse, tonic::Status> {
    self.runner.recognize_text(capture, region, custom_words, recognition_languages).await
  }

  pub async fn finish(self, outcome: proto::RunOutcome) -> Result<proto::Run, PlacementError> {
    let RunnerExecution { run, runner } = self;
    let release = runner.release().await;
    let finish = run.finish_if_owned(outcome).await;
    match (release, finish) {
      (Ok(_), Ok(run)) => Ok(run),
      (Err(primary), Ok(_)) => Err(primary.into()),
      (Ok(_), Err(primary)) => Err(primary),
      (Err(primary), Err(cleanup)) => Err(PlacementError::Cleanup {
        primary: primary.to_string(),
        cleanup: cleanup.to_string(),
      }),
    }
  }

  pub async fn release(self) -> Result<proto::Run, PlacementError> {
    self.finish(proto::RunOutcome::Succeeded).await
  }
}

impl RunClient {
  pub fn resource(&self) -> &proto::Run {
    &self.run
  }

  pub fn device(&self) -> Option<&proto::Device> {
    self.selected_device.as_ref()
  }

  pub fn is_owned(&self) -> bool {
    self.owned
  }

  pub fn local(mut self) -> Result<Self, PlacementError> {
    self.auv = self.auv.local()?;
    self.selected_device = select_default_device(&self.run_devices, PlacementConstraint::LocalOnly, Some(&self.run.devices))?;
    Ok(self)
  }

  pub async fn runner(&self, options: RunnerOptions) -> Result<driver::RunnerClient, PlacementError> {
    let run_ref = self
      .run
      .r#ref
      .clone()
      .filter(|run| !run.run_id.trim().is_empty())
      .ok_or_else(|| PlacementError::Selection("Run omitted its canonical ref".to_string()))?;
    let selected_device = if options.device.is_empty() {
      self.selected_device.clone()
    } else {
      Some(select_device(&self.run_devices, &options.device, self.auv.constraint, Some(&self.run.devices))?)
    };
    if self.auv.constraint == PlacementConstraint::LocalOnly && selected_device.as_ref().is_none_or(|device| !device.local) {
      return Err(PlacementError::Selection("local Runner placement requires one caller-local Device in the Run".to_string()));
    }
    let device = selected_device.as_ref().map(required_device_ref).transpose()?;
    let mut client = self.auv.client.clone();
    let claimed = client
      .claim_runner(proto::RunnerClaim {
        run: Some(run_ref),
        device,
        device_match_labels: options.device_match_labels,
        runner_class: Some(proto::RunnerClassRef {
          runner_class: options.runner_class,
        }),
        required_capabilities: options.required_capabilities,
        match_labels: options.match_labels,
        reuse_policy: options.reuse_policy as i32,
        lifecycle: Some(options.lifecycle as i32),
        idle_timeout: options.idle_timeout,
        operation_capacity: options.operation_capacity,
      })
      .await?;
    let runner = claimed.runner.ok_or_else(|| tonic::Status::internal("ClaimRunner response omitted Runner"))?;
    let lease =
      claimed.lease.and_then(|lease| lease.r#ref).ok_or_else(|| tonic::Status::internal("ClaimRunner response omitted Runner lease"))?;
    Ok(driver::RunnerClient::from_claim(client, runner, lease)?)
  }

  /// Stops only Runs created implicitly by this high-level client. Attached
  /// Runs remain open for later operations.
  pub async fn finish_if_owned(mut self, outcome: proto::RunOutcome) -> Result<proto::Run, PlacementError> {
    if !self.owned {
      return Ok(self.run);
    }
    let run_id = self
      .run
      .r#ref
      .as_ref()
      .map(|run| run.run_id.clone())
      .filter(|run_id| !run_id.is_empty())
      .ok_or_else(|| PlacementError::Selection("Run omitted its canonical ref".to_string()))?;
    self.run = self.auv.client.clone().stop_run(run_id, outcome).await?;
    self.owned = false;
    Ok(self.run)
  }
}

fn context_device_selector(context: &AuvContext) -> Option<DeviceSelector> {
  match (&context.device_id, &context.device_name) {
    // Root-injected IDs are canonical. The name is only a display snapshot and
    // must not turn a later rename into a selector conflict.
    (Some(id), _) => Some(DeviceSelector::by_id(id.clone())),
    (None, Some(name)) => Some(DeviceSelector::by_name(name.clone())),
    (None, None) => None,
  }
}

fn select_default_device(
  devices: &[proto::Device],
  constraint: PlacementConstraint,
  allowed: Option<&[proto::DeviceRef]>,
) -> Result<Option<proto::Device>, PlacementError> {
  let allowed_devices = devices.iter().filter(|device| is_allowed(device, allowed)).collect::<Vec<_>>();
  if allowed.is_some() && constraint == PlacementConstraint::Automatic {
    return match allowed_devices.as_slice() {
      [device] => Ok(Some((*device).clone())),
      _ => Ok(None),
    };
  }
  let local = allowed_devices.into_iter().filter(|device| device.local).collect::<Vec<_>>();
  match local.as_slice() {
    [device] => Ok(Some((*device).clone())),
    [] => Err(PlacementError::Selection("the selected daemon exposes no eligible implicit local Device".to_string())),
    _ => Err(PlacementError::Selection("the selected daemon exposes more than one eligible implicit local Device".to_string())),
  }
}

fn select_device(
  devices: &[proto::Device],
  selector: &DeviceSelector,
  constraint: PlacementConstraint,
  allowed: Option<&[proto::DeviceRef]>,
) -> Result<proto::Device, PlacementError> {
  let candidates = devices.iter().filter(|device| is_allowed(device, allowed)).collect::<Vec<_>>();
  let by_id = match selector.id.as_deref() {
    Some(id) => Some(
      *candidates
        .iter()
        .find(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == id))
        .ok_or_else(|| PlacementError::Selection(format!("unknown or Run-ineligible Device ID {id:?}")))?,
    ),
    None => None,
  };
  let by_name = match selector.name.as_deref() {
    Some(name) => {
      let matches = candidates.iter().copied().filter(|device| device.name == name).collect::<Vec<_>>();
      match matches.as_slice() {
        [] => return Err(PlacementError::Selection(format!("unknown or Run-ineligible Device name {name:?}"))),
        [device] => Some(*device),
        matches => {
          let ids = matches.iter().map(|device| device_id(device)).collect::<Vec<_>>().join(", ");
          return Err(PlacementError::Selection(format!("Device name {name:?} is ambiguous; candidate IDs: {ids}")));
        }
      }
    }
    None => None,
  };
  let selected = match (by_id, by_name) {
    (Some(by_id), Some(by_name)) if !std::ptr::eq(by_id, by_name) => {
      return Err(PlacementError::Selection(format!(
        "Device name and ID select different Devices ({:?} and {:?})",
        device_id(by_name),
        device_id(by_id)
      )));
    }
    (Some(device), _) | (_, Some(device)) => device,
    (None, None) => {
      return select_default_device(devices, constraint, allowed)?
        .ok_or_else(|| PlacementError::Selection("Device selection is ambiguous".to_string()));
    }
  };
  if constraint == PlacementConstraint::LocalOnly && !selected.local {
    return Err(PlacementError::Selection(format!("local placement conflicts with remote Device {:?}", device_id(selected))));
  }
  Ok(selected.clone())
}

fn is_allowed(device: &proto::Device, allowed: Option<&[proto::DeviceRef]>) -> bool {
  allowed.is_none_or(|allowed| {
    device.r#ref.as_ref().is_some_and(|reference| allowed.iter().any(|candidate| candidate.device_id == reference.device_id))
  })
}

fn required_device_ref(device: &proto::Device) -> Result<proto::DeviceRef, PlacementError> {
  device
    .r#ref
    .clone()
    .filter(|reference| !reference.device_id.trim().is_empty())
    .ok_or_else(|| PlacementError::Selection("selected Device omitted its canonical ref".to_string()))
}

fn device_id(device: &proto::Device) -> &str {
  device.r#ref.as_ref().map(|reference| reference.device_id.as_str()).unwrap_or("<missing>")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn device(id: &str, name: &str, local: bool) -> proto::Device {
    proto::Device {
      r#ref: Some(proto::DeviceRef {
        device_id: id.to_string(),
      }),
      name: name.to_string(),
      local,
      ..Default::default()
    }
  }

  #[test]
  fn duplicate_names_report_stable_candidate_ids() {
    let devices = [
      device("device_a", "studio", true),
      device("device_b", "studio", false),
    ];
    let error = select_device(&devices, &DeviceSelector::by_name("studio"), PlacementConstraint::Automatic, None).unwrap_err();
    assert!(error.to_string().contains("device_a, device_b"));
  }

  #[test]
  fn local_constraint_rejects_an_explicit_remote_device() {
    let devices = [
      device("device_local", "local", true),
      device("device_remote", "remote", false),
    ];
    let error = select_device(&devices, &DeviceSelector::by_id("device_remote"), PlacementConstraint::LocalOnly, None).unwrap_err();
    assert!(error.to_string().contains("conflicts with remote Device"));
  }

  #[test]
  fn automatic_existing_multi_device_run_defers_placement_to_the_claim() {
    let devices = [
      device("device_a", "a", true),
      device("device_b", "b", false),
    ];
    let allowed = [
      proto::DeviceRef {
        device_id: "device_a".to_string(),
      },
      proto::DeviceRef {
        device_id: "device_b".to_string(),
      },
    ];
    assert!(select_default_device(&devices, PlacementConstraint::Automatic, Some(&allowed)).unwrap().is_none());
    assert_eq!(
      select_default_device(&devices, PlacementConstraint::LocalOnly, Some(&allowed))
        .unwrap()
        .and_then(|device| device.r#ref)
        .map(|reference| reference.device_id),
      Some("device_a".to_string())
    );
  }
}
