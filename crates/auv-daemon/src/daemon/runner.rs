//! Daemon-owned process Runner supervision and private local gRPC routing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use tokio::process::Child;
use tokio::sync::{Notify, futures::Notified};
use tonic::transport::{Channel, Endpoint};

use super::runner_provider::{
  FirstPartyRunnerRuntimes, RegisteredRunnerProvider, RunnerProviderConfig, RunnerProviderRegistry, RunnerRuntime,
};

const RUNNER_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
// Tonic requires an HTTP origin even when the custom connector supplies local
// IPC. The connector discards this URI and adopts the inherited stream.
const LOCAL_IPC_ORIGIN: &str = "http://localhost";

pub(crate) struct RunnerSupervisor {
  providers: RunnerProviderRegistry,
  local_device: daemon_proto::DeviceRef,
  parent_context: Option<ParentContext>,
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
  activity_changed: Arc<Notify>,
}

struct ManagedRunner {
  record: daemon_proto::Runner,
  runtime: ManagedRunnerRuntime,
  channel: Channel,
  display_name: String,
  run_affinities: u64,
}

enum ManagedRunnerRuntime {
  Executable { child: Child },
  RemoteGrpc,
}

struct ReadyRunner {
  runtime: ManagedRunnerRuntime,
  channel: Channel,
  display_name: String,
  labels: HashMap<String, String>,
  process_id: u32,
}

pub(crate) struct OperationPermit {
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
  activity_changed: Arc<Notify>,
  runner_id: String,
}

impl Drop for OperationPermit {
  fn drop(&mut self) {
    let (stop_now, schedule) = match decrement_activity_locked(&self.runners, &self.runner_id, false) {
      Ok(transition) => transition,
      Err(RunnerError::Unknown(_)) => return,
      Err(_) => {
        debug_assert!(false, "admitted Runner operation accounting must remain balanced");
        return;
      }
    };
    self.activity_changed.notify_waiters();
    if let Some(managed) = stop_now {
      tokio::spawn(async move {
        let _ = stop_managed(managed).await;
      });
    }
    if let Some((deadline, timeout)) = schedule {
      schedule_idle_stop(self.runners.clone(), self.runner_id.clone(), deadline, timeout);
    }
  }
}

impl RunnerSupervisor {
  pub(crate) fn with_providers(
    local_device: daemon_proto::DeviceRef,
    parent_endpoint: Option<String>,
    parent_credential: Option<zeroize::Zeroizing<String>>,
    first_party: FirstPartyRunnerRuntimes,
    custom: Vec<RunnerProviderConfig>,
  ) -> Result<Self, String> {
    let parent_context = parent_endpoint
      .map(|daemon_endpoint| {
        serde_json::to_string(&serde_json::json!({
          "device_id": local_device.device_id.clone(),
          "daemon_endpoint": daemon_endpoint,
        }))
        .map(|context| ParentContext {
          context,
          credential: parent_credential,
        })
      })
      .transpose()
      .map_err(|error| format!("failed to encode Runner parent AUV_CONTEXT: {error}"))?;
    Ok(Self {
      providers: RunnerProviderRegistry::build_with_first_party(first_party.local_driver, custom).map_err(|error| error.to_string())?,
      local_device,
      parent_context,
      runners: Arc::new(Mutex::new(HashMap::new())),
      activity_changed: Arc::new(Notify::new()),
    })
  }

  pub(crate) async fn create(
    &self,
    request: daemon_proto::CreateRunnerRequest,
    initial_run_affinities: u64,
  ) -> Result<daemon_proto::CreateRunnerResponse, RunnerError> {
    let runner_class = request
      .runner_class
      .as_ref()
      .map(|runner_class| runner_class.runner_class.as_str())
      .filter(|runner_class| !runner_class.is_empty())
      .ok_or(RunnerError::InvalidArgument("runner_class is required"))?;
    let provider = self.providers.get(runner_class).cloned().ok_or_else(|| RunnerError::ProviderUnavailable(runner_class.to_string()))?;
    let lifecycle =
      daemon_proto::RunnerLifecycle::try_from(request.lifecycle).map_err(|_| RunnerError::InvalidArgument("runner lifecycle is unknown"))?;
    match lifecycle {
      daemon_proto::RunnerLifecycle::Ephemeral | daemon_proto::RunnerLifecycle::UnlessShutdown => {}
      daemon_proto::RunnerLifecycle::UnlessIdle => {
        validate_idle_timeout(request.idle_timeout.as_ref())?;
      }
      daemon_proto::RunnerLifecycle::Unspecified => return Err(RunnerError::InvalidArgument("runner lifecycle is required")),
    }
    let runner_id = crate::resource_id::generate().map_err(RunnerError::Start)?;
    let ready = match spawn_ready(&provider, self.parent_context.as_ref(), &runner_id).await {
      Ok(ready) => ready,
      Err(error) => {
        tracing::warn!(runner_id = %runner_id, runner_class, %error, "Runner failed to start");
        return Err(error);
      }
    };
    tracing::info!(runner_id = %runner_id, runner_class, process_id = ready.process_id, lifecycle = ?lifecycle, "Runner started");
    let mut labels = ready.labels.clone();
    labels.extend(request.labels);
    let record = daemon_proto::Runner {
      r#ref: Some(daemon_proto::RunnerRef {
        runner_id: runner_id.clone(),
      }),
      device: Some(self.local_device.clone()),
      runner_class: request.runner_class,
      labels,
      lifecycle: request.lifecycle,
      idle_timeout: request.idle_timeout,
      phase: daemon_proto::RunnerPhase::Ready as i32,
      created_at: Some(timestamp_from_system_time(std::time::SystemTime::now())),
      process_id: ready.process_id,
      active_operations: 0,
      idle_deadline: None,
    };
    let managed = ManagedRunner {
      record: record.clone(),
      runtime: ready.runtime,
      channel: ready.channel,
      display_name: ready.display_name,
      run_affinities: initial_run_affinities,
    };
    self.runners.lock().expect("Runner registry lock poisoned").insert(runner_id, managed);
    Ok(daemon_proto::CreateRunnerResponse {
      runner: Some(record),
    })
  }

  pub(crate) fn find_routable(&self, device_id: &str, runner_class: &str) -> Option<daemon_proto::Runner> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners.values().find_map(|managed| {
      let record = &managed.record;
      (record.phase == daemon_proto::RunnerPhase::Ready as i32
        && record.device.as_ref().is_some_and(|device| device.device_id == device_id)
        && record.runner_class.as_ref().is_some_and(|class| class.runner_class == runner_class))
      .then(|| record.clone())
    })
  }

  pub(crate) fn attach_run(&self, runner_id: &str) -> Result<(), RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
    managed.run_affinities = managed.run_affinities.saturating_add(1);
    managed.record.idle_deadline = None;
    Ok(())
  }

  pub(crate) async fn release_run_affinity(&self, runner_id: &str) -> Result<(), RunnerError> {
    self.decrement_activity(runner_id, true).await
  }

  pub(crate) fn begin_external_operation(
    &self,
    runner_id: &str,
    service: &str,
    method: &str,
  ) -> Result<(Channel, OperationPermit), RunnerError> {
    self.begin_operation_inner(runner_id, service, method)
  }

  fn begin_operation_inner(&self, runner_id: &str, service: &str, method: &str) -> Result<(Channel, OperationPermit), RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
    if managed.record.phase != daemon_proto::RunnerPhase::Ready as i32 {
      return Err(RunnerError::Call("Runner is not ready for operation admission".to_string()));
    }
    // Registration approves the complete endpoint. The daemon routes the
    // original gRPC path without parsing descriptors or maintaining a
    // per-service/method allowlist.
    let _ = (service, method);
    managed.record.active_operations = managed.record.active_operations.saturating_add(1);
    managed.record.idle_deadline = None;
    Ok((
      managed.channel.clone(),
      OperationPermit {
        runners: self.runners.clone(),
        activity_changed: self.activity_changed.clone(),
        runner_id: runner_id.to_string(),
      },
    ))
  }

  async fn decrement_activity(&self, runner_id: &str, run_affinity: bool) -> Result<(), RunnerError> {
    let (stop_now, schedule) = decrement_activity_locked(&self.runners, runner_id, run_affinity)?;
    self.activity_changed.notify_waiters();
    if let Some(managed) = stop_now {
      stop_managed(managed).await?;
    }
    if let Some((deadline, timeout)) = schedule {
      schedule_idle_stop(self.runners.clone(), runner_id.to_string(), deadline, timeout);
    }
    Ok(())
  }

  pub(crate) fn list_classes(&self) -> daemon_proto::ListRunnerClassesResponse {
    let runners = self.runners.lock().expect("Runner registry lock poisoned");
    daemon_proto::ListRunnerClassesResponse {
      runner_classes: self
        .providers
        .values()
        .map(|provider| runner_class_record(provider, self.local_device.clone(), runners.values()))
        .collect(),
    }
  }

  pub(crate) fn get_class(&self, runner_class: &str) -> Result<daemon_proto::GetRunnerClassResponse, RunnerError> {
    let provider = self.providers.get(runner_class).ok_or_else(|| RunnerError::ProviderUnavailable(runner_class.to_string()))?;
    let runners = self.runners.lock().expect("Runner registry lock poisoned");
    Ok(daemon_proto::GetRunnerClassResponse {
      runner_class: Some(runner_class_record(provider, self.local_device.clone(), runners.values())),
    })
  }

  pub(crate) fn list(&self) -> daemon_proto::ListRunnersResponse {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    let mut records = runners.values().map(|runner| runner.record.clone()).collect::<Vec<_>>();
    records.sort_by(|left, right| runner_id(left).cmp(runner_id(right)));
    daemon_proto::ListRunnersResponse { runners: records }
  }

  pub(crate) fn get(&self, runner_id: &str) -> Result<daemon_proto::GetRunnerResponse, RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners
      .get(runner_id)
      .map(|runner| daemon_proto::GetRunnerResponse {
        runner: Some(runner.record.clone()),
      })
      .ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))
  }

  pub(crate) fn has_live(&self) -> bool {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners.values().any(|runner| runner.record.phase == daemon_proto::RunnerPhase::Ready as i32)
  }

  pub(crate) async fn delete(
    &self,
    runner_id: &str,
    grace_period: Option<prost_types::Duration>,
    force: bool,
  ) -> Result<daemon_proto::DeleteRunnerResponse, RunnerError> {
    let grace_period = grace_period.map(validate_grace_period).transpose()?;
    // Claiming Draining marks this delete the single owner of the Runner's
    // deletion: a second delete, lifecycle auto-stop, and the idle stop all
    // leave Draining entries to that owner.
    let drain = {
      let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
      let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
      if managed.record.phase == daemon_proto::RunnerPhase::Draining as i32 {
        return Err(RunnerError::Deleting(runner_id.to_string()));
      }
      managed.record.phase = daemon_proto::RunnerPhase::Draining as i32;
      managed.run_affinities = 0;
      managed.record.idle_deadline = None;
      matches!(&managed.runtime, ManagedRunnerRuntime::RemoteGrpc) && !force
    };
    // Claim and spawn happen without an await between them, so a cancelled
    // caller can never strand a claimed-but-unowned Draining entry: the task
    // owns the deletion to completion once the claim is taken.
    let task = tokio::spawn(delete_runner_task(
      self.runners.clone(),
      self.activity_changed.clone(),
      runner_id.to_string(),
      grace_period.map(|grace| tokio::time::Instant::now() + grace),
      force,
      drain,
    ));
    // The task result is already the caller-facing Result; `?` only unwraps
    // the JoinHandle's join error.
    task.await.map_err(|error| RunnerError::Stop(format!("Runner deletion task failed: {error}")))?
  }

  pub(crate) async fn shutdown(&self) {
    let runners = std::mem::take(&mut *self.runners.lock().expect("Runner registry lock poisoned"));
    // Draining deletes watch registry membership, not the map's identity;
    // wake them so they observe the taken entry instead of waiting forever.
    self.activity_changed.notify_waiters();
    for (_, managed) in runners {
      let _ = stop_managed(managed).await;
    }
  }
}

/// Owns one Runner deletion to completion, detached from its caller.
///
/// The registry entry is the ownership token: only this task removes a
/// Draining entry, so a cancelled or racing caller can neither stop the same
/// Runner twice nor lose the registry's only handle to an executable child.
async fn delete_runner_task(
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
  activity_changed: Arc<Notify>,
  runner_id: String,
  deadline: Option<tokio::time::Instant>,
  force: bool,
  drain: bool,
) -> Result<daemon_proto::DeleteRunnerResponse, RunnerError> {
  if drain {
    loop {
      let mut changed = std::pin::pin!(activity_changed.notified());
      let already_notified = arm_notify_waiter(changed.as_mut()).await;
      let active_operations =
        runners.lock().expect("Runner registry lock poisoned").get(&runner_id).map(|managed| managed.record.active_operations);
      let Some(active_operations) = active_operations else {
        // The entry left the registry while draining; only shutdown() does
        // that, and it stops the Runner itself.
        return Err(RunnerError::Unknown(runner_id));
      };
      if active_operations == 0 {
        break;
      }
      if already_notified {
        continue;
      }
      match deadline {
        Some(deadline) => {
          if tokio::time::timeout_at(deadline, changed.as_mut()).await.is_err() {
            break;
          }
        }
        None => changed.await,
      }
    }
  }
  let mut managed =
    { runners.lock().expect("Runner registry lock poisoned").remove(&runner_id) }.ok_or_else(|| RunnerError::Unknown(runner_id.clone()))?;
  match stop_managed_in_place(&mut managed, deadline, force).await {
    Ok(()) => Ok(daemon_proto::DeleteRunnerResponse {
      runner: Some(managed.record),
    }),
    Err(error) => {
      record_stop_failure(&runners, runner_id, managed);
      Err(error)
    }
  }
}

/// Registers a `Notified` waiter before the caller re-checks its predicate.
///
/// `Notified` registers with the `Notify` on its first poll and
/// `notify_waiters` never stores permits, so a waiter that registers after the
/// notification fires never wakes. Polling once up front closes that window:
/// any later `notify_waiters` finds the waiter registered, and any state
/// change that fired before registration is already visible to the predicate
/// check that follows. Returns whether the waiter was already notified.
async fn arm_notify_waiter(mut waiter: std::pin::Pin<&mut Notified<'_>>) -> bool {
  use std::future::Future as _;
  std::future::poll_fn(|cx| match waiter.as_mut().poll(cx) {
    std::task::Poll::Ready(()) => std::task::Poll::Ready(true),
    std::task::Poll::Pending => std::task::Poll::Ready(false),
  })
  .await
}

/// Retains a Runner whose stop failed: the registry keeps tracking it (and an
/// executable runtime keeps owning its child) under the Failed phase so a
/// later DeleteRunner can retry, instead of leaking an unmanaged process.
fn record_stop_failure(runners: &Arc<Mutex<HashMap<String, ManagedRunner>>>, runner_id: String, mut managed: ManagedRunner) {
  managed.record.phase = daemon_proto::RunnerPhase::Failed as i32;
  tracing::warn!(runner_id = %runner_id, "Runner did not stop cleanly; retained as Failed for a retry");
  runners.lock().expect("Runner registry lock poisoned").insert(runner_id, managed);
}

fn runner_class_record<'a>(
  provider: &RegisteredRunnerProvider,
  device: daemon_proto::DeviceRef,
  runners: impl Iterator<Item = &'a ManagedRunner>,
) -> daemon_proto::RunnerClass {
  let observed = runners.filter(|runner| {
    runner.record.runner_class.as_ref().is_some_and(|class| class.runner_class == provider.runner_class)
      && runner.record.phase == daemon_proto::RunnerPhase::Ready as i32
  });
  let mut display_name = provider.runner_class.clone();
  for runner in observed {
    display_name = runner.display_name.clone();
  }
  daemon_proto::RunnerClass {
    r#ref: Some(daemon_proto::RunnerClassRef {
      runner_class: provider.runner_class.clone(),
    }),
    device: Some(device),
    display_name,
    supported_lifecycles: vec![
      daemon_proto::RunnerLifecycle::Ephemeral as i32,
      daemon_proto::RunnerLifecycle::UnlessIdle as i32,
      daemon_proto::RunnerLifecycle::UnlessShutdown as i32,
    ],
    available: true,
  }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RunnerError {
  #[error("invalid Runner argument: {0}")]
  InvalidArgument(&'static str),
  #[error("no RunnerProvider is registered for RunnerClass: {0}")]
  ProviderUnavailable(String),
  #[error("unknown Runner: {0}")]
  Unknown(String),
  #[error("Runner {0} is already being deleted")]
  Deleting(String),
  #[error("failed to start Runner: {0}")]
  Start(String),
  #[error("failed to stop Runner: {0}")]
  Stop(String),
  #[error("Runner call failed: {0}")]
  Call(String),
}

fn validate_idle_timeout(value: Option<&prost_types::Duration>) -> Result<Duration, RunnerError> {
  let value = value.ok_or(RunnerError::InvalidArgument("idle_timeout is required for unless-idle"))?;
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 || (value.seconds == 0 && value.nanos == 0) {
    return Err(RunnerError::InvalidArgument("idle_timeout must be positive"));
  }
  Ok(Duration::new(
    u64::try_from(value.seconds).map_err(|_| RunnerError::InvalidArgument("idle_timeout is too large"))?,
    u32::try_from(value.nanos).map_err(|_| RunnerError::InvalidArgument("idle_timeout is invalid"))?,
  ))
}

fn validate_grace_period(value: prost_types::Duration) -> Result<Duration, RunnerError> {
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(RunnerError::InvalidArgument("grace_period must be a non-negative protobuf duration"));
  }
  Ok(Duration::new(value.seconds as u64, value.nanos as u32))
}

type IdleTransition = (Option<ManagedRunner>, Option<(std::time::SystemTime, Duration)>);

fn decrement_activity_locked(
  runners: &Arc<Mutex<HashMap<String, ManagedRunner>>>,
  runner_id: &str,
  run_affinity: bool,
) -> Result<IdleTransition, RunnerError> {
  let mut runners = runners.lock().expect("Runner registry lock poisoned");
  let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
  if run_affinity {
    managed.run_affinities =
      managed.run_affinities.checked_sub(1).ok_or_else(|| RunnerError::Call("Runner affinity accounting underflow".to_string()))?;
  } else {
    managed.record.active_operations = managed
      .record
      .active_operations
      .checked_sub(1)
      .ok_or_else(|| RunnerError::Call("Runner operation accounting underflow".to_string()))?;
  }
  if managed.run_affinities != 0 || managed.record.active_operations != 0 {
    return Ok((None, None));
  }
  if managed.record.phase != daemon_proto::RunnerPhase::Ready as i32 {
    // A Draining Runner is owned by its in-flight delete; a Failed Runner is
    // retained stop-failure evidence. Lifecycle auto-stop only applies to
    // Ready Runners so it can never race the delete owner.
    return Ok((None, None));
  }
  match daemon_proto::RunnerLifecycle::try_from(managed.record.lifecycle).unwrap_or_default() {
    daemon_proto::RunnerLifecycle::Ephemeral => Ok((runners.remove(runner_id), None)),
    daemon_proto::RunnerLifecycle::UnlessIdle => {
      let timeout = validate_idle_timeout(managed.record.idle_timeout.as_ref())?;
      let deadline = std::time::SystemTime::now() + timeout;
      managed.record.idle_deadline = Some(timestamp_from_system_time(deadline));
      Ok((None, Some((deadline, timeout))))
    }
    _ => Ok((None, None)),
  }
}

fn schedule_idle_stop(
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
  runner_id: String,
  deadline: std::time::SystemTime,
  timeout: Duration,
) {
  tokio::spawn(async move {
    tokio::time::sleep(timeout).await;
    let managed = {
      let mut runners = runners.lock().expect("Runner registry lock poisoned");
      let should_stop = runners.get(&runner_id).is_some_and(|managed| {
        managed.record.phase == daemon_proto::RunnerPhase::Ready as i32
          && managed.run_affinities == 0
          && managed.record.active_operations == 0
          && managed.record.idle_deadline.as_ref() == Some(&timestamp_from_system_time(deadline))
      });
      should_stop.then(|| runners.remove(&runner_id)).flatten()
    };
    if let Some(managed) = managed {
      tracing::info!(runner_id = %runner_id, idle_seconds = timeout.as_secs(), "stopping idle Runner");
      if let Err(error) = stop_managed(managed).await {
        tracing::warn!(runner_id = %runner_id, %error, "idle Runner did not stop cleanly");
      }
    }
  });
}

async fn stop_managed(mut managed: ManagedRunner) -> Result<(), RunnerError> {
  stop_managed_in_place(&mut managed, None, false).await
}

async fn stop_managed_in_place(managed: &mut ManagedRunner, deadline: Option<tokio::time::Instant>, force: bool) -> Result<(), RunnerError> {
  managed.record.phase = daemon_proto::RunnerPhase::Draining as i32;
  let channel = std::mem::replace(&mut managed.channel, Channel::from_static("http://[::]:1").connect_lazy());
  match &mut managed.runtime {
    ManagedRunnerRuntime::RemoteGrpc => {
      // A RemoteGrpc runtime may be shared infrastructure. Deleting the local
      // Runner detaches its Channel but never drains or terminates the remote
      // endpoint itself.
      drop(channel);
    }
    ManagedRunnerRuntime::Executable { child } => {
      if force {
        drop(channel);
        terminate_child(child).await?;
      } else {
        drop(channel);
        match deadline {
          Some(deadline) => match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => return Err(RunnerError::Stop(error.to_string())),
            Err(_) => terminate_child(child).await?,
          },
          None => {
            child.wait().await.map_err(|error| RunnerError::Stop(error.to_string()))?;
          }
        }
      }
    }
  }
  managed.record.phase = daemon_proto::RunnerPhase::Stopped as i32;
  tracing::info!(runner_id = runner_id(&managed.record), force, "Runner stopped");
  Ok(())
}

async fn terminate_child(child: &mut Child) -> Result<(), RunnerError> {
  if child.try_wait().map_err(|error| RunnerError::Stop(error.to_string()))?.is_none() {
    child.kill().await.map_err(|error| RunnerError::Stop(error.to_string()))?;
  }
  child.wait().await.map_err(|error| RunnerError::Stop(error.to_string()))?;
  Ok(())
}

/// What a Runner needs to call its daemon back.
pub(crate) struct ParentContext {
  /// Non-secret `AUV_CONTEXT` JSON naming the daemon endpoint and Device.
  context: String,
  /// Launcher token for a launcher-policy daemon, sent over the Runner's
  /// stdin because other processes of the same user can read the environment.
  credential: Option<zeroize::Zeroizing<String>>,
}

fn configure_parent(command: &mut tokio::process::Command, parent_context: Option<&ParentContext>) {
  command.stdin(std::process::Stdio::null());
  let Some(parent_context) = parent_context else {
    return;
  };
  command.env("AUV_CONTEXT", &parent_context.context);
  if parent_context.credential.is_some() {
    command.env(auv::CREDENTIAL_STDIN_ENV, "1").stdin(std::process::Stdio::piped());
  }
}

/// A Runner that never reads its stdin leaves the line in the pipe buffer; one
/// that already exited fails the write, and its startup error surfaces later.
async fn send_parent_credential(child: &mut tokio::process::Child, parent_context: Option<&ParentContext>) {
  use tokio::io::AsyncWriteExt as _;
  let (Some(credential), Some(mut stdin)) = (parent_context.and_then(|parent| parent.credential.as_ref()), child.stdin.take()) else {
    return;
  };
  let _ = stdin.write_all(credential.as_bytes()).await;
  let _ = stdin.write_all(b"\n").await;
}

#[cfg(unix)]
async fn spawn_ready(
  provider: &RegisteredRunnerProvider,
  parent_context: Option<&ParentContext>,
  runner_id: &str,
) -> Result<ReadyRunner, RunnerError> {
  match &provider.runtime {
    RunnerRuntime::Executable(runtime) => spawn_executable_ready(provider, runtime, parent_context, runner_id).await,
    RunnerRuntime::RemoteGrpc(runtime) => connect_remote_ready(provider, runtime).await,
  }
}

#[cfg(unix)]
async fn spawn_executable_ready(
  provider: &RegisteredRunnerProvider,
  runtime: &super::runner_provider::ExecutableRunnerRuntime,
  parent_context: Option<&ParentContext>,
  runner_id: &str,
) -> Result<ReadyRunner, RunnerError> {
  use std::os::fd::AsRawFd;

  let (parent, child_stream) = std::os::unix::net::UnixStream::pair().map_err(|error| RunnerError::Start(error.to_string()))?;
  parent.set_nonblocking(true).map_err(|error| RunnerError::Start(error.to_string()))?;
  let inherited_fd = child_stream.as_raw_fd();
  let mut command = tokio::process::Command::new(&runtime.executable);
  command
    .args(&runtime.arguments)
    // Runner children inherit the daemon environment so platform launch
    // context such as XDG, Wayland, DBus, dynamic-loader, and GPU variables
    // remains available without rebuilding AUV for every new integration.
    .envs(&runtime.environment)
    // The daemon owns this value. Never let a stale context inherited by the
    // daemon or supplied by a provider manifest redirect child delegation.
    .env_remove("AUV_CONTEXT")
    .env(super::runner_provider::RUNNER_ID_ENV, runner_id)
    .env("AUV_RUNNER_IPC_FD", "3")
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::inherit());
  configure_parent(&mut command, parent_context);
  if let Some(working_directory) = &runtime.working_directory {
    command.current_dir(working_directory);
  }
  // SAFETY: this closure uses only async-signal-safe libc calls between fork
  // and exec, copies one already-open socket to a fixed descriptor, and
  // reports failures through io::Error.
  unsafe {
    command.pre_exec(move || {
      if inherited_fd != 3 && libc::dup2(inherited_fd, 3) == -1 {
        return Err(std::io::Error::last_os_error());
      }
      if libc::fcntl(3, libc::F_SETFD, 0) == -1 {
        return Err(std::io::Error::last_os_error());
      }
      Ok(())
    });
  }
  let mut child = command.spawn().map_err(|error| RunnerError::Start(error.to_string()))?;
  send_parent_credential(&mut child, parent_context).await;
  drop(child_stream);
  let stream = tokio::net::UnixStream::from_std(parent).map_err(|error| RunnerError::Start(error.to_string()))?;
  let once = std::sync::Arc::new(tokio::sync::Mutex::new(Some(stream)));
  let endpoint = Endpoint::from_static(LOCAL_IPC_ORIGIN);
  let connect = endpoint.connect_with_connector(tower::service_fn(move |_: http::Uri| {
    let once = once.clone();
    async move {
      once
        .lock()
        .await
        .take()
        .map(hyper_util::rt::TokioIo::new)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "Runner IPC stream already consumed"))
    }
  }));
  let channel = match connect.await {
    Ok(channel) => channel,
    Err(error) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(RunnerError::Start(error.to_string()));
    }
  };
  let reflected = match validate_ready(channel.clone(), provider).await {
    Ok(reflected) => reflected,
    Err(error) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(error);
    }
  };
  let process_id = child.id().ok_or_else(|| RunnerError::Start("Runner process omitted its PID".to_string()))?;
  Ok(ReadyRunner {
    runtime: ManagedRunnerRuntime::Executable { child },
    channel,
    display_name: reflected.display_name,
    labels: reflected.labels,
    process_id,
  })
}

#[cfg(windows)]
async fn spawn_ready(
  provider: &RegisteredRunnerProvider,
  parent_context: Option<&ParentContext>,
  runner_id: &str,
) -> Result<ReadyRunner, RunnerError> {
  match &provider.runtime {
    RunnerRuntime::Executable(runtime) => spawn_executable_ready(provider, runtime, parent_context, runner_id).await,
    RunnerRuntime::RemoteGrpc(runtime) => connect_remote_ready(provider, runtime).await,
  }
}

#[cfg(windows)]
async fn spawn_executable_ready(
  provider: &RegisteredRunnerProvider,
  runtime: &super::runner_provider::ExecutableRunnerRuntime,
  parent_context: Option<&ParentContext>,
  runner_id: &str,
) -> Result<ReadyRunner, RunnerError> {
  let pipe_name = format!(r"\\.\pipe\auv-runner-{}", uuid::Uuid::now_v7());
  let pipe = auv_api_server::windows_pipe::create_owner_only(&pipe_name, true)
    .map_err(|error| RunnerError::Start(format!("failed to create Runner named pipe: {error}")))?;
  let mut command = tokio::process::Command::new(&runtime.executable);
  command
    .args(&runtime.arguments)
    // Preserve the interactive desktop and application environment used by
    // the daemon host while keeping AUV-owned routing values authoritative.
    .envs(&runtime.environment)
    .env_remove("AUV_CONTEXT")
    .env(super::runner_provider::RUNNER_ID_ENV, runner_id)
    .env(auv_api_server::runner_transport::RUNNER_IPC_PIPE_ENV, &pipe_name)
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::inherit());
  configure_parent(&mut command, parent_context);
  if let Some(working_directory) = &runtime.working_directory {
    command.current_dir(working_directory);
  }
  let mut child = command.spawn().map_err(|error| RunnerError::Start(error.to_string()))?;
  send_parent_credential(&mut child, parent_context).await;
  let Some(runner_process_id) = child.id() else {
    let _ = terminate_child(&mut child).await;
    return Err(RunnerError::Start("Runner process omitted its PID".to_string()));
  };
  let connected = tokio::select! {
    result = accept_runner_client(&pipe_name, pipe, runner_process_id) => {
      result.map_err(|error| RunnerError::Start(format!("failed to connect Runner named pipe: {error}")))
    }
    result = child.wait() => {
      let status = result.map_err(|error| RunnerError::Start(error.to_string()))?;
      Err(RunnerError::Start(format!("Runner process exited before connecting to its named pipe: {status}")))
    }
  };
  let pipe = match connected {
    Ok(pipe) => pipe,
    Err(error) => {
      let _ = terminate_child(&mut child).await;
      return Err(error);
    }
  };
  let once = std::sync::Arc::new(tokio::sync::Mutex::new(Some(pipe)));
  let endpoint = Endpoint::from_static(LOCAL_IPC_ORIGIN);
  let connect = endpoint.connect_with_connector(tower::service_fn(move |_: http::Uri| {
    let once = once.clone();
    async move {
      once
        .lock()
        .await
        .take()
        .map(hyper_util::rt::TokioIo::new)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "Runner IPC stream already consumed"))
    }
  }));
  let channel = match connect.await {
    Ok(channel) => channel,
    Err(error) => {
      let _ = terminate_child(&mut child).await;
      return Err(RunnerError::Start(error.to_string()));
    }
  };
  let reflected = match validate_ready(channel.clone(), provider).await {
    Ok(reflected) => reflected,
    Err(error) => {
      let _ = terminate_child(&mut child).await;
      return Err(error);
    }
  };
  let process_id = child.id().ok_or_else(|| RunnerError::Start("Runner process omitted its PID".to_string()))?;
  Ok(ReadyRunner {
    runtime: ManagedRunnerRuntime::Executable { child },
    channel,
    display_name: reflected.display_name,
    labels: reflected.labels,
    process_id,
  })
}

/// Waits for the spawned Runner to open its pipe. The owner-only DACL keeps
/// other users out; a same-user process that connects first is dropped
/// instead of being adopted as the Runner.
#[cfg(windows)]
async fn accept_runner_client(
  path: &str,
  mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
  runner_process_id: u32,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
  loop {
    pipe.connect().await?;
    if auv_api_server::windows_pipe::client_process_id(&pipe)? == runner_process_id {
      return Ok(pipe);
    }
    // A fresh instance replaces the impostor's: a disconnected Tokio pipe
    // keeps the old client's I/O state. Creating it before dropping the old
    // one keeps the name from becoming free for another process to claim.
    pipe = auv_api_server::windows_pipe::create_owner_only(path, false)?;
  }
}

#[cfg(not(any(unix, windows)))]
async fn spawn_ready(
  provider: &RegisteredRunnerProvider,
  _parent_context: Option<&ParentContext>,
  _runner_id: &str,
) -> Result<ReadyRunner, RunnerError> {
  match &provider.runtime {
    RunnerRuntime::Executable(_) => Err(RunnerError::Start("executable Runner IPC is not supported on this platform".to_string())),
    RunnerRuntime::RemoteGrpc(runtime) => connect_remote_ready(provider, runtime).await,
  }
}

async fn connect_remote_ready(
  provider: &RegisteredRunnerProvider,
  runtime: &super::runner_provider::RemoteGrpcRunnerRuntime,
) -> Result<ReadyRunner, RunnerError> {
  let endpoint = Endpoint::from_shared(runtime.endpoint.clone()).map_err(|error| RunnerError::Start(error.to_string()))?;
  let channel = endpoint.connect().await.map_err(|error| RunnerError::Start(error.to_string()))?;
  let reflected = validate_ready(channel.clone(), provider).await?;
  Ok(ReadyRunner {
    runtime: ManagedRunnerRuntime::RemoteGrpc,
    channel,
    display_name: reflected.display_name,
    labels: reflected.labels,
    process_id: 0,
  })
}

struct ReflectedRuntime {
  display_name: String,
  labels: HashMap<String, String>,
}

async fn validate_ready(channel: Channel, provider: &RegisteredRunnerProvider) -> Result<ReflectedRuntime, RunnerError> {
  let mut health = tonic_health::pb::health_client::HealthClient::new(channel.clone());
  let response = tokio::time::timeout(
    RUNNER_HEALTH_CHECK_TIMEOUT,
    health.check(tonic_health::pb::HealthCheckRequest {
      service: String::new(),
    }),
  )
  .await
  .map_err(|_| RunnerError::Start(format!("Runner health check timed out after {}s", RUNNER_HEALTH_CHECK_TIMEOUT.as_secs())))?
  .map_err(|status| RunnerError::Start(format!("Runner health check failed: {status}")))?
  .into_inner();
  if response.status != tonic_health::pb::health_check_response::ServingStatus::Serving as i32 {
    return Err(RunnerError::Start("Runner endpoint is not serving".to_string()));
  }
  Ok(ReflectedRuntime {
    display_name: provider.runner_class.clone(),
    labels: Default::default(),
  })
}

fn refresh_exited(runners: &mut HashMap<String, ManagedRunner>) {
  for (runner_id, managed) in runners.iter_mut() {
    let exit_status = match &mut managed.runtime {
      ManagedRunnerRuntime::Executable { child } => child.try_wait().ok().flatten(),
      ManagedRunnerRuntime::RemoteGrpc => None,
    };
    let exited = match &mut managed.runtime {
      ManagedRunnerRuntime::Executable { .. } => exit_status.is_some(),
      // TODO(remote-runner-watch-status): consume WatchStatus and health
      // transitions once retry/backoff and failure evidence semantics are
      // approved. Ordinary RPCs already surface endpoint unavailability.
      ManagedRunnerRuntime::RemoteGrpc => false,
    };
    if managed.record.phase == daemon_proto::RunnerPhase::Ready as i32 && exited {
      // TODO(runner-restart-policy): crashed children are retained as FAILED
      // evidence and a later routed request may create a replacement.
      // Automatic restart/backoff is deferred until the owner approves
      // attempt limits, crash-loop visibility, and Run-affinity reassignment.
      managed.record.phase = daemon_proto::RunnerPhase::Failed as i32;
      tracing::warn!(
        runner_id = %runner_id,
        process_id = managed.record.process_id,
        status = ?exit_status,
        "Runner exited while ready; see its runner-<id> log"
      );
    }
  }
}

fn runner_id(runner: &daemon_proto::Runner) -> &str {
  runner.r#ref.as_ref().map(|runner| runner.runner_id.as_str()).unwrap_or_default()
}

fn timestamp_from_system_time(value: std::time::SystemTime) -> prost_types::Timestamp {
  let duration = value.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  prost_types::Timestamp {
    seconds: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
    nanos: i32::try_from(duration.subsec_nanos()).expect("nanoseconds fit i32"),
  }
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;
