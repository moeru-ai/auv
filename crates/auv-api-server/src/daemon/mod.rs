//! Long-lived Device, Run, and Runner state owned by one daemon.

mod runner;
pub mod runner_provider;

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::Mutex;

use auv_api_proto::auv::api::daemon::v1 as proto;
use tonic::transport::Channel;

use self::runner::RunnerSupervisor;
use crate::auth::CallerId;

const LOCAL_DEVICE_ID_FILE: &str = "device-id";

/// Process-local view of durable control-plane identity and live resources.
pub(crate) struct Daemon {
  local_device: proto::Device,
  // TODO(distributed-run-authority): Runs are daemon-memory resources until a
  // coordinator/storage slice defines cross-daemon ownership, recovery, and
  // terminal append semantics in the accepted architecture document.
  runs: Mutex<HashMap<String, OwnedRun>>,
  runner_affinities: Mutex<HashMap<RunnerAffinityKey, String>>,
  runner_affinity_locks: tokio::sync::Mutex<HashMap<RunnerAffinityKey, std::sync::Arc<tokio::sync::Mutex<()>>>>,
  runners: RunnerSupervisor,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RunnerAffinityKey {
  caller: CallerId,
  run_id: String,
  device_id: String,
  runner_class: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RunnerRoute {
  pub(crate) device_id: Option<String>,
  pub(crate) run_id: Option<String>,
  pub(crate) runner_class: String,
}

struct OwnedRun {
  caller: CallerId,
  run: proto::Run,
}

pub(crate) struct OperationPermit {
  _runner: runner::OperationPermit,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonError {
  #[error("failed to generate control-plane identity: {0}")]
  Identity(String),
  #[error("invalid control-plane argument: {0}")]
  InvalidArgument(&'static str),
  #[error("unknown Device: {0}")]
  UnknownDevice(String),
  #[error("unknown Run: {0}")]
  UnknownRun(String),
  #[error("unknown Runner: {0}")]
  UnknownRunner(String),
  #[error("no RunnerProvider is registered for RunnerClass: {0}")]
  RunnerProviderUnavailable(String),
  #[error("Runner operation failed: {0}")]
  RunnerOperation(String),
}

impl Daemon {
  #[cfg(test)]
  pub(crate) fn open(store_root: &Path) -> Result<Self, String> {
    Self::open_with_runner_providers_and_parent_endpoint(store_root, None, runner_provider::FirstPartyRunnerRuntimes::default(), Vec::new())
  }

  pub(super) fn open_with_runner_providers_and_parent_endpoint(
    store_root: &Path,
    parent_endpoint: Option<String>,
    first_party_runners: runner_provider::FirstPartyRunnerRuntimes,
    runner_providers: Vec<runner_provider::RunnerProviderConfig>,
  ) -> Result<Self, String> {
    let control_root = store_root.join("control");
    fs::create_dir_all(&control_root)
      .map_err(|error| format!("failed to create control-plane directory {}: {error}", control_root.display()))?;
    set_private_directory(&control_root)?;
    let device_id = load_or_create_local_device_id(&control_root.join(LOCAL_DEVICE_ID_FILE))?;
    let platform = local_platform();
    let mut labels = HashMap::new();
    labels.insert("auv.dev/platform".to_string(), proto::DevicePlatform::try_from(platform).unwrap_or_default().as_str_name().to_string());
    let local_device = proto::Device {
      r#ref: Some(proto::DeviceRef { device_id }),
      name: std::env::var("HOSTNAME").unwrap_or_default(),
      platform,
      local: true,
      labels,
    };
    let runners = RunnerSupervisor::with_providers(
      local_device.r#ref.clone().expect("local Device always has a ref"),
      parent_endpoint,
      first_party_runners,
      runner_providers,
    )?;
    Ok(Self {
      local_device,
      runs: Mutex::new(HashMap::new()),
      runner_affinities: Mutex::new(HashMap::new()),
      runner_affinity_locks: tokio::sync::Mutex::new(HashMap::new()),
      runners,
    })
  }

  pub fn list_devices(&self) -> proto::ListDevicesResponse {
    proto::ListDevicesResponse {
      devices: vec![self.local_device.clone()],
    }
  }

  pub fn get_device(&self, device_id: &str) -> Option<proto::Device> {
    self.local_device.r#ref.as_ref().is_some_and(|device| device.device_id == device_id).then(|| self.local_device.clone())
  }

  pub(crate) fn create_run(&self, caller: &CallerId, request: proto::CreateRunRequest) -> Result<proto::CreateRunResponse, DaemonError> {
    let mut runs = self.runs.lock().expect("Run registry lock poisoned");
    let devices = if request.devices.is_empty() {
      vec![self.local_device.r#ref.clone().expect("local Device always has a ref")]
    } else {
      let mut devices = Vec::with_capacity(request.devices.len());
      for device in request.devices {
        // TODO(distributed-run-authority): remote Device membership requires
        // the owner/participant contract in the accepted aggregated API design.
        if self.get_device(&device.device_id).is_none() {
          return Err(DaemonError::UnknownDevice(device.device_id));
        }
        if !devices.iter().any(|existing: &proto::DeviceRef| existing.device_id == device.device_id) {
          devices.push(device);
        }
      }
      devices
    };
    let run_id = crate::resource_id::generate_run().map_err(DaemonError::Identity)?;
    let run = proto::Run {
      r#ref: Some(proto::RunRef {
        run_id: run_id.clone(),
      }),
      phase: proto::RunPhase::Running as i32,
      devices,
      labels: request.labels,
      created_at: Some(timestamp_now()),
      completed_at: None,
    };
    runs.insert(
      run_id,
      OwnedRun {
        caller: caller.clone(),
        run: run.clone(),
      },
    );
    Ok(proto::CreateRunResponse { run: Some(run) })
  }

  pub async fn stop_run(&self, caller: &CallerId, run_id: &str, outcome: proto::RunOutcome) -> Result<proto::StopRunResponse, DaemonError> {
    let terminal_phase = match outcome {
      proto::RunOutcome::Succeeded => proto::RunPhase::Succeeded,
      proto::RunOutcome::Failed => proto::RunPhase::Failed,
      proto::RunOutcome::Canceled => proto::RunPhase::Canceled,
      proto::RunOutcome::Unspecified => return Err(DaemonError::InvalidArgument("terminal Run outcome is required")),
    };
    let run = {
      let mut runs = self.runs.lock().expect("Run registry lock poisoned");
      let owned = runs.get_mut(run_id).filter(|owned| &owned.caller == caller).ok_or_else(|| DaemonError::UnknownRun(run_id.to_string()))?;
      if owned.run.phase == proto::RunPhase::Running as i32 || owned.run.phase == proto::RunPhase::Pending as i32 {
        owned.run.phase = terminal_phase as i32;
        owned.run.completed_at = Some(timestamp_now());
      } else if owned.run.phase != terminal_phase as i32 {
        return Err(DaemonError::InvalidArgument("Run already stopped with a different outcome"));
      }
      owned.run.clone()
    };
    let runner_ids = {
      let mut affinities = self.runner_affinities.lock().expect("Runner affinity registry lock poisoned");
      let keys = affinities.keys().filter(|key| &key.caller == caller && key.run_id == run_id).cloned().collect::<Vec<_>>();
      keys.into_iter().filter_map(|key| affinities.remove(&key)).collect::<Vec<_>>()
    };
    self.runner_affinity_locks.lock().await.retain(|key, _| &key.caller != caller || key.run_id != run_id);
    for runner_id in runner_ids {
      self.runners.release_run_affinity(&runner_id).await?;
    }
    Ok(proto::StopRunResponse { run: Some(run) })
  }

  pub fn list_runs(&self, caller: &CallerId) -> proto::ListRunsResponse {
    let mut runs = self
      .runs
      .lock()
      .expect("Run registry lock poisoned")
      .values()
      .filter(|owned| &owned.caller == caller)
      .map(|owned| owned.run.clone())
      .collect::<Vec<_>>();
    runs.sort_by(|left, right| run_id(left).cmp(run_id(right)));
    proto::ListRunsResponse { runs }
  }

  pub(crate) fn get_run(&self, caller: &CallerId, run_id: &str) -> Result<proto::GetRunResponse, DaemonError> {
    self
      .runs
      .lock()
      .expect("Run registry lock poisoned")
      .get(run_id)
      .filter(|owned| &owned.caller == caller)
      .map(|owned| proto::GetRunResponse {
        run: Some(owned.run.clone()),
      })
      .ok_or_else(|| DaemonError::UnknownRun(run_id.to_string()))
  }

  pub fn list_runners(&self) -> proto::ListRunnersResponse {
    self.runners.list()
  }

  pub(crate) async fn create_runner(&self, request: proto::CreateRunnerRequest) -> Result<proto::CreateRunnerResponse, DaemonError> {
    let device_id = request
      .device
      .as_ref()
      .or(self.local_device.r#ref.as_ref())
      .map(|device| device.device_id.as_str())
      .expect("local Device always has a ref");
    if self.get_device(device_id).is_none() {
      return Err(DaemonError::UnknownDevice(device_id.to_string()));
    }
    if request.runner_class.as_ref().is_none_or(|runner_class| runner_class.runner_class.trim().is_empty()) {
      return Err(DaemonError::InvalidArgument("runner_class is required"));
    }
    self.runners.create(request, 0).await.map_err(Into::into)
  }

  pub(crate) fn get_runner(&self, runner_id: &str) -> Result<proto::GetRunnerResponse, DaemonError> {
    self.runners.get(runner_id).map_err(Into::into)
  }

  pub(crate) fn list_runner_classes(&self, device_id: Option<&str>) -> Result<proto::ListRunnerClassesResponse, DaemonError> {
    self.validate_local_device(device_id)?;
    Ok(self.runners.list_classes())
  }

  pub(crate) fn get_runner_class(&self, device_id: Option<&str>, runner_class: &str) -> Result<proto::GetRunnerClassResponse, DaemonError> {
    self.validate_local_device(device_id)?;
    self.runners.get_class(runner_class).map_err(Into::into)
  }

  pub async fn delete_runner(
    &self,
    runner_id: &str,
    grace_period: Option<prost_types::Duration>,
    force: bool,
  ) -> Result<proto::DeleteRunnerResponse, DaemonError> {
    self.runners.delete(runner_id, grace_period, force).await.map_err(Into::into)
  }

  pub(crate) async fn admit_routed_channel(
    &self,
    caller: &CallerId,
    route: RunnerRoute,
    service: &str,
    method: &str,
  ) -> Result<(Channel, OperationPermit), DaemonError> {
    if route.runner_class.trim().is_empty() {
      return Err(DaemonError::InvalidArgument("runner_class routing metadata is required"));
    }
    let device_id = route
      .device_id
      .as_deref()
      .or_else(|| self.local_device.r#ref.as_ref().map(|device| device.device_id.as_str()))
      .expect("local Device always has a ref")
      .to_string();
    self.validate_local_device(Some(&device_id))?;

    let affinity = match route.run_id.as_deref() {
      Some(run_id) => {
        let run = self.get_run(caller, run_id)?.run.expect("GetRun always returns a Run");
        if run.phase != proto::RunPhase::Running as i32 {
          return Err(DaemonError::InvalidArgument("capability routing requires a running Run"));
        }
        if !run.devices.iter().any(|device| device.device_id == device_id) {
          return Err(DaemonError::InvalidArgument("routed Device is not attached to the Run"));
        }
        Some(RunnerAffinityKey {
          caller: caller.clone(),
          run_id: run_id.to_string(),
          device_id: device_id.clone(),
          runner_class: route.runner_class.clone(),
        })
      }
      None => None,
    };

    // Only calls for the same Run/Device/RunnerClass serialize while a Runner
    // starts. An unhealthy provider must not block unrelated routing or Run
    // shutdown across the daemon.
    let affinity_lock = match &affinity {
      Some(key) => Some(
        self
          .runner_affinity_locks
          .lock()
          .await
          .entry(key.clone())
          .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
          .clone(),
      ),
      None => None,
    };
    let _affinity_resolution = match &affinity_lock {
      Some(lock) => Some(lock.lock().await),
      None => None,
    };
    let existing_affinity =
      affinity.as_ref().and_then(|key| self.runner_affinities.lock().expect("Runner affinity registry lock poisoned").get(key).cloned());
    let mut runner = existing_affinity
      .as_deref()
      .and_then(|runner_id| self.runners.get(runner_id).ok().and_then(|response| response.runner))
      .filter(|runner| runner.phase == proto::RunnerPhase::Ready as i32);
    if runner.is_none()
      && existing_affinity.is_some()
      && let Some(key) = &affinity
    {
      self.runner_affinities.lock().expect("Runner affinity registry lock poisoned").remove(key);
      if let Some(stale_runner_id) = existing_affinity.as_deref()
        && self.runners.get(stale_runner_id).is_ok()
      {
        self.runners.release_run_affinity(stale_runner_id).await?;
      }
    }
    let mut attach_to_run = false;
    let mut created_runner = false;
    if runner.is_none() {
      runner = self.runners.find_routable(&device_id, &route.runner_class);
      attach_to_run = runner.is_some() && affinity.is_some();
      if runner.is_none() {
        runner = self
          .runners
          .create(
            proto::CreateRunnerRequest {
              device: Some(proto::DeviceRef {
                device_id: device_id.clone(),
              }),
              runner_class: Some(proto::RunnerClassRef {
                runner_class: route.runner_class.clone(),
              }),
              labels: HashMap::new(),
              lifecycle: if affinity.is_some() {
                proto::RunnerLifecycle::UnlessShutdown as i32
              } else {
                proto::RunnerLifecycle::Ephemeral as i32
              },
              idle_timeout: None,
            },
            0,
          )
          .await?
          .runner;
        attach_to_run = affinity.is_some();
        created_runner = true;
      }
    }
    let runner_id = runner
      .as_ref()
      .map(runner_id)
      .filter(|id| !id.is_empty())
      .ok_or(DaemonError::RunnerOperation("resolved Runner omitted its canonical ID".to_string()))?;
    let mut attached_to_run = false;
    let admission = if let Some(key) = &affinity {
      // StopRun changes the Run phase under this same lock. Revalidate after
      // the potentially slow spawn, then publish the affinity and admit the
      // operation atomically with respect to a terminal transition.
      let runs = self.runs.lock().expect("Run registry lock poisoned");
      (|| {
        let run =
          runs.get(&key.run_id).filter(|owned| &owned.caller == caller).ok_or_else(|| DaemonError::UnknownRun(key.run_id.clone()))?;
        if run.run.phase != proto::RunPhase::Running as i32 {
          return Err(DaemonError::InvalidArgument("capability routing requires a running Run"));
        }
        if attach_to_run {
          self.runners.attach_run(runner_id)?;
          attached_to_run = true;
        }
        self.runner_affinities.lock().expect("Runner affinity registry lock poisoned").insert(key.clone(), runner_id.to_string());
        match self.runners.begin_external_operation(runner_id, service, method) {
          Ok(admission) => Ok(admission),
          Err(error) => {
            self.runner_affinities.lock().expect("Runner affinity registry lock poisoned").remove(key);
            Err(error.into())
          }
        }
      })()
    } else {
      self.runners.begin_external_operation(runner_id, service, method).map_err(Into::into)
    };
    let (channel, permit) = match admission {
      Ok(admission) => admission,
      Err(error) => {
        if attached_to_run {
          self.runners.release_run_affinity(runner_id).await?;
        }
        if created_runner {
          self.runners.delete(runner_id, None, true).await?;
        }
        return Err(error);
      }
    };
    Ok((channel, OperationPermit { _runner: permit }))
  }

  pub async fn shutdown(&self) {
    self.runners.shutdown().await;
  }

  pub fn has_live_runners(&self) -> bool {
    self.runners.has_live()
  }

  fn validate_local_device(&self, device_id: Option<&str>) -> Result<(), DaemonError> {
    if let Some(device_id) = device_id
      && self.get_device(device_id).is_none()
    {
      return Err(DaemonError::UnknownDevice(device_id.to_string()));
    }
    Ok(())
  }
}

impl From<runner::RunnerError> for DaemonError {
  fn from(error: runner::RunnerError) -> Self {
    match error {
      runner::RunnerError::InvalidArgument(message) => Self::InvalidArgument(message),
      runner::RunnerError::ProviderUnavailable(class) => Self::RunnerProviderUnavailable(class),
      runner::RunnerError::Unknown(id) => Self::UnknownRunner(id),
      error => Self::RunnerOperation(error.to_string()),
    }
  }
}

fn run_id(run: &proto::Run) -> &str {
  run.r#ref.as_ref().map(|run| run.run_id.as_str()).unwrap_or_default()
}

fn runner_id(runner: &proto::Runner) -> &str {
  runner.r#ref.as_ref().map(|runner| runner.runner_id.as_str()).unwrap_or_default()
}

fn timestamp_now() -> prost_types::Timestamp {
  let duration = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  prost_types::Timestamp {
    seconds: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
    nanos: i32::try_from(duration.subsec_nanos()).expect("nanoseconds fit i32"),
  }
}

fn load_or_create_local_device_id(path: &Path) -> Result<String, String> {
  match fs::read_to_string(path) {
    Ok(value) => validate_device_id(path, value.trim()),
    Err(error) if error.kind() == ErrorKind::NotFound => create_local_device_id(path),
    Err(error) => Err(format!("failed to read local Device ID {}: {error}", path.display())),
  }
}

fn create_local_device_id(path: &Path) -> Result<String, String> {
  let device_id = crate::resource_id::generate()?;
  match OpenOptions::new().write(true).create_new(true).open(path) {
    Ok(mut file) => {
      set_private_file(path)?;
      writeln!(file, "{device_id}").map_err(|error| format!("failed to write local Device ID {}: {error}", path.display()))?;
      file.sync_all().map_err(|error| format!("failed to sync local Device ID {}: {error}", path.display()))?;
      Ok(device_id)
    }
    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
      let value =
        fs::read_to_string(path).map_err(|error| format!("failed to read concurrently created Device ID {}: {error}", path.display()))?;
      validate_device_id(path, value.trim())
    }
    Err(error) => Err(format!("failed to create local Device ID {}: {error}", path.display())),
  }
}

fn validate_device_id(path: &Path, value: &str) -> Result<String, String> {
  // TODO(resource-id-migration): Accept the previously persisted
  // `device_<UUID>` shape until Device stores have an explicit migration.
  // New identities are always opaque 256-bit hexadecimal values.
  let legacy = value.strip_prefix("device_").is_some_and(|value| uuid::Uuid::parse_str(value).is_ok());
  (crate::resource_id::validate(value) || legacy)
    .then(|| value.to_string())
    .ok_or_else(|| format!("invalid local Device ID in {}", path.display()))
}

#[cfg(target_os = "linux")]
fn local_platform() -> i32 {
  proto::DevicePlatform::Linux as i32
}

#[cfg(target_os = "macos")]
fn local_platform() -> i32 {
  proto::DevicePlatform::Macos as i32
}

#[cfg(target_os = "windows")]
fn local_platform() -> i32 {
  proto::DevicePlatform::Windows as i32
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn local_platform() -> i32 {
  proto::DevicePlatform::Unspecified as i32
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    .map_err(|error| format!("failed to set private control-plane directory permissions on {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), String> {
  Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|error| format!("failed to set private local Device ID permissions on {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), String> {
  Ok(())
}
