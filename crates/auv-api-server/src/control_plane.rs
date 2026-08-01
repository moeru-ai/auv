//! Device, Run, and Runner control resources owned by one daemon.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::Mutex;

use auv_api_proto::auv::api::core::v1 as proto;
use tonic::transport::Channel;

use crate::authority::PrincipalId;
use crate::runner::RunnerSupervisor;

const LOCAL_DEVICE_ID_FILE: &str = "device-id";
const MAX_RUNS_PER_PRINCIPAL: usize = 1024;
const MAX_IN_MEMORY_RUNS: usize = 4096;
const MAX_LABELS: usize = 64;
const MAX_LABEL_BYTES: usize = 256;

/// Process-local view of durable control-plane identity and live resources.
pub struct ControlPlane {
  local_device: proto::Device,
  // TODO(distributed-run-authority): Runs are daemon-memory resources until a
  // coordinator/storage slice defines cross-daemon ownership, recovery, and
  // terminal append semantics in the accepted architecture document.
  runs: Mutex<HashMap<String, OwnedRun>>,
  runner_leases: std::sync::Arc<Mutex<HashMap<String, OwnedRunnerLease>>>,
  runners: RunnerSupervisor,
}

struct OwnedRun {
  principal: PrincipalId,
  run: proto::Run,
}

struct OwnedRunnerLease {
  principal: PrincipalId,
  lease: proto::RunnerLease,
  active_operations: u32,
}

pub(crate) struct OperationPermit {
  _runner: crate::runner::OperationPermit,
  leases: std::sync::Arc<Mutex<HashMap<String, OwnedRunnerLease>>>,
  lease_id: String,
}

impl Drop for OperationPermit {
  fn drop(&mut self) {
    if let Some(owned) = self.leases.lock().expect("Runner lease registry lock poisoned").get_mut(&self.lease_id) {
      owned.active_operations = owned.active_operations.checked_sub(1).expect("admitted lease operation count cannot underflow");
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum ControlPlaneError {
  #[error("invalid control-plane argument: {0}")]
  InvalidArgument(&'static str),
  #[error("unknown Device: {0}")]
  UnknownDevice(String),
  #[error("unknown Run: {0}")]
  UnknownRun(String),
  #[error("unknown Runner: {0}")]
  UnknownRunner(String),
  #[error("unknown Runner lease: {0}")]
  UnknownRunnerLease(String),
  #[error("no RunnerProvider is registered for RunnerClass: {0}")]
  RunnerProviderUnavailable(String),
  #[error("Runner capability is unavailable: {0}")]
  RunnerCapabilityUnavailable(String),
  #[error("principal already owns the maximum {0} in-memory Runs")]
  RunCapacityExhausted(usize),
  #[error("Runner operation failed: {0}")]
  RunnerOperation(String),
  #[error("Runner RPC failed: {0}")]
  RunnerRpcStatus(tonic::Status),
}

impl ControlPlane {
  pub fn open(store_root: &Path) -> Result<Self, String> {
    Self::open_with_runner_providers(store_root, crate::runner_provider::FirstPartyRunnerRuntimes::default(), Vec::new())
  }

  pub fn open_with_runner_providers(
    store_root: &Path,
    first_party_runners: crate::runner_provider::FirstPartyRunnerRuntimes,
    runner_providers: Vec<crate::runner_provider::RunnerProviderConfig>,
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
      first_party_runners,
      runner_providers,
    )?;
    Ok(Self {
      local_device,
      runs: Mutex::new(HashMap::new()),
      runner_leases: std::sync::Arc::new(Mutex::new(HashMap::new())),
      runners,
    })
  }

  pub fn list_devices(&self) -> proto::ListDevicesResponse {
    proto::ListDevicesResponse {
      devices: vec![self.local_device.clone()],
    }
  }

  pub fn list_services(&self) -> proto::ListServicesResponse {
    self.runners.list_services()
  }

  pub fn get_device(&self, device_id: &str) -> Option<proto::Device> {
    self.local_device.r#ref.as_ref().is_some_and(|device| device.device_id == device_id).then(|| self.local_device.clone())
  }

  pub fn create_run(
    &self,
    principal: &PrincipalId,
    request: proto::CreateRunRequest,
  ) -> Result<proto::CreateRunResponse, ControlPlaneError> {
    let mut runs = self.runs.lock().expect("Run registry lock poisoned");
    if runs.len() >= MAX_IN_MEMORY_RUNS {
      return Err(ControlPlaneError::RunCapacityExhausted(MAX_IN_MEMORY_RUNS));
    }
    if runs.values().filter(|owned| &owned.principal == principal).count() >= MAX_RUNS_PER_PRINCIPAL {
      return Err(ControlPlaneError::RunCapacityExhausted(MAX_RUNS_PER_PRINCIPAL));
    }
    validate_labels(&request.labels)?;
    let devices = if request.devices.is_empty() {
      vec![self.local_device.r#ref.clone().expect("local Device always has a ref")]
    } else {
      let mut devices = Vec::with_capacity(request.devices.len());
      for device in request.devices {
        // TODO(distributed-run-authority): remote Device membership requires
        // the owner/participant contract in the accepted aggregated API design.
        if self.get_device(&device.device_id).is_none() {
          return Err(ControlPlaneError::UnknownDevice(device.device_id));
        }
        if !devices.iter().any(|existing: &proto::DeviceRef| existing.device_id == device.device_id) {
          devices.push(device);
        }
      }
      devices
    };
    let run_id = format!("run_{}", uuid::Uuid::now_v7());
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
        principal: principal.clone(),
        run: run.clone(),
      },
    );
    Ok(proto::CreateRunResponse { run: Some(run) })
  }

  pub async fn stop_run(
    &self,
    principal: &PrincipalId,
    run_id: &str,
    outcome: proto::RunOutcome,
  ) -> Result<proto::StopRunResponse, ControlPlaneError> {
    let terminal_phase = match outcome {
      proto::RunOutcome::Succeeded => proto::RunPhase::Succeeded,
      proto::RunOutcome::Failed => proto::RunPhase::Failed,
      proto::RunOutcome::Canceled => proto::RunPhase::Canceled,
      proto::RunOutcome::Unspecified => return Err(ControlPlaneError::InvalidArgument("terminal Run outcome is required")),
    };
    let run = {
      let mut runs = self.runs.lock().expect("Run registry lock poisoned");
      let owned = runs
        .get_mut(run_id)
        .filter(|owned| &owned.principal == principal)
        .ok_or_else(|| ControlPlaneError::UnknownRun(run_id.to_string()))?;
      if owned.run.phase == proto::RunPhase::Running as i32 || owned.run.phase == proto::RunPhase::Pending as i32 {
        owned.run.phase = terminal_phase as i32;
        owned.run.completed_at = Some(timestamp_now());
      } else if owned.run.phase != terminal_phase as i32 {
        return Err(ControlPlaneError::InvalidArgument("Run already stopped with a different outcome"));
      }
      owned.run.clone()
    };
    let lease_ids = self
      .runner_leases
      .lock()
      .expect("Runner lease registry lock poisoned")
      .iter()
      .filter(|(_, owned)| &owned.principal == principal && lease_run_id(&owned.lease) == run_id)
      .map(|(id, _)| id.clone())
      .collect::<Vec<_>>();
    for lease_id in lease_ids {
      self.release_runner_lease_by_id(principal, &lease_id).await?;
    }
    Ok(proto::StopRunResponse { run: Some(run) })
  }

  pub fn list_runs(&self, principal: &PrincipalId) -> proto::ListRunsResponse {
    let mut runs = self
      .runs
      .lock()
      .expect("Run registry lock poisoned")
      .values()
      .filter(|owned| &owned.principal == principal)
      .map(|owned| owned.run.clone())
      .collect::<Vec<_>>();
    runs.sort_by(|left, right| run_id(left).cmp(run_id(right)));
    proto::ListRunsResponse { runs }
  }

  pub fn get_run(&self, principal: &PrincipalId, run_id: &str) -> Result<proto::GetRunResponse, ControlPlaneError> {
    self
      .runs
      .lock()
      .expect("Run registry lock poisoned")
      .get(run_id)
      .filter(|owned| &owned.principal == principal)
      .map(|owned| proto::GetRunResponse {
        run: Some(owned.run.clone()),
      })
      .ok_or_else(|| ControlPlaneError::UnknownRun(run_id.to_string()))
  }

  pub fn list_runners(&self) -> proto::ListRunnersResponse {
    self.runners.list()
  }

  pub async fn create_runner(&self, request: proto::CreateRunnerRequest) -> Result<proto::CreateRunnerResponse, ControlPlaneError> {
    let device_id = request
      .device
      .as_ref()
      .or(self.local_device.r#ref.as_ref())
      .map(|device| device.device_id.as_str())
      .expect("local Device always has a ref");
    if self.get_device(device_id).is_none() {
      return Err(ControlPlaneError::UnknownDevice(device_id.to_string()));
    }
    if request.runner_class.as_ref().is_none_or(|runner_class| runner_class.runner_class.trim().is_empty()) {
      return Err(ControlPlaneError::InvalidArgument("runner_class is required"));
    }
    validate_labels(&request.labels)?;
    self.runners.create(request, 0, 0).await.map_err(Into::into)
  }

  pub async fn claim_runner(
    &self,
    principal: &PrincipalId,
    request: proto::ClaimRunnerRequest,
  ) -> Result<proto::ClaimRunnerResponse, ControlPlaneError> {
    let mut claim = request.claim.ok_or(ControlPlaneError::InvalidArgument("claim is required"))?;
    let run_id = claim
      .run
      .as_ref()
      .map(|run| run.run_id.clone())
      .filter(|run| !run.is_empty())
      .ok_or(ControlPlaneError::InvalidArgument("claim.run is required"))?;
    let run = self.get_run(principal, &run_id)?.run.expect("GetRun always returns a Run");
    if run.phase != proto::RunPhase::Running as i32 {
      return Err(ControlPlaneError::InvalidArgument("Runner claims require a running Run"));
    }
    validate_labels(&claim.device_match_labels)?;
    let matching_devices = run
      .devices
      .iter()
      .filter(|device| {
        self.get_device(&device.device_id).is_some_and(|record| {
          claim.device.as_ref().is_none_or(|selected| selected.device_id == device.device_id)
            && claim.device_match_labels.iter().all(|(key, value)| record.labels.get(key) == Some(value))
        })
      })
      .cloned()
      .collect::<Vec<_>>();
    let device = match matching_devices.as_slice() {
      [device] => device.clone(),
      [] => return Err(ControlPlaneError::InvalidArgument("no Run Device satisfies the claim selector")),
      _ => return Err(ControlPlaneError::InvalidArgument("claim Device selector is ambiguous")),
    };
    if !run.devices.iter().any(|candidate| candidate.device_id == device.device_id) {
      return Err(ControlPlaneError::InvalidArgument("claim Device is not attached to the Run"));
    }
    claim.device = Some(device.clone());
    if claim.runner_class.as_ref().is_none_or(|class| class.runner_class.is_empty()) {
      claim.runner_class = Some(proto::RunnerClassRef {
        runner_class: "auv.core.local".to_string(),
      });
    }
    validate_labels(&claim.match_labels)?;
    let lifecycle = claim.lifecycle.unwrap_or(proto::RunnerLifecycle::UnlessShutdown as i32);
    claim.lifecycle = Some(lifecycle);
    let requested_capacity = claim.operation_capacity.max(1);
    self.runners.validate_claim(&claim, requested_capacity)?;
    let reuse_policy = proto::RunnerReusePolicy::try_from(claim.reuse_policy).unwrap_or_default();
    let existing = if reuse_policy == proto::RunnerReusePolicy::CreateNew {
      None
    } else {
      self.runners.find_compatible(&claim, requested_capacity)
    };
    let (runner, runner_created) = if let Some(runner) = existing {
      (runner, false)
    } else {
      if reuse_policy == proto::RunnerReusePolicy::RequireExisting {
        return Err(ControlPlaneError::RunnerCapabilityUnavailable("no compatible ready Runner satisfies the claim".to_string()));
      }
      self
        .runners
        .create(
          proto::CreateRunnerRequest {
            device: Some(device),
            runner_class: claim.runner_class.clone(),
            labels: claim.match_labels.clone(),
            lifecycle,
            idle_timeout: claim.idle_timeout.clone(),
          },
          1,
          requested_capacity,
        )
        .await?
        .runner
        .map(|runner| (runner, true))
        .expect("CreateRunner always returns a Runner")
    };
    let runner_id = runner.r#ref.as_ref().expect("Runner always has a ref").runner_id.clone();
    let lease_id = format!("lease_{}", uuid::Uuid::now_v7());
    let lease = proto::RunnerLease {
      r#ref: Some(proto::RunnerLeaseRef {
        run: claim.run,
        runner: Some(proto::RunnerRef {
          runner_id: runner_id.clone(),
        }),
        lease_id: lease_id.clone(),
      }),
      created_at: Some(timestamp_now()),
      operation_capacity: requested_capacity,
    };
    let bound = {
      let runs = self.runs.lock().expect("Run registry lock poisoned");
      let running =
        runs.get(&run_id).is_some_and(|owned| &owned.principal == principal && owned.run.phase == proto::RunPhase::Running as i32);
      if running {
        self.runner_leases.lock().expect("Runner lease registry lock poisoned").insert(
          lease_id,
          OwnedRunnerLease {
            principal: principal.clone(),
            lease: lease.clone(),
            active_operations: 0,
          },
        );
      }
      running
    };
    if !bound {
      self.runners.release_run_lease(&runner_id, requested_capacity).await?;
      return Err(ControlPlaneError::InvalidArgument("Run stopped while the Runner claim was being bound"));
    }
    Ok(proto::ClaimRunnerResponse {
      runner: Some(runner),
      lease: Some(lease),
      runner_created,
    })
  }

  pub async fn release_runner_lease(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
  ) -> Result<proto::ReleaseRunnerLeaseResponse, ControlPlaneError> {
    if lease.lease_id.is_empty() {
      return Err(ControlPlaneError::InvalidArgument("lease is required"));
    }
    {
      let leases = self.runner_leases.lock().expect("Runner lease registry lock poisoned");
      if let Some(owned) = leases.get(&lease.lease_id) {
        let expected = owned.lease.r#ref.as_ref().expect("stored Runner lease always has a ref");
        if &owned.principal != principal
          || lease.run.as_ref().is_some_and(|run| expected.run.as_ref() != Some(run))
          || lease.runner.as_ref().is_some_and(|runner| expected.runner.as_ref() != Some(runner))
        {
          return Err(ControlPlaneError::UnknownRunnerLease(lease.lease_id.clone()));
        }
      }
    }
    let released = self.release_runner_lease_by_id(principal, &lease.lease_id).await?;
    Ok(proto::ReleaseRunnerLeaseResponse { released })
  }

  async fn release_runner_lease_by_id(&self, principal: &PrincipalId, lease_id: &str) -> Result<bool, ControlPlaneError> {
    let owned = {
      let mut leases = self.runner_leases.lock().expect("Runner lease registry lock poisoned");
      match leases.get(lease_id) {
        Some(owned) if &owned.principal != principal => return Err(ControlPlaneError::UnknownRunnerLease(lease_id.to_string())),
        Some(_) => leases.remove(lease_id),
        None => None,
      }
    };
    let Some(owned) = owned else { return Ok(false) };
    let runner_id = lease_runner_id(&owned.lease).to_string();
    self.runners.release_run_lease(&runner_id, owned.lease.operation_capacity).await?;
    Ok(true)
  }

  pub fn get_runner(&self, runner_id: &str) -> Result<proto::GetRunnerResponse, ControlPlaneError> {
    self.runners.get(runner_id).map_err(Into::into)
  }

  pub fn list_runner_classes(&self, device_id: Option<&str>) -> Result<proto::ListRunnerClassesResponse, ControlPlaneError> {
    self.validate_local_device(device_id)?;
    Ok(self.runners.list_classes())
  }

  pub fn get_runner_class(&self, device_id: Option<&str>, runner_class: &str) -> Result<proto::GetRunnerClassResponse, ControlPlaneError> {
    self.validate_local_device(device_id)?;
    self.runners.get_class(runner_class).map_err(Into::into)
  }

  pub async fn delete_runner(&self, runner_id: &str) -> Result<proto::DeleteRunnerResponse, ControlPlaneError> {
    self.runner_leases.lock().expect("Runner lease registry lock poisoned").retain(|_, owned| lease_runner_id(&owned.lease) != runner_id);
    self.runners.delete(runner_id).await.map_err(Into::into)
  }

  pub async fn list_runner_displays(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ListDisplaysResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.DisplayService", "ListDisplays")?;
    let result = self.runners.list_displays(&runner_id).await.map_err(Into::into);
    result
  }

  pub async fn list_runner_windows(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ListWindowsResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.WindowService", "ListWindows")?;
    self.runners.list_windows(&runner_id).await.map_err(Into::into)
  }

  pub async fn resolve_runner_window(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    selector: auv_api_proto::auv::api::driver::v1::WindowSelector,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ResolveWindowResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.WindowService", "ResolveWindow")?;
    self.runners.resolve_window(&runner_id, selector).await.map_err(Into::into)
  }

  pub async fn capture_runner_window(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::CaptureWindowRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::CaptureWindowResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.CaptureService", "CaptureWindow")?;
    self.runners.capture_window(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn capture_runner_display(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    selector: Option<auv_api_proto::auv::api::driver::v1::DisplaySelector>,
  ) -> Result<auv_api_proto::auv::api::driver::v1::CaptureDisplayResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.CaptureService", "CaptureDisplay")?;
    self.runners.capture_display(&runner_id, selector).await.map_err(Into::into)
  }

  pub async fn capture_runner_region(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    region: auv_api_proto::auv::api::driver::v1::ScreenRect,
    selector: Option<auv_api_proto::auv::api::driver::v1::DisplaySelector>,
  ) -> Result<auv_api_proto::auv::api::driver::v1::CaptureRegionResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.CaptureService", "CaptureRegion")?;
    self.runners.capture_region(&runner_id, region, selector).await.map_err(Into::into)
  }

  pub async fn recognize_runner_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::RecognizeTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::RecognizeTextResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.TextRecognitionService", "RecognizeText")?;
    self.runners.recognize_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn detect_runner_objects(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::inference::v1::DetectObjectsRequest,
  ) -> Result<auv_api_proto::auv::api::inference::v1::DetectObjectsResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.inference.v1.ObjectDetectionService", "DetectObjects")?;
    self.runners.detect_objects(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn find_runner_window_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::FindWindowTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::FindWindowTextResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.v1.TextRecognitionService", "FindWindowText")?;
    self.runners.find_window_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn find_runner_display_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::FindDisplayTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::FindDisplayTextResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.v1.TextRecognitionService", "FindDisplayText")?;
    self.runners.find_display_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn click_runner_window_point(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::ClickWindowPointRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ClickWindowPointResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.InputService", "ClickWindowPoint")?;
    self.runners.click_window_point(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn click_runner_screen_point(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::ClickScreenPointRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ClickScreenPointResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.InputService", "ClickScreenPoint")?;
    self.runners.click_screen_point(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn type_runner_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::TypeTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::TypeTextResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.InputService", "TypeText")?;
    self.runners.type_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn paste_runner_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::PasteTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::PasteTextResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.InputService", "PasteText")?;
    self.runners.paste_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn press_runner_key(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::PressKeyRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::PressKeyResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.InputService", "PressKey")?;
    self.runners.press_key(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn probe_runner_permissions(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::ProbePermissionsRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::ProbePermissionsResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.PermissionService", "ProbePermissions")?;
    self.runners.probe_permissions(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn get_runner_now_playing(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::GetNowPlayingRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::GetNowPlayingResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "GetNowPlaying")?;
    self.runners.get_now_playing(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn focus_runner_text(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::FocusTextRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::FocusTextResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.AccessibilityService", "FocusText")?;
    self.runners.focus_text(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn play_runner_media(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::PlayRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::PlayResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "Play")?;
    self.runners.play_media(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn pause_runner_media(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::PauseRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::PauseResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "Pause")?;
    self.runners.pause_media(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn toggle_runner_media_play_pause(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::TogglePlayPauseRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::TogglePlayPauseResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "TogglePlayPause")?;
    self.runners.toggle_media_play_pause(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn next_runner_media_track(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::NextTrackRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::NextTrackResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "NextTrack")?;
    self.runners.next_media_track(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn previous_runner_media_track(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::PreviousTrackRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::PreviousTrackResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.MediaControlService", "PreviousTrack")?;
    self.runners.previous_media_track(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn show_runner_overlay(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::ShowOverlayRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::ShowOverlayResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.OverlayService", "ShowOverlay")?;
    self.runners.show_overlay(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn remove_runner_overlay(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::v1::RemoveOverlayRequest,
  ) -> Result<auv_api_proto::auv::api::driver::v1::RemoveOverlayResponse, ControlPlaneError> {
    let (runner_id, _permit) = self.admit_runner_operation(principal, lease, "auv.api.driver.v1.OverlayService", "RemoveOverlay")?;
    self.runners.remove_overlay(&runner_id, request).await.map_err(Into::into)
  }

  pub async fn activate_runner_bundle_id(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    request: auv_api_proto::auv::api::driver::macos::v1::ActivateBundleIdRequest,
  ) -> Result<auv_api_proto::auv::api::driver::macos::v1::ActivateBundleIdResponse, ControlPlaneError> {
    let (runner_id, _permit) =
      self.admit_runner_operation(principal, lease, "auv.api.driver.macos.v1.ApplicationService", "ActivateBundleId")?;
    self.runners.activate_bundle_id(&runner_id, request).await.map_err(Into::into)
  }

  fn admit_runner_operation(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    service: &str,
    method: &str,
  ) -> Result<(String, OperationPermit), ControlPlaneError> {
    let runner_id = self.admit_runner_lease_operation(principal, lease)?;
    let runner = match self.runners.begin_operation(&runner_id, service, method) {
      Ok(permit) => permit,
      Err(error) => {
        self.rollback_runner_lease_operation(&lease.lease_id);
        return Err(error.into());
      }
    };
    Ok((
      runner_id,
      OperationPermit {
        _runner: runner,
        leases: self.runner_leases.clone(),
        lease_id: lease.lease_id.clone(),
      },
    ))
  }

  fn admit_runner_lease_operation(&self, principal: &PrincipalId, lease: &proto::RunnerLeaseRef) -> Result<String, ControlPlaneError> {
    let mut leases = self.runner_leases.lock().expect("Runner lease registry lock poisoned");
    let owned = leases
      .get_mut(&lease.lease_id)
      .filter(|owned| &owned.principal == principal)
      .ok_or_else(|| ControlPlaneError::UnknownRunnerLease(lease.lease_id.clone()))?;
    let expected = owned.lease.r#ref.as_ref().expect("stored Runner lease always has a ref");
    if lease.run != expected.run || lease.runner != expected.runner {
      return Err(ControlPlaneError::UnknownRunnerLease(lease.lease_id.clone()));
    }
    if owned.active_operations >= owned.lease.operation_capacity {
      return Err(ControlPlaneError::RunnerOperation("Runner lease operation capacity is exhausted".to_string()));
    }
    owned.active_operations += 1;
    Ok(expected.runner.as_ref().expect("stored Runner lease always has a Runner").runner_id.clone())
  }

  fn rollback_runner_lease_operation(&self, lease_id: &str) {
    if let Some(owned) = self.runner_leases.lock().expect("Runner lease registry lock poisoned").get_mut(lease_id) {
      owned.active_operations = owned.active_operations.checked_sub(1).expect("rolled-back lease admission cannot underflow");
    }
  }

  pub(crate) fn admit_runner_channel(
    &self,
    principal: &PrincipalId,
    lease: &proto::RunnerLeaseRef,
    service: &str,
    method: &str,
  ) -> Result<(Channel, OperationPermit), ControlPlaneError> {
    let runner_id = self.admit_runner_lease_operation(principal, lease)?;
    match self.runners.begin_external_operation(&runner_id, service, method) {
      Ok((channel, runner)) => Ok((
        channel,
        OperationPermit {
          _runner: runner,
          leases: self.runner_leases.clone(),
          lease_id: lease.lease_id.clone(),
        },
      )),
      Err(error) => {
        self.rollback_runner_lease_operation(&lease.lease_id);
        Err(error.into())
      }
    }
  }

  pub async fn shutdown(&self) {
    self.runners.shutdown().await;
  }

  pub fn has_live_runners(&self) -> bool {
    self.runners.has_live()
  }

  fn validate_local_device(&self, device_id: Option<&str>) -> Result<(), ControlPlaneError> {
    if let Some(device_id) = device_id
      && self.get_device(device_id).is_none()
    {
      return Err(ControlPlaneError::UnknownDevice(device_id.to_string()));
    }
    Ok(())
  }
}

impl From<crate::runner::RunnerError> for ControlPlaneError {
  fn from(error: crate::runner::RunnerError) -> Self {
    match error {
      crate::runner::RunnerError::InvalidArgument(message) => Self::InvalidArgument(message),
      crate::runner::RunnerError::ProviderUnavailable(class) => Self::RunnerProviderUnavailable(class),
      crate::runner::RunnerError::Unknown(id) => Self::UnknownRunner(id),
      crate::runner::RunnerError::Unimplemented(capability) => Self::RunnerCapabilityUnavailable(capability),
      crate::runner::RunnerError::RpcStatus(status) => Self::RunnerRpcStatus(status),
      error => Self::RunnerOperation(error.to_string()),
    }
  }
}

fn lease_run_id(lease: &proto::RunnerLease) -> &str {
  lease.r#ref.as_ref().and_then(|lease| lease.run.as_ref()).map(|run| run.run_id.as_str()).unwrap_or_default()
}

fn lease_runner_id(lease: &proto::RunnerLease) -> &str {
  lease.r#ref.as_ref().and_then(|lease| lease.runner.as_ref()).map(|runner| runner.runner_id.as_str()).unwrap_or_default()
}

fn run_id(run: &proto::Run) -> &str {
  run.r#ref.as_ref().map(|run| run.run_id.as_str()).unwrap_or_default()
}

fn validate_labels(labels: &HashMap<String, String>) -> Result<(), ControlPlaneError> {
  if labels.len() > MAX_LABELS {
    return Err(ControlPlaneError::InvalidArgument("labels exceed the maximum entry count"));
  }
  if labels.iter().any(|(key, value)| key.len() > MAX_LABEL_BYTES || value.len() > MAX_LABEL_BYTES) {
    return Err(ControlPlaneError::InvalidArgument("label key or value exceeds the maximum byte length"));
  }
  Ok(())
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
  let device_id = format!("device_{}", uuid::Uuid::now_v7());
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
  let Some(uuid) = value.strip_prefix("device_") else {
    return Err(format!("invalid local Device ID in {}", path.display()));
  };
  uuid::Uuid::parse_str(uuid).map_err(|error| format!("invalid local Device UUID in {}: {error}", path.display()))?;
  Ok(value.to_string())
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
