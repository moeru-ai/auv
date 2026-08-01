//! Daemon-owned process Runner supervision and private Unix gRPC routing.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use auv_api_proto::auv::api::core::v1 as core_proto;
use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
use auv_api_proto::auv::api::driver::macos::v1::accessibility_service_client::AccessibilityServiceClient;
use auv_api_proto::auv::api::driver::macos::v1::application_service_client::ApplicationServiceClient;
use auv_api_proto::auv::api::driver::macos::v1::media_control_service_client::MediaControlServiceClient;
use auv_api_proto::auv::api::driver::macos::v1::permission_service_client::PermissionServiceClient;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::driver::v1::capture_service_client::CaptureServiceClient;
use auv_api_proto::auv::api::driver::v1::display_service_client::DisplayServiceClient;
use auv_api_proto::auv::api::driver::v1::input_service_client::InputServiceClient;
use auv_api_proto::auv::api::driver::v1::overlay_service_client::OverlayServiceClient;
use auv_api_proto::auv::api::driver::v1::text_recognition_service_client::TextRecognitionServiceClient;
use auv_api_proto::auv::api::driver::v1::window_service_client::WindowServiceClient;
use auv_api_proto::auv::api::inference::v1 as inference_proto;
use auv_api_proto::auv::api::inference::v1::object_detection_service_client::ObjectDetectionServiceClient;
use auv_api_proto::auv::api::runner::v1 as runtime_proto;
use auv_api_proto::auv::api::runner::v1::runner_runtime_service_client::RunnerRuntimeServiceClient;
use prost::Message;
use tokio::process::Child;
use tonic::transport::{Channel, Endpoint};

use crate::runner_provider::{
  FirstPartyRunnerRuntimes, RegisteredRunnerProvider, RunnerProviderConfig, RunnerProviderRegistry, RunnerRuntime,
};

#[cfg(test)]
const CAPTURE_SERVICE: &str = "auv.api.driver.v1.CaptureService";
#[cfg(test)]
const DISPLAY_SERVICE: &str = "auv.api.driver.v1.DisplayService";
#[cfg(test)]
const INPUT_SERVICE: &str = "auv.api.driver.v1.InputService";
#[cfg(test)]
const TEXT_RECOGNITION_SERVICE: &str = "auv.api.driver.v1.TextRecognitionService";
#[cfg(test)]
const WINDOW_SERVICE: &str = "auv.api.driver.v1.WindowService";
#[cfg(all(test, target_os = "macos"))]
const PERMISSION_SERVICE: &str = "auv.api.driver.macos.v1.PermissionService";
#[cfg(all(test, target_os = "macos"))]
const MEDIA_CONTROL_SERVICE: &str = "auv.api.driver.macos.v1.MediaControlService";
#[cfg(all(test, target_os = "macos"))]
const OVERLAY_SERVICE: &str = "auv.api.driver.v1.OverlayService";
#[cfg(all(test, target_os = "macos"))]
const APPLICATION_SERVICE: &str = "auv.api.driver.macos.v1.ApplicationService";
#[cfg(all(test, target_os = "macos"))]
const ACCESSIBILITY_SERVICE: &str = "auv.api.driver.macos.v1.AccessibilityService";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct RunnerSupervisor {
  providers: RunnerProviderRegistry,
  local_device: core_proto::DeviceRef,
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
  starting_remote_classes: Arc<Mutex<BTreeSet<String>>>,
}

struct ManagedRunner {
  record: core_proto::Runner,
  runtime: ManagedRunnerRuntime,
  channel: Channel,
  external_routes: BTreeSet<(String, String)>,
  reserved_operation_capacity: u32,
}

enum ManagedRunnerRuntime {
  Executable { child: Child },
  RemoteGrpc,
}

struct ReadyRunner {
  runtime: ManagedRunnerRuntime,
  channel: Channel,
  descriptor_set_sha256: Vec<u8>,
  process_id: u32,
}

pub(crate) struct OperationPermit {
  runners: Arc<Mutex<HashMap<String, ManagedRunner>>>,
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
  #[cfg(test)]
  pub(crate) fn new(local_device: core_proto::DeviceRef) -> Self {
    Self::with_providers(local_device, FirstPartyRunnerRuntimes::default(), Vec::new()).expect("empty RunnerProvider configuration is valid")
  }

  pub(crate) fn with_providers(
    local_device: core_proto::DeviceRef,
    first_party: FirstPartyRunnerRuntimes,
    custom: Vec<RunnerProviderConfig>,
  ) -> Result<Self, String> {
    Ok(Self {
      providers: RunnerProviderRegistry::build_with_first_party(first_party.local_driver, first_party.inference_ultralytics, custom)
        .map_err(|error| error.to_string())?,
      local_device,
      runners: Arc::new(Mutex::new(HashMap::new())),
      starting_remote_classes: Arc::new(Mutex::new(BTreeSet::new())),
    })
  }

  pub(crate) async fn create(
    &self,
    request: core_proto::CreateRunnerRequest,
    initial_run_leases: u32,
    initial_operation_capacity: u32,
  ) -> Result<core_proto::CreateRunnerResponse, RunnerError> {
    let runner_class = request
      .runner_class
      .as_ref()
      .map(|runner_class| runner_class.runner_class.as_str())
      .filter(|runner_class| !runner_class.is_empty())
      .ok_or(RunnerError::InvalidArgument("runner_class is required"))?;
    let provider = self.providers.get(runner_class).cloned().ok_or_else(|| RunnerError::ProviderUnavailable(runner_class.to_string()))?;
    let lifecycle = core_proto::RunnerLifecycle::try_from(request.lifecycle).unwrap_or_default();
    match lifecycle {
      core_proto::RunnerLifecycle::Ephemeral | core_proto::RunnerLifecycle::UnlessShutdown => {}
      core_proto::RunnerLifecycle::UnlessIdle => {
        validate_idle_timeout(request.idle_timeout.as_ref())?;
      }
      core_proto::RunnerLifecycle::Unspecified => return Err(RunnerError::InvalidArgument("runner lifecycle is required")),
    }
    if !provider.supported_lifecycles.contains(&request.lifecycle) {
      return Err(RunnerError::InvalidArgument("Runner lifecycle is not supported by the RunnerClass"));
    }
    if initial_run_leases == 0 && lifecycle != core_proto::RunnerLifecycle::UnlessShutdown {
      return Err(RunnerError::InvalidArgument("ephemeral and unless-idle Runners must be created through a Run claim"));
    }
    let runner_id = format!("runner_{}", uuid::Uuid::now_v7());
    let remote_class = matches!(provider.runtime, RunnerRuntime::RemoteGrpc(_)).then(|| provider.runner_class.clone());
    if let Some(remote_class) = &remote_class {
      let mut starting = self.starting_remote_classes.lock().expect("remote Runner start registry lock poisoned");
      let already_live = self.runners.lock().expect("Runner registry lock poisoned").values().any(|runner| {
        runner.record.runner_class.as_ref().is_some_and(|class| class.runner_class == *remote_class)
          && runner.record.phase != core_proto::RunnerPhase::Stopped as i32
      });
      if already_live || !starting.insert(remote_class.clone()) {
        return Err(RunnerError::Call("a remote gRPC Runner provider can back only one live Runner resource".to_string()));
      }
    }
    let ready = match spawn_ready(&provider).await {
      Ok(ready) => ready,
      Err(error) => {
        if let Some(remote_class) = &remote_class {
          self.starting_remote_classes.lock().expect("remote Runner start registry lock poisoned").remove(remote_class);
        }
        return Err(error);
      }
    };
    debug_assert_eq!(ready.descriptor_set_sha256.as_slice(), provider.validated_schema.descriptor_set_sha256);
    let record = core_proto::Runner {
      r#ref: Some(core_proto::RunnerRef {
        runner_id: runner_id.clone(),
      }),
      device: Some(self.local_device.clone()),
      runner_class: request.runner_class,
      labels: request.labels,
      lifecycle: request.lifecycle,
      idle_timeout: request.idle_timeout,
      phase: core_proto::RunnerPhase::Ready as i32,
      created_at: Some(timestamp_now()),
      capabilities: provider.capabilities.clone(),
      descriptor_set_sha256: ready.descriptor_set_sha256,
      process_id: ready.process_id,
      operation_capacity: provider.operation_capacity,
      active_run_leases: initial_run_leases,
      active_operations: 0,
      idle_deadline: None,
    };
    let managed = ManagedRunner {
      record: record.clone(),
      runtime: ready.runtime,
      channel: ready.channel,
      external_routes: provider.external_routes,
      reserved_operation_capacity: initial_operation_capacity,
    };
    if let Some(remote_class) = &remote_class {
      let mut starting = self.starting_remote_classes.lock().expect("remote Runner start registry lock poisoned");
      self.runners.lock().expect("Runner registry lock poisoned").insert(runner_id, managed);
      starting.remove(remote_class);
    } else {
      self.runners.lock().expect("Runner registry lock poisoned").insert(runner_id, managed);
    }
    Ok(core_proto::CreateRunnerResponse {
      runner: Some(record),
    })
  }

  pub(crate) fn find_compatible(&self, claim: &core_proto::RunnerClaim, requested_capacity: u32) -> Option<core_proto::Runner> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners.values_mut().find_map(|managed| {
      let record = &mut managed.record;
      let class_matches = claim
        .runner_class
        .as_ref()
        .is_none_or(|class| record.runner_class.as_ref().is_some_and(|candidate| candidate.runner_class == class.runner_class));
      let device_matches =
        claim.device.as_ref().is_none_or(|device| record.device.as_ref().is_some_and(|candidate| candidate.device_id == device.device_id));
      let labels_match = claim.match_labels.iter().all(|(key, value)| record.labels.get(key) == Some(value));
      let lifecycle_matches = claim.lifecycle.is_none_or(|lifecycle| record.lifecycle == lifecycle);
      let idle_timeout_matches = claim
        .lifecycle
        .is_none_or(|lifecycle| lifecycle != core_proto::RunnerLifecycle::UnlessIdle as i32 || record.idle_timeout == claim.idle_timeout);
      let has_capacity = managed.reserved_operation_capacity.saturating_add(requested_capacity) <= record.operation_capacity;
      let capabilities_match = claim.required_capabilities.iter().all(|required| {
        record
          .capabilities
          .iter()
          .any(|available| available.service == required.service && required.methods.iter().all(|method| available.methods.contains(method)))
      });
      (record.phase == core_proto::RunnerPhase::Ready as i32
        && class_matches
        && device_matches
        && labels_match
        && lifecycle_matches
        && idle_timeout_matches
        && has_capacity
        && capabilities_match)
        .then(|| {
          record.active_run_leases = record.active_run_leases.saturating_add(1);
          record.idle_deadline = None;
          managed.reserved_operation_capacity = managed.reserved_operation_capacity.saturating_add(requested_capacity);
          record.clone()
        })
    })
  }

  pub(crate) async fn release_run_lease(&self, runner_id: &str, operation_capacity: u32) -> Result<(), RunnerError> {
    {
      let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
      let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
      managed.reserved_operation_capacity = managed
        .reserved_operation_capacity
        .checked_sub(operation_capacity)
        .ok_or_else(|| RunnerError::Call("Runner reserved capacity accounting underflow".to_string()))?;
    }
    self.decrement_activity(runner_id, true).await
  }

  pub(crate) fn begin_operation(&self, runner_id: &str, service: &str, method: &str) -> Result<OperationPermit, RunnerError> {
    self.begin_operation_inner(runner_id, service, method, false).map(|(_, permit)| permit)
  }

  pub(crate) fn begin_external_operation(
    &self,
    runner_id: &str,
    service: &str,
    method: &str,
  ) -> Result<(Channel, OperationPermit), RunnerError> {
    self.begin_operation_inner(runner_id, service, method, true)
  }

  fn begin_operation_inner(
    &self,
    runner_id: &str,
    service: &str,
    method: &str,
    external: bool,
  ) -> Result<(Channel, OperationPermit), RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
    if managed.record.phase != core_proto::RunnerPhase::Ready as i32 {
      return Err(RunnerError::Call("Runner is not ready for operation admission".to_string()));
    }
    if !managed
      .record
      .capabilities
      .iter()
      .any(|capability| capability.service == service && capability.methods.iter().any(|candidate| candidate == method))
    {
      return Err(RunnerError::Unimplemented(format!("{service}/{method}")));
    }
    if external && !managed.external_routes.contains(&(service.to_string(), method.to_string())) {
      return Err(RunnerError::Unimplemented(format!("{service}/{method}")));
    }
    managed.record.active_operations = managed.record.active_operations.saturating_add(1);
    managed.record.idle_deadline = None;
    Ok((
      managed.channel.clone(),
      OperationPermit {
        runners: self.runners.clone(),
        runner_id: runner_id.to_string(),
      },
    ))
  }

  async fn decrement_activity(&self, runner_id: &str, lease: bool) -> Result<(), RunnerError> {
    let (stop_now, schedule) = decrement_activity_locked(&self.runners, runner_id, lease)?;
    if let Some(managed) = stop_now {
      stop_managed(managed).await?;
    }
    if let Some((deadline, timeout)) = schedule {
      schedule_idle_stop(self.runners.clone(), runner_id.to_string(), deadline, timeout);
    }
    Ok(())
  }

  pub(crate) fn list_classes(&self) -> core_proto::ListRunnerClassesResponse {
    core_proto::ListRunnerClassesResponse {
      runner_classes: self.providers.values().map(|provider| provider.runner_class_record(self.local_device.clone())).collect(),
    }
  }

  pub(crate) fn list_services(&self) -> core_proto::ListServicesResponse {
    core_proto::ListServicesResponse {
      services: self.providers.service_catalog(),
    }
  }

  pub(crate) fn get_class(&self, runner_class: &str) -> Result<core_proto::GetRunnerClassResponse, RunnerError> {
    let provider = self.providers.get(runner_class).ok_or_else(|| RunnerError::ProviderUnavailable(runner_class.to_string()))?;
    Ok(core_proto::GetRunnerClassResponse {
      runner_class: Some(provider.runner_class_record(self.local_device.clone())),
    })
  }

  pub(crate) fn validate_claim(&self, claim: &core_proto::RunnerClaim, requested_capacity: u32) -> Result<(), RunnerError> {
    let class = claim
      .runner_class
      .as_ref()
      .map(|class| class.runner_class.as_str())
      .filter(|class| !class.is_empty())
      .ok_or(RunnerError::InvalidArgument("runner_class is required"))?;
    let manifest = self.get_class(class)?.runner_class.expect("available RunnerClass has a record");
    let supported = claim.required_capabilities.iter().all(|required| {
      manifest
        .capabilities
        .iter()
        .any(|available| available.service == required.service && required.methods.iter().all(|method| available.methods.contains(method)))
    });
    if !supported {
      return Err(RunnerError::Unimplemented("claim requires a capability outside the RunnerClass manifest".to_string()));
    }
    if let Some(lifecycle) = claim.lifecycle
      && !manifest.supported_lifecycles.contains(&lifecycle)
    {
      return Err(RunnerError::InvalidArgument("Runner lifecycle is not supported by the RunnerClass"));
    }
    let operation_capacity = self.providers.get(class).expect("get_class established provider availability").operation_capacity;
    if requested_capacity > operation_capacity {
      return Err(RunnerError::InvalidArgument("operation_capacity exceeds the RunnerClass limit"));
    }
    Ok(())
  }

  pub(crate) fn list(&self) -> core_proto::ListRunnersResponse {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    let mut records = runners.values().map(|runner| runner.record.clone()).collect::<Vec<_>>();
    records.sort_by(|left, right| runner_id(left).cmp(runner_id(right)));
    core_proto::ListRunnersResponse { runners: records }
  }

  pub(crate) fn get(&self, runner_id: &str) -> Result<core_proto::GetRunnerResponse, RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners
      .get(runner_id)
      .map(|runner| core_proto::GetRunnerResponse {
        runner: Some(runner.record.clone()),
      })
      .ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))
  }

  pub(crate) fn has_live(&self) -> bool {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners.values().any(|runner| runner.record.phase == core_proto::RunnerPhase::Ready as i32)
  }

  pub(crate) async fn delete(&self, runner_id: &str) -> Result<core_proto::DeleteRunnerResponse, RunnerError> {
    {
      let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
      let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
      managed.record.phase = core_proto::RunnerPhase::Draining as i32;
      managed.record.active_run_leases = 0;
      managed.record.idle_deadline = None;
      managed.reserved_operation_capacity = 0;
    }
    let drain_deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    loop {
      let drained = self
        .runners
        .lock()
        .expect("Runner registry lock poisoned")
        .get(runner_id)
        .is_some_and(|managed| managed.record.active_operations == 0);
      if drained || tokio::time::Instant::now() >= drain_deadline {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut managed =
      self.runners.lock().expect("Runner registry lock poisoned").remove(runner_id).expect("draining Runner remains registered");
    stop_managed_in_place(&mut managed).await?;
    Ok(core_proto::DeleteRunnerResponse {
      runner: Some(managed.record),
    })
  }

  pub(crate) async fn list_displays(&self, runner_id: &str) -> Result<driver_proto::ListDisplaysResponse, RunnerError> {
    let channel = self.ready_channel(runner_id)?;
    DisplayServiceClient::new(channel)
      .list_displays(driver_proto::ListDisplaysRequest { lease: None })
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn list_windows(&self, runner_id: &str) -> Result<driver_proto::ListWindowsResponse, RunnerError> {
    WindowServiceClient::new(self.ready_channel(runner_id)?)
      .list_windows(driver_proto::ListWindowsRequest { lease: None })
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn resolve_window(
    &self,
    runner_id: &str,
    selector: driver_proto::WindowSelector,
  ) -> Result<driver_proto::ResolveWindowResponse, RunnerError> {
    WindowServiceClient::new(self.ready_channel(runner_id)?)
      .resolve_window(driver_proto::ResolveWindowRequest {
        lease: None,
        selector: Some(selector),
      })
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn capture_window(
    &self,
    runner_id: &str,
    mut request: driver_proto::CaptureWindowRequest,
  ) -> Result<driver_proto::CaptureWindowResponse, RunnerError> {
    request.lease = None;
    CaptureServiceClient::new(self.ready_channel(runner_id)?)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .capture_window(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn capture_display(
    &self,
    runner_id: &str,
    selector: Option<driver_proto::DisplaySelector>,
  ) -> Result<driver_proto::CaptureDisplayResponse, RunnerError> {
    CaptureServiceClient::new(self.ready_channel(runner_id)?)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .capture_display(driver_proto::CaptureDisplayRequest {
        lease: None,
        selector,
      })
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn capture_region(
    &self,
    runner_id: &str,
    region: driver_proto::ScreenRect,
    selector: Option<driver_proto::DisplaySelector>,
  ) -> Result<driver_proto::CaptureRegionResponse, RunnerError> {
    CaptureServiceClient::new(self.ready_channel(runner_id)?)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .capture_region(driver_proto::CaptureRegionRequest {
        lease: None,
        region: Some(region),
        selector,
      })
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn recognize_text(
    &self,
    runner_id: &str,
    mut request: driver_proto::RecognizeTextRequest,
  ) -> Result<driver_proto::RecognizeTextResponse, RunnerError> {
    request.lease = None;
    TextRecognitionServiceClient::new(self.ready_channel(runner_id)?)
      .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .recognize_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn detect_objects(
    &self,
    runner_id: &str,
    mut request: inference_proto::DetectObjectsRequest,
  ) -> Result<inference_proto::DetectObjectsResponse, RunnerError> {
    request.lease = None;
    ObjectDetectionServiceClient::new(self.ready_channel(runner_id)?)
      .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .detect_objects(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn find_window_text(
    &self,
    runner_id: &str,
    mut request: driver_proto::FindWindowTextRequest,
  ) -> Result<driver_proto::FindWindowTextResponse, RunnerError> {
    request.lease = None;
    TextRecognitionServiceClient::new(self.ready_channel(runner_id)?)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .find_window_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn find_display_text(
    &self,
    runner_id: &str,
    mut request: driver_proto::FindDisplayTextRequest,
  ) -> Result<driver_proto::FindDisplayTextResponse, RunnerError> {
    request.lease = None;
    TextRecognitionServiceClient::new(self.ready_channel(runner_id)?)
      .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
      .find_display_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn click_window_point(
    &self,
    runner_id: &str,
    mut request: driver_proto::ClickWindowPointRequest,
  ) -> Result<driver_proto::ClickWindowPointResponse, RunnerError> {
    request.lease = None;
    InputServiceClient::new(self.ready_channel(runner_id)?)
      .click_window_point(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn click_screen_point(
    &self,
    runner_id: &str,
    mut request: driver_proto::ClickScreenPointRequest,
  ) -> Result<driver_proto::ClickScreenPointResponse, RunnerError> {
    request.lease = None;
    InputServiceClient::new(self.ready_channel(runner_id)?)
      .click_screen_point(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn type_text(
    &self,
    runner_id: &str,
    mut request: driver_proto::TypeTextRequest,
  ) -> Result<driver_proto::TypeTextResponse, RunnerError> {
    request.lease = None;
    InputServiceClient::new(self.ready_channel(runner_id)?)
      .type_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn paste_text(
    &self,
    runner_id: &str,
    mut request: driver_proto::PasteTextRequest,
  ) -> Result<driver_proto::PasteTextResponse, RunnerError> {
    request.lease = None;
    InputServiceClient::new(self.ready_channel(runner_id)?)
      .paste_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn press_key(
    &self,
    runner_id: &str,
    mut request: driver_proto::PressKeyRequest,
  ) -> Result<driver_proto::PressKeyResponse, RunnerError> {
    request.lease = None;
    InputServiceClient::new(self.ready_channel(runner_id)?)
      .press_key(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn probe_permissions(
    &self,
    runner_id: &str,
    mut request: macos_proto::ProbePermissionsRequest,
  ) -> Result<macos_proto::ProbePermissionsResponse, RunnerError> {
    request.lease = None;
    PermissionServiceClient::new(self.ready_channel(runner_id)?)
      .probe_permissions(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn get_now_playing(
    &self,
    runner_id: &str,
    mut request: macos_proto::GetNowPlayingRequest,
  ) -> Result<macos_proto::GetNowPlayingResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .get_now_playing(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn play_media(
    &self,
    runner_id: &str,
    mut request: macos_proto::PlayRequest,
  ) -> Result<macos_proto::PlayResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .play(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn pause_media(
    &self,
    runner_id: &str,
    mut request: macos_proto::PauseRequest,
  ) -> Result<macos_proto::PauseResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .pause(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn toggle_media_play_pause(
    &self,
    runner_id: &str,
    mut request: macos_proto::TogglePlayPauseRequest,
  ) -> Result<macos_proto::TogglePlayPauseResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .toggle_play_pause(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn next_media_track(
    &self,
    runner_id: &str,
    mut request: macos_proto::NextTrackRequest,
  ) -> Result<macos_proto::NextTrackResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .next_track(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn previous_media_track(
    &self,
    runner_id: &str,
    mut request: macos_proto::PreviousTrackRequest,
  ) -> Result<macos_proto::PreviousTrackResponse, RunnerError> {
    request.lease = None;
    MediaControlServiceClient::new(self.ready_channel(runner_id)?)
      .previous_track(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn show_overlay(
    &self,
    runner_id: &str,
    mut request: driver_proto::ShowOverlayRequest,
  ) -> Result<driver_proto::ShowOverlayResponse, RunnerError> {
    request.lease = None;
    OverlayServiceClient::new(self.ready_channel(runner_id)?)
      .show_overlay(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn remove_overlay(
    &self,
    runner_id: &str,
    mut request: driver_proto::RemoveOverlayRequest,
  ) -> Result<driver_proto::RemoveOverlayResponse, RunnerError> {
    request.lease = None;
    OverlayServiceClient::new(self.ready_channel(runner_id)?)
      .remove_overlay(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn activate_bundle_id(
    &self,
    runner_id: &str,
    mut request: macos_proto::ActivateBundleIdRequest,
  ) -> Result<macos_proto::ActivateBundleIdResponse, RunnerError> {
    request.lease = None;
    ApplicationServiceClient::new(self.ready_channel(runner_id)?)
      .activate_bundle_id(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  pub(crate) async fn focus_text(
    &self,
    runner_id: &str,
    mut request: macos_proto::FocusTextRequest,
  ) -> Result<macos_proto::FocusTextResponse, RunnerError> {
    request.lease = None;
    AccessibilityServiceClient::new(self.ready_channel(runner_id)?)
      .focus_text(request)
      .await
      .map(tonic::Response::into_inner)
      .map_err(runner_call_status)
  }

  fn ready_channel(&self, runner_id: &str) -> Result<Channel, RunnerError> {
    let mut runners = self.runners.lock().expect("Runner registry lock poisoned");
    refresh_exited(&mut runners);
    runners
      .get(runner_id)
      .filter(|runner| runner.record.phase == core_proto::RunnerPhase::Ready as i32)
      .map(|runner| runner.channel.clone())
      .ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))
  }

  pub(crate) async fn shutdown(&self) {
    let runners = std::mem::take(&mut *self.runners.lock().expect("Runner registry lock poisoned"));
    for (_, managed) in runners {
      let _ = stop_managed(managed).await;
    }
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
  #[error("failed to start Runner: {0}")]
  Start(String),
  #[error("failed to stop Runner: {0}")]
  Stop(String),
  #[error("Runner call failed: {0}")]
  Call(String),
  #[error("Runner RPC failed: {0}")]
  RpcStatus(tonic::Status),
  #[error("Runner capability is not implemented: {0}")]
  Unimplemented(String),
}

fn runner_call_status(status: tonic::Status) -> RunnerError {
  RunnerError::RpcStatus(status)
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

type IdleTransition = (Option<ManagedRunner>, Option<(std::time::SystemTime, Duration)>);

fn decrement_activity_locked(
  runners: &Arc<Mutex<HashMap<String, ManagedRunner>>>,
  runner_id: &str,
  lease: bool,
) -> Result<IdleTransition, RunnerError> {
  let mut runners = runners.lock().expect("Runner registry lock poisoned");
  let managed = runners.get_mut(runner_id).ok_or_else(|| RunnerError::Unknown(runner_id.to_string()))?;
  if lease {
    managed.record.active_run_leases =
      managed.record.active_run_leases.checked_sub(1).ok_or_else(|| RunnerError::Call("Runner lease accounting underflow".to_string()))?;
  } else {
    managed.record.active_operations = managed
      .record
      .active_operations
      .checked_sub(1)
      .ok_or_else(|| RunnerError::Call("Runner operation accounting underflow".to_string()))?;
  }
  if managed.record.active_run_leases != 0 || managed.record.active_operations != 0 {
    return Ok((None, None));
  }
  match core_proto::RunnerLifecycle::try_from(managed.record.lifecycle).unwrap_or_default() {
    core_proto::RunnerLifecycle::Ephemeral => Ok((runners.remove(runner_id), None)),
    core_proto::RunnerLifecycle::UnlessIdle => {
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
        managed.record.active_run_leases == 0
          && managed.record.active_operations == 0
          && managed.record.idle_deadline.as_ref() == Some(&timestamp_from_system_time(deadline))
      });
      should_stop.then(|| runners.remove(&runner_id)).flatten()
    };
    if let Some(managed) = managed {
      let _ = stop_managed(managed).await;
    }
  });
}

async fn stop_managed(mut managed: ManagedRunner) -> Result<(), RunnerError> {
  stop_managed_in_place(&mut managed).await
}

async fn stop_managed_in_place(managed: &mut ManagedRunner) -> Result<(), RunnerError> {
  managed.record.phase = core_proto::RunnerPhase::Draining as i32;
  let channel = std::mem::replace(&mut managed.channel, Channel::from_static("http://[::]:1").connect_lazy());
  drop(channel);
  if let ManagedRunnerRuntime::Executable { child } = &mut managed.runtime
    && tokio::time::timeout(STOP_TIMEOUT, child.wait()).await.is_err()
  {
    child.kill().await.map_err(|error| RunnerError::Stop(error.to_string()))?;
    child.wait().await.map_err(|error| RunnerError::Stop(error.to_string()))?;
  }
  managed.record.phase = core_proto::RunnerPhase::Stopped as i32;
  Ok(())
}

#[cfg(unix)]
async fn spawn_ready(provider: &RegisteredRunnerProvider) -> Result<ReadyRunner, RunnerError> {
  match &provider.runtime {
    RunnerRuntime::Executable(runtime) => spawn_executable_ready(provider, runtime).await,
    RunnerRuntime::RemoteGrpc(runtime) => connect_remote_ready(provider, runtime).await,
  }
}

#[cfg(unix)]
async fn spawn_executable_ready(
  provider: &RegisteredRunnerProvider,
  runtime: &crate::runner_provider::ExecutableRunnerRuntime,
) -> Result<ReadyRunner, RunnerError> {
  use std::os::fd::AsRawFd;

  let (parent, child_stream) = std::os::unix::net::UnixStream::pair().map_err(|error| RunnerError::Start(error.to_string()))?;
  parent.set_nonblocking(true).map_err(|error| RunnerError::Start(error.to_string()))?;
  let inherited_fd = child_stream.as_raw_fd();
  let mut command = tokio::process::Command::new(&runtime.executable);
  command
    .args(&runtime.arguments)
    // Runner executables are operator-trusted in this version, but they still
    // do not inherit daemon credentials or unrelated process configuration.
    // TODO(runner-sandbox-v1): community/untrusted Runner executables require
    // a separate uid or platform sandbox and child-principal isolation before
    // they can be treated as a security boundary; private IPC alone is not a
    // sandbox for another process running as the daemon user.
    .env_clear()
    .env("AUV_RUNNER_IPC_FD", "3")
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::inherit());
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
  drop(child_stream);
  let stream = tokio::net::UnixStream::from_std(parent).map_err(|error| RunnerError::Start(error.to_string()))?;
  let once = std::sync::Arc::new(tokio::sync::Mutex::new(Some(stream)));
  let endpoint = Endpoint::from_static("http://[::]:50051");
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
  let channel = match tokio::time::timeout(STARTUP_TIMEOUT, connect).await {
    Ok(Ok(channel)) => channel,
    Ok(Err(error)) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(RunnerError::Start(error.to_string()));
    }
    Err(_) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(RunnerError::Start("readiness deadline exceeded".to_string()));
    }
  };
  let descriptor_hash = match tokio::time::timeout(STARTUP_TIMEOUT, validate_ready(channel.clone(), provider)).await {
    Ok(Ok(descriptor_hash)) => descriptor_hash,
    Ok(Err(error)) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(error);
    }
    Err(_) => {
      let _ = child.kill().await;
      let _ = child.wait().await;
      return Err(RunnerError::Start("Runner health/reflection readiness deadline exceeded".to_string()));
    }
  };
  let process_id = child.id().ok_or_else(|| RunnerError::Start("Runner process omitted its PID".to_string()))?;
  Ok(ReadyRunner {
    runtime: ManagedRunnerRuntime::Executable { child },
    channel,
    descriptor_set_sha256: descriptor_hash,
    process_id,
  })
}

#[cfg(not(unix))]
async fn spawn_ready(provider: &RegisteredRunnerProvider) -> Result<ReadyRunner, RunnerError> {
  match &provider.runtime {
    RunnerRuntime::Executable(_) => Err(RunnerError::Start("inherited-stream Runner IPC requires Unix".to_string())),
    RunnerRuntime::RemoteGrpc(runtime) => connect_remote_ready(provider, runtime).await,
  }
}

async fn connect_remote_ready(
  provider: &RegisteredRunnerProvider,
  runtime: &crate::runner_provider::RemoteGrpcRunnerRuntime,
) -> Result<ReadyRunner, RunnerError> {
  let endpoint = Endpoint::from_shared(runtime.endpoint.clone()).map_err(|error| RunnerError::Start(error.to_string()))?;
  let channel = tokio::time::timeout(STARTUP_TIMEOUT, endpoint.connect())
    .await
    .map_err(|_| RunnerError::Start("remote gRPC Runner connection deadline exceeded".to_string()))?
    .map_err(|error| RunnerError::Start(error.to_string()))?;
  let descriptor_set_sha256 = tokio::time::timeout(STARTUP_TIMEOUT, validate_ready(channel.clone(), provider))
    .await
    .map_err(|_| RunnerError::Start("remote gRPC Runner readiness deadline exceeded".to_string()))??;
  Ok(ReadyRunner {
    runtime: ManagedRunnerRuntime::RemoteGrpc,
    channel,
    descriptor_set_sha256,
    process_id: 0,
  })
}

async fn validate_ready(channel: Channel, provider: &RegisteredRunnerProvider) -> Result<Vec<u8>, RunnerError> {
  let mut health = tonic_health::pb::health_client::HealthClient::new(channel.clone());
  for service in
    provider.manifest.services.iter().map(|service| service.name.as_str()).chain(std::iter::once(auv_runner_protocol::RUNTIME_SERVICE_NAME))
  {
    let response = health
      .check(tonic_health::pb::HealthCheckRequest {
        service: service.to_string(),
      })
      .await
      .map_err(|status| RunnerError::Start(format!("health check failed for {service}: {status}")))?
      .into_inner();
    if response.status != tonic_health::pb::health_check_response::ServingStatus::Serving as i32 {
      return Err(RunnerError::Start(format!("{service} is not serving")));
    }
  }

  let mut runtime = RunnerRuntimeServiceClient::new(channel.clone());
  let metadata = runtime
    .get_metadata(runtime_proto::GetMetadataRequest {})
    .await
    .map_err(|status| RunnerError::Start(format!("Runner runtime metadata failed: {status}")))?
    .into_inner();
  if metadata.runner_class != provider.runner_class
    || metadata.display_name != provider.display_name
    || metadata.operation_capacity != provider.operation_capacity
  {
    return Err(RunnerError::Start("Runner runtime metadata differs from its daemon-owned RunnerClass policy".to_string()));
  }
  let status = runtime
    .get_status(runtime_proto::GetStatusRequest {})
    .await
    .map_err(|status| RunnerError::Start(format!("Runner runtime status failed: {status}")))?
    .into_inner()
    .status
    .ok_or_else(|| RunnerError::Start("Runner runtime omitted its status".to_string()))?;
  if status.phase != runtime_proto::RunnerRuntimePhase::Ready as i32 {
    return Err(RunnerError::Start("Runner runtime is not ready".to_string()));
  }
  let operations = status.operations.ok_or_else(|| RunnerError::Start("Runner runtime omitted operation status".to_string()))?;
  if operations.capacity != provider.operation_capacity || operations.active > operations.capacity {
    return Err(RunnerError::Start("Runner runtime operation status is inconsistent with its RunnerClass policy".to_string()));
  }

  let mut reflection = tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient::new(channel);
  use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
  let business_services = provider.manifest.services.iter().map(|service| service.name.clone()).collect::<Vec<_>>();
  let requests = std::iter::once(tonic_reflection::pb::v1::ServerReflectionRequest {
    host: String::new(),
    message_request: Some(MessageRequest::ListServices(String::new())),
  })
  .chain(business_services.iter().cloned().map(|service| tonic_reflection::pb::v1::ServerReflectionRequest {
    host: String::new(),
    message_request: Some(MessageRequest::FileContainingSymbol(service)),
  }))
  .collect::<Vec<_>>();
  let mut responses = reflection
    .server_reflection_info(tokio_stream::iter(requests))
    .await
    .map_err(|status| RunnerError::Start(format!("reflection failed: {status}")))?
    .into_inner();
  use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
  let first = responses.message().await.map_err(|status| RunnerError::Start(format!("reflection response failed: {status}")))?;
  let services = match first.and_then(|response| response.message_response) {
    Some(MessageResponse::ListServicesResponse(response)) => response.service,
    _ => return Err(RunnerError::Start("reflection omitted its service list".to_string())),
  };
  let service_names = services.into_iter().map(|service| service.name).collect::<Vec<_>>();
  if !service_names.iter().any(|service| service == auv_runner_protocol::RUNTIME_SERVICE_NAME) {
    return Err(RunnerError::Start("Runner reflection omitted RunnerRuntimeService".to_string()));
  }
  let mut descriptors = Vec::new();
  for service in &business_services {
    let response = responses.message().await.map_err(|status| RunnerError::Start(format!("descriptor response failed: {status}")))?;
    match response.and_then(|response| response.message_response) {
      Some(MessageResponse::FileDescriptorResponse(response)) => descriptors.extend(response.file_descriptor_proto),
      _ => return Err(RunnerError::Start(format!("reflection omitted the {service} descriptor closure"))),
    }
  }
  drop(responses);
  fetch_descriptor_dependencies(&mut reflection, &mut descriptors).await?;
  validate_manifest(&service_names, &descriptors, &provider.manifest)
}

async fn fetch_descriptor_dependencies(
  reflection: &mut tonic_reflection::pb::v1::server_reflection_client::ServerReflectionClient<Channel>,
  descriptors: &mut Vec<Vec<u8>>,
) -> Result<(), RunnerError> {
  use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
  use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;

  loop {
    let mut present = BTreeSet::new();
    let mut required = BTreeSet::new();
    for bytes in descriptors.iter() {
      let descriptor = prost_types::FileDescriptorProto::decode(bytes.as_slice())
        .map_err(|error| RunnerError::Start(format!("reflection returned an invalid descriptor: {error}")))?;
      let name = descriptor
        .name
        .filter(|name| !name.is_empty())
        .ok_or_else(|| RunnerError::Start("reflection descriptor omitted filename".to_string()))?;
      present.insert(name);
      required.extend(descriptor.dependency);
    }
    let missing = required.difference(&present).cloned().collect::<Vec<_>>();
    if missing.is_empty() {
      return Ok(());
    }
    if present.len().saturating_add(missing.len()) > crate::runner_provider::SchemaLimits::default().max_files {
      return Err(RunnerError::Start("reflection descriptor closure exceeds the file limit".to_string()));
    }

    let requests = missing
      .iter()
      .cloned()
      .map(|filename| tonic_reflection::pb::v1::ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::FileByFilename(filename)),
      })
      .collect::<Vec<_>>();
    let mut responses = reflection
      .server_reflection_info(tokio_stream::iter(requests))
      .await
      .map_err(|status| RunnerError::Start(format!("reflection dependency lookup failed: {status}")))?
      .into_inner();
    for filename in missing {
      let response =
        responses.message().await.map_err(|status| RunnerError::Start(format!("reflection dependency response failed: {status}")))?;
      match response.and_then(|response| response.message_response) {
        Some(MessageResponse::FileDescriptorResponse(response)) if !response.file_descriptor_proto.is_empty() => {
          descriptors.extend(response.file_descriptor_proto);
        }
        Some(MessageResponse::ErrorResponse(error)) => {
          return Err(RunnerError::Start(format!(
            "reflection could not provide dependency {filename}: code={} message={}",
            error.error_code, error.error_message
          )));
        }
        _ => return Err(RunnerError::Start(format!("reflection omitted dependency descriptor {filename}"))),
      }
    }
  }
}

fn validate_manifest(
  service_names: &[String],
  descriptors: &[Vec<u8>],
  manifest: &crate::runner_provider::TrustedManifest,
) -> Result<Vec<u8>, RunnerError> {
  let mut business_services = service_names
    .iter()
    .filter(|service| {
      !matches!(service.as_str(), "grpc.health.v1.Health" | "grpc.reflection.v1.ServerReflection")
        && service.as_str() != auv_runner_protocol::RUNTIME_SERVICE_NAME
    })
    .map(String::as_str)
    .collect::<Vec<_>>();
  business_services.sort_unstable();
  let mut expected_services = manifest.services.iter().map(|service| service.name.as_str()).collect::<Vec<_>>();
  expected_services.sort_unstable();
  if business_services != expected_services {
    return Err(RunnerError::Start(format!("Runner service manifest differs from its RunnerClass: {business_services:?}")));
  }
  if descriptors.is_empty() {
    return Err(RunnerError::Start("reflection returned an empty descriptor closure".to_string()));
  }

  // Reflection can return the same dependency while resolving several
  // services. Deduplicate byte-identical files, but reject conflicting files
  // with the same protobuf name before constructing the trusted closure.
  let mut unique_descriptors = BTreeMap::new();
  for descriptor in descriptors {
    let file = prost_types::FileDescriptorProto::decode(descriptor.as_slice())
      .map_err(|error| RunnerError::Start(format!("reflection returned an invalid file descriptor: {error}")))?;
    let name = file
      .name
      .filter(|name| !name.is_empty())
      .ok_or_else(|| RunnerError::Start("reflection returned a descriptor without a file name".to_string()))?;
    if let Some(existing) = unique_descriptors.insert(name.clone(), descriptor.clone())
      && existing != *descriptor
    {
      return Err(RunnerError::Start(format!("reflection returned conflicting descriptors for {name}")));
    }
  }
  let descriptors = unique_descriptors.into_values().collect::<Vec<_>>();
  let reflected = crate::runner_provider::encode_descriptor_set_files(&descriptors);
  let validated = crate::runner_provider::validate_encoded(&reflected, manifest, crate::runner_provider::SchemaLimits::default())
    .map_err(|error| RunnerError::Start(format!("Runner schema differs from its trusted RunnerClass: {error}")))?;
  Ok(validated.descriptor_set_sha256.to_vec())
}

fn refresh_exited(runners: &mut HashMap<String, ManagedRunner>) {
  for managed in runners.values_mut() {
    let exited = match &mut managed.runtime {
      ManagedRunnerRuntime::Executable { child } => child.try_wait().ok().flatten().is_some(),
      // TODO(remote-runner-watch-status): consume WatchStatus and health
      // transitions once retry/backoff and failure evidence semantics are
      // approved. Ordinary RPCs already surface endpoint unavailability.
      ManagedRunnerRuntime::RemoteGrpc => false,
    };
    if managed.record.phase == core_proto::RunnerPhase::Ready as i32 && exited {
      // TODO(runner-restart-policy): crashed children are retained as FAILED
      // evidence and a later claim may create a replacement. Automatic
      // restart/backoff is deferred until the owner approves attempt limits,
      // crash-loop visibility, and lease reassignment semantics.
      managed.record.phase = core_proto::RunnerPhase::Failed as i32;
    }
  }
}

fn runner_id(runner: &core_proto::Runner) -> &str {
  runner.r#ref.as_ref().map(|runner| runner.runner_id.as_str()).unwrap_or_default()
}

fn timestamp_now() -> prost_types::Timestamp {
  timestamp_from_system_time(std::time::SystemTime::now())
}

fn timestamp_from_system_time(value: std::time::SystemTime) -> prost_types::Timestamp {
  let duration = value.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  prost_types::Timestamp {
    seconds: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
    nanos: i32::try_from(duration.subsec_nanos()).expect("nanoseconds fit i32"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Default)]
  struct RemoteInferenceFixture;

  #[tonic::async_trait]
  impl inference_proto::object_detection_service_server::ObjectDetectionService for RemoteInferenceFixture {
    async fn detect_objects(
      &self,
      _request: tonic::Request<inference_proto::DetectObjectsRequest>,
    ) -> Result<tonic::Response<inference_proto::DetectObjectsResponse>, tonic::Status> {
      tokio::time::sleep(Duration::from_millis(100)).await;
      Ok(tonic::Response::new(inference_proto::DetectObjectsResponse::default()))
    }
  }

  #[tokio::test]
  async fn remote_grpc_runtime_connects_without_owning_the_endpoint_process() {
    use inference_proto::object_detection_service_server::ObjectDetectionServiceServer;
    use tokio_stream::wrappers::TcpListenerStream;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind remote Runner fixture");
    let address = listener.local_addr().expect("remote Runner fixture address");
    let runtime = auv_runner_protocol::RuntimeControl::ready(auv_runner_protocol::RuntimeMetadata {
      runner_class: "auv.inference.ultralytics".to_string(),
      display_name: "AUV Ultralytics inference".to_string(),
      labels: Default::default(),
      operation_capacity: 1,
    })
    .expect("runtime control");
    let runtime_service = runtime.service();
    let inference = ObjectDetectionServiceServer::new(RemoteInferenceFixture);
    let (health_reporter, health) = tonic_health::server::health_reporter();
    health_reporter.set_serving::<ObjectDetectionServiceServer<RemoteInferenceFixture>>().await;
    health_reporter
      .set_serving::<runtime_proto::runner_runtime_service_server::RunnerRuntimeServiceServer<auv_runner_protocol::RuntimeControl>>()
      .await;
    let descriptor = auv_runner_protocol::RuntimeControl::descriptor_set_for_services(&["auv.api.inference.v1.ObjectDetectionService"])
      .expect("remote Runner descriptor");
    let reflection = auv_runner_protocol::reflection_service(&descriptor).expect("remote Runner reflection");
    let server = tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(health)
        .add_service(reflection)
        .add_service(runtime_service)
        .add_service(runtime.track(inference))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
    });

    let executable = std::env::current_exe().expect("test executable");
    let registry =
      RunnerProviderRegistry::build_with_first_party(None, Some(crate::runner_provider::executable_runtime(executable)), Vec::new())
        .expect("inference manifest");
    let mut provider = registry.get("auv.inference.ultralytics").expect("inference provider").clone();
    provider.runtime = RunnerRuntime::RemoteGrpc(crate::runner_provider::RemoteGrpcRunnerRuntime {
      endpoint: format!("http://{address}"),
    });

    let ready = spawn_ready(&provider).await.expect("connect remote Runner");
    assert_eq!(ready.process_id, 0);
    let operation_channel = ready.channel.clone();
    let operation = tokio::spawn(async move {
      ObjectDetectionServiceClient::new(operation_channel).detect_objects(inference_proto::DetectObjectsRequest::default()).await
    });
    let mut status_client = RunnerRuntimeServiceClient::new(ready.channel.clone());
    let mut observed_active = false;
    for _ in 0..20 {
      let status = status_client
        .get_status(runtime_proto::GetStatusRequest {})
        .await
        .expect("runtime status during operation")
        .into_inner()
        .status
        .expect("runtime status");
      if status.operations.is_some_and(|operations| operations.active == 1) {
        observed_active = true;
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(observed_active, "tracked business RPC must be visible as active runtime work");
    operation.await.expect("join business RPC").expect("business RPC");
    let mut managed = ManagedRunner {
      record: core_proto::Runner::default(),
      runtime: ready.runtime,
      channel: ready.channel,
      external_routes: Default::default(),
      reserved_operation_capacity: 0,
    };
    stop_managed_in_place(&mut managed).await.expect("detach remote Runner");

    let mut client =
      RunnerRuntimeServiceClient::connect(format!("http://{address}")).await.expect("remote endpoint remains reachable after detach");
    assert!(client.get_status(runtime_proto::GetStatusRequest {}).await.is_ok());
    server.abort();
  }

  fn local_services() -> Vec<&'static str> {
    let mut services = vec![
      CAPTURE_SERVICE,
      DISPLAY_SERVICE,
      INPUT_SERVICE,
      TEXT_RECOGNITION_SERVICE,
      WINDOW_SERVICE,
    ];
    #[cfg(target_os = "macos")]
    services.push(MEDIA_CONTROL_SERVICE);
    #[cfg(target_os = "macos")]
    services.push(OVERLAY_SERVICE);
    #[cfg(target_os = "macos")]
    services.push(APPLICATION_SERVICE);
    #[cfg(target_os = "macos")]
    services.push(ACCESSIBILITY_SERVICE);
    #[cfg(target_os = "macos")]
    services.push(PERMISSION_SERVICE);
    services
  }

  fn descriptor_closure() -> Vec<Vec<u8>> {
    let encoded = auv_api_proto::descriptor_set_for_services(&local_services()).expect("generated local Runner descriptor closure");
    let pool = prost_reflect::DescriptorPool::decode(encoded.as_slice()).expect("generated descriptor set is valid");
    pool.files().map(|descriptor| descriptor.encode_to_vec()).collect()
  }

  fn local_manifest() -> crate::runner_provider::TrustedManifest {
    let services = local_services();
    let encoded = auv_api_proto::descriptor_set_for_services(&services).expect("generated local Runner descriptor closure");
    crate::runner_provider::manifest_from_trusted_descriptors(
      &encoded,
      &services.iter().map(|service| (*service, true)).collect::<Vec<_>>(),
      None,
    )
    .expect("local manifest")
  }

  #[test]
  fn runner_manifest_accepts_only_the_declared_typed_services() {
    let services = vec![
      "grpc.health.v1.Health".to_string(),
      "grpc.reflection.v1.ServerReflection".to_string(),
      DISPLAY_SERVICE.to_string(),
      INPUT_SERVICE.to_string(),
      TEXT_RECOGNITION_SERVICE.to_string(),
      WINDOW_SERVICE.to_string(),
      "auv.api.driver.v1.CaptureService".to_string(),
      #[cfg(target_os = "macos")]
      MEDIA_CONTROL_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      OVERLAY_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      APPLICATION_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      PERMISSION_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      ACCESSIBILITY_SERVICE.to_string(),
    ];

    validate_manifest(&services, &descriptor_closure(), &local_manifest())
      .expect("generated driver services match the RunnerClass manifest");
  }

  #[test]
  fn runner_manifest_deduplicates_identical_reflection_dependencies() {
    let services = vec![
      CAPTURE_SERVICE.to_string(),
      DISPLAY_SERVICE.to_string(),
      INPUT_SERVICE.to_string(),
      TEXT_RECOGNITION_SERVICE.to_string(),
      WINDOW_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      MEDIA_CONTROL_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      OVERLAY_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      APPLICATION_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      PERMISSION_SERVICE.to_string(),
      #[cfg(target_os = "macos")]
      ACCESSIBILITY_SERVICE.to_string(),
    ];
    let mut descriptors = descriptor_closure();
    descriptors.push(descriptors.first().expect("descriptor closure is non-empty").clone());

    validate_manifest(&services, &descriptors, &local_manifest())
      .expect("reflection may repeat one byte-identical dependency across service lookups");
  }

  #[test]
  fn local_runner_capabilities_publish_each_input_method() {
    let manifest = local_manifest();
    let input = manifest.services.iter().find(|service| service.name == INPUT_SERVICE).expect("local Runner publishes InputService");
    assert_eq!(
      input.methods.iter().map(|method| method.name.as_str()).collect::<Vec<_>>(),
      [
        "ClickWindowPoint",
        "ClickScreenPoint",
        "TypeText",
        "PasteText",
        "PressKey"
      ]
    );
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn local_runner_manifest_publishes_exact_permission_capability() {
    let manifest = local_manifest();
    let permission =
      manifest.services.iter().find(|service| service.name == PERMISSION_SERVICE).expect("macOS local Runner publishes PermissionService");
    assert_eq!(permission.methods.iter().map(|method| method.name.as_str()).collect::<Vec<_>>(), ["ProbePermissions"]);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn local_runner_manifest_publishes_exact_media_control_capability() {
    let manifest = local_manifest();
    let media = manifest
      .services
      .iter()
      .find(|service| service.name == MEDIA_CONTROL_SERVICE)
      .expect("macOS local Runner publishes MediaControlService");
    assert_eq!(
      media.methods.iter().map(|method| method.name.as_str()).collect::<Vec<_>>(),
      [
        "GetNowPlaying",
        "Play",
        "Pause",
        "TogglePlayPause",
        "NextTrack",
        "PreviousTrack"
      ]
    );
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn local_runner_manifest_publishes_exact_application_capability() {
    let manifest = local_manifest();
    let application =
      manifest.services.iter().find(|service| service.name == APPLICATION_SERVICE).expect("macOS local Runner publishes ApplicationService");
    assert_eq!(application.methods.iter().map(|method| method.name.as_str()).collect::<Vec<_>>(), ["ActivateBundleId"]);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn local_runner_manifest_publishes_exact_accessibility_capability() {
    let manifest = local_manifest();
    let accessibility = manifest
      .services
      .iter()
      .find(|service| service.name == ACCESSIBILITY_SERVICE)
      .expect("macOS local Runner publishes AccessibilityService");
    assert_eq!(accessibility.methods.iter().map(|method| method.name.as_str()).collect::<Vec<_>>(), ["FocusText"]);
  }

  #[test]
  fn runner_manifest_rejects_undeclared_business_services() {
    let services = vec![
      DISPLAY_SERVICE.to_string(),
      WINDOW_SERVICE.to_string(),
      "third.party.HiddenService".to_string(),
    ];

    let error = validate_manifest(&services, &descriptor_closure(), &local_manifest())
      .expect_err("undeclared services must not enter the trusted registry");

    assert!(error.to_string().contains("differs from its RunnerClass"));
  }

  #[cfg(unix)]
  async fn managed_runner(lifecycle: core_proto::RunnerLifecycle, leases: u32) -> ManagedRunner {
    let child = tokio::process::Command::new("/bin/sleep").arg("10").spawn().expect("spawn inert test child");
    ManagedRunner {
      record: core_proto::Runner {
        r#ref: Some(core_proto::RunnerRef {
          runner_id: "runner_test".to_string(),
        }),
        lifecycle: lifecycle as i32,
        idle_timeout: Some(prost_types::Duration {
          seconds: 0,
          nanos: 50_000_000,
        }),
        phase: core_proto::RunnerPhase::Ready as i32,
        capabilities: vec![core_proto::RunnerCapability {
          service: DISPLAY_SERVICE.to_string(),
          methods: vec!["ListDisplays".to_string()],
        }],
        operation_capacity: 1,
        active_run_leases: leases,
        ..core_proto::Runner::default()
      },
      runtime: ManagedRunnerRuntime::Executable { child },
      channel: Channel::from_static("http://[::]:1").connect_lazy(),
      external_routes: [(DISPLAY_SERVICE.to_string(), "ListDisplays".to_string())].into_iter().collect(),
      reserved_operation_capacity: leases,
    }
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn dropped_operation_permit_balances_cancellation_safe_accounting() {
    let supervisor = RunnerSupervisor::new(core_proto::DeviceRef {
      device_id: "device_test".to_string(),
    });
    let managed = managed_runner(core_proto::RunnerLifecycle::UnlessShutdown, 0).await;
    supervisor.runners.lock().expect("registry").insert("runner_test".to_string(), managed);

    let permit = supervisor.begin_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
    assert_eq!(supervisor.get("runner_test").expect("Runner").runner.expect("record").active_operations, 1);
    drop(permit);
    assert_eq!(supervisor.get("runner_test").expect("Runner").runner.expect("record").active_operations, 0);

    let managed = supervisor.runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
    stop_managed(managed).await.expect("stop test Runner");
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn aggregated_admission_requires_the_immutable_external_route_snapshot() {
    let supervisor = RunnerSupervisor::new(core_proto::DeviceRef {
      device_id: "device_test".to_string(),
    });
    let mut managed = managed_runner(core_proto::RunnerLifecycle::UnlessShutdown, 0).await;
    managed.external_routes.clear();
    supervisor.runners.lock().expect("registry").insert("runner_test".to_string(), managed);

    assert!(matches!(
      supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays"),
      Err(RunnerError::Unimplemented(_))
    ));
    assert_eq!(supervisor.get("runner_test").expect("Runner").runner.expect("record").active_operations, 0);

    let permit = supervisor.begin_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("internal typed call remains admitted");
    drop(permit);
    let managed = supervisor.runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
    stop_managed(managed).await.expect("stop test Runner");
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn final_activity_selects_ephemeral_stop_or_unless_idle_deadline() {
    let runners = Arc::new(Mutex::new(HashMap::new()));
    let managed = managed_runner(core_proto::RunnerLifecycle::Ephemeral, 1).await;
    runners.lock().expect("registry").insert("runner_test".to_string(), managed);
    let (ephemeral, deadline) = decrement_activity_locked(&runners, "runner_test", true).expect("release ephemeral lease");
    assert!(deadline.is_none());
    stop_managed(ephemeral.expect("ephemeral Runner stops immediately")).await.expect("stop ephemeral test Runner");

    let managed = managed_runner(core_proto::RunnerLifecycle::UnlessIdle, 1).await;
    runners.lock().expect("registry").insert("runner_test".to_string(), managed);
    let (stopped, deadline) = decrement_activity_locked(&runners, "runner_test", true).expect("release idle lease");
    assert!(stopped.is_none());
    assert!(deadline.is_some());
    assert!(runners.lock().expect("registry").get("runner_test").expect("idle Runner remains registered").record.idle_deadline.is_some());
    let managed = runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
    stop_managed(managed).await.expect("stop idle test Runner");
  }
}
