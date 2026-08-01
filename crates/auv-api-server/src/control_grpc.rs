//! Typed gRPC adapters for Device, Run, and Runner control resources.

use std::sync::Arc;

use auv_api_proto::auv::api::core::v1 as proto;
use auv_api_proto::auv::api::core::v1::device_service_server::DeviceService;
use auv_api_proto::auv::api::core::v1::discovery_service_server::DiscoveryService;
use auv_api_proto::auv::api::core::v1::run_service_server::RunService;
use auv_api_proto::auv::api::core::v1::runner_class_service_server::RunnerClassService;
use auv_api_proto::auv::api::core::v1::runner_service_server::RunnerService;
use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
use auv_api_proto::auv::api::driver::macos::v1::accessibility_service_server::AccessibilityService;
use auv_api_proto::auv::api::driver::macos::v1::application_service_server::ApplicationService;
use auv_api_proto::auv::api::driver::macos::v1::media_control_service_server::MediaControlService;
use auv_api_proto::auv::api::driver::macos::v1::permission_service_server::PermissionService;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::driver::v1::capture_service_server::CaptureService;
use auv_api_proto::auv::api::driver::v1::display_service_server::DisplayService;
use auv_api_proto::auv::api::driver::v1::input_service_server::InputService;
use auv_api_proto::auv::api::driver::v1::overlay_service_server::OverlayService;
use auv_api_proto::auv::api::driver::v1::text_recognition_service_server::TextRecognitionService;
use auv_api_proto::auv::api::driver::v1::window_service_server::WindowService;
use auv_api_proto::auv::api::inference::v1 as inference_proto;
use auv_api_proto::auv::api::inference::v1::object_detection_service_server::ObjectDetectionService;
use tonic::{Request, Response, Status};

use crate::authority::ApiScope;
use crate::handler::ApiHandler;
use crate::transport::RequestAuthority;

#[derive(Clone)]
pub(crate) struct DiscoveryServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl DiscoveryServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl DiscoveryService for DiscoveryServiceGrpc {
  async fn list_api_namespaces(
    &self,
    request: Request<proto::ListApiNamespacesRequest>,
  ) -> Result<Response<proto::ListApiNamespacesResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    Ok(Response::new(proto::ListApiNamespacesResponse {
      namespaces: vec![proto::ApiNamespace {
        name: "auv".to_string(),
      }],
    }))
  }

  async fn get_api_namespace(
    &self,
    request: Request<proto::GetApiNamespaceRequest>,
  ) -> Result<Response<proto::GetApiNamespaceResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    if request.get_ref().namespace != "auv" {
      return Err(Status::not_found("unknown API namespace"));
    }
    Ok(Response::new(proto::GetApiNamespaceResponse {
      namespace: "auv".to_string(),
      groups: vec![
        proto::ApiGroup {
          name: "core".to_string(),
          versions: vec!["v1".to_string()],
        },
        proto::ApiGroup {
          name: "runtime".to_string(),
          versions: vec!["v1".to_string()],
        },
      ],
    }))
  }

  async fn get_api_group_version(
    &self,
    request: Request<proto::GetApiGroupVersionRequest>,
  ) -> Result<Response<proto::GetApiGroupVersionResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    let can_manage = self.authority.principal(&request, ApiScope::ControlManage).is_ok();
    let request = request.into_inner();
    if request.namespace != "auv" {
      return Err(Status::not_found("unknown API namespace"));
    }
    let resources = match (request.group.as_str(), request.version.as_str()) {
      ("core", "v1") => vec![
        api_resource(
          "devices",
          "Device",
          &[
            proto::ApiResourceOperation::List,
            proto::ApiResourceOperation::Get,
          ],
        ),
        api_resource("services", "ApiService", &[proto::ApiResourceOperation::List]),
      ],
      ("runtime", "v1") => {
        let read = [
          proto::ApiResourceOperation::List,
          proto::ApiResourceOperation::Get,
        ];
        let mut runners = read.to_vec();
        let mut runs = read.to_vec();
        if can_manage {
          runners.extend([
            proto::ApiResourceOperation::Create,
            proto::ApiResourceOperation::Delete,
          ]);
          runs.push(proto::ApiResourceOperation::Create);
        }
        vec![
          api_resource("runners", "Runner", &runners),
          api_resource("runnerclasses", "RunnerClass", &read),
          api_resource(
            "runnerleases",
            "RunnerLease",
            if can_manage {
              &[
                proto::ApiResourceOperation::Create,
                proto::ApiResourceOperation::Delete,
              ]
            } else {
              &[]
            },
          ),
          api_resource("runs", "Run", &runs),
        ]
      }
      _ => return Err(Status::not_found("unknown AUV API group or version")),
    };
    Ok(Response::new(proto::GetApiGroupVersionResponse {
      namespace: request.namespace,
      group: request.group,
      version: request.version,
      resources,
    }))
  }

  async fn list_services(&self, request: Request<proto::ListServicesRequest>) -> Result<Response<proto::ListServicesResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    if self.authority.principal(&request, ApiScope::OperationsExecute).is_err() {
      return Ok(Response::new(proto::ListServicesResponse {
        services: Vec::new(),
      }));
    }
    Ok(Response::new(self.handler.control_plane().list_services()))
  }
}

fn api_resource(name: &str, kind: &str, operations: &[proto::ApiResourceOperation]) -> proto::ApiResource {
  proto::ApiResource {
    name: name.to_string(),
    kind: kind.to_string(),
    operations: operations.iter().map(|operation| *operation as i32).collect(),
  }
}

#[derive(Clone)]
pub(crate) struct DeviceServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl DeviceServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl DeviceService for DeviceServiceGrpc {
  async fn list_devices(&self, request: Request<proto::ListDevicesRequest>) -> Result<Response<proto::ListDevicesResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    Ok(Response::new(self.handler.control_plane().list_devices()))
  }

  async fn get_device(&self, request: Request<proto::GetDeviceRequest>) -> Result<Response<proto::GetDeviceResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    let device_id = request
      .into_inner()
      .device
      .map(|device| device.device_id)
      .filter(|device_id| !device_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("device is required"))?;
    let device =
      self.handler.control_plane().get_device(&device_id).ok_or_else(|| Status::not_found(format!("unknown Device: {device_id}")))?;
    Ok(Response::new(proto::GetDeviceResponse {
      device: Some(device),
    }))
  }
}

#[derive(Clone)]
pub(crate) struct RunServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl RunServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl RunService for RunServiceGrpc {
  async fn create_run(&self, request: Request<proto::CreateRunRequest>) -> Result<Response<proto::CreateRunResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlManage)?;
    self.handler.control_plane().create_run(&principal, request.into_inner()).map(Response::new).map_err(map_control_error)
  }

  async fn list_runs(&self, request: Request<proto::ListRunsRequest>) -> Result<Response<proto::ListRunsResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    Ok(Response::new(self.handler.control_plane().list_runs(&principal)))
  }

  async fn get_run(&self, request: Request<proto::GetRunRequest>) -> Result<Response<proto::GetRunResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    self.handler.control_plane().get_run(&principal, &run_id).map(Response::new).map_err(map_control_error)
  }

  async fn stop_run(&self, request: Request<proto::StopRunRequest>) -> Result<Response<proto::StopRunResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlManage)?;
    let request = request.into_inner();
    let outcome = proto::RunOutcome::try_from(request.outcome).unwrap_or_default();
    let run_id =
      request.run.map(|run| run.run_id).filter(|run_id| !run_id.is_empty()).ok_or_else(|| Status::invalid_argument("run is required"))?;
    self.handler.control_plane().stop_run(&principal, &run_id, outcome).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct RunnerClassServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl RunnerClassServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl RunnerClassService for RunnerClassServiceGrpc {
  async fn list_runner_classes(
    &self,
    request: Request<proto::ListRunnerClassesRequest>,
  ) -> Result<Response<proto::ListRunnerClassesResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    let device_id = request.get_ref().device.as_ref().map(|device| device.device_id.as_str());
    self.handler.control_plane().list_runner_classes(device_id).map(Response::new).map_err(map_control_error)
  }

  async fn get_runner_class(
    &self,
    request: Request<proto::GetRunnerClassRequest>,
  ) -> Result<Response<proto::GetRunnerClassResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let device_id = request.device.as_ref().map(|device| device.device_id.as_str());
    let runner_class = request
      .runner_class
      .as_ref()
      .map(|runner_class| runner_class.runner_class.as_str())
      .filter(|runner_class| !runner_class.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner_class is required"))?;
    self.handler.control_plane().get_runner_class(device_id, runner_class).map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct RunnerServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl RunnerServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl RunnerService for RunnerServiceGrpc {
  async fn create_runner(&self, request: Request<proto::CreateRunnerRequest>) -> Result<Response<proto::CreateRunnerResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlManage)?;
    self.handler.control_plane().create_runner(request.into_inner()).await.map(Response::new).map_err(map_control_error)
  }

  async fn list_runners(&self, request: Request<proto::ListRunnersRequest>) -> Result<Response<proto::ListRunnersResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    Ok(Response::new(self.handler.control_plane().list_runners()))
  }

  async fn get_runner(&self, request: Request<proto::GetRunnerRequest>) -> Result<Response<proto::GetRunnerResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlInspect)?;
    let runner_id = request
      .into_inner()
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    self.handler.control_plane().get_runner(&runner_id).map(Response::new).map_err(map_control_error)
  }

  async fn delete_runner(&self, request: Request<proto::DeleteRunnerRequest>) -> Result<Response<proto::DeleteRunnerResponse>, Status> {
    self.authority.principal(&request, ApiScope::ControlManage)?;
    let runner_id = request
      .into_inner()
      .runner
      .map(|runner| runner.runner_id)
      .filter(|runner_id| !runner_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("runner is required"))?;
    self.handler.control_plane().delete_runner(&runner_id).await.map(Response::new).map_err(map_control_error)
  }

  async fn claim_runner(&self, request: Request<proto::ClaimRunnerRequest>) -> Result<Response<proto::ClaimRunnerResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlManage)?;
    self.handler.control_plane().claim_runner(&principal, request.into_inner()).await.map(Response::new).map_err(map_control_error)
  }

  async fn release_runner_lease(
    &self,
    request: Request<proto::ReleaseRunnerLeaseRequest>,
  ) -> Result<Response<proto::ReleaseRunnerLeaseResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlManage)?;
    let lease = request.into_inner().lease.ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().release_runner_lease(&principal, &lease).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct DisplayServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl DisplayServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl DisplayService for DisplayServiceGrpc {
  async fn list_displays(
    &self,
    request: Request<driver_proto::ListDisplaysRequest>,
  ) -> Result<Response<driver_proto::ListDisplaysResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let lease =
      request.into_inner().lease.filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().list_runner_displays(&principal, &lease).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct WindowServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl WindowServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl WindowService for WindowServiceGrpc {
  async fn list_windows(
    &self,
    request: Request<driver_proto::ListWindowsRequest>,
  ) -> Result<Response<driver_proto::ListWindowsResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let lease =
      request.into_inner().lease.filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().list_runner_windows(&principal, &lease).await.map(Response::new).map_err(map_control_error)
  }

  async fn resolve_window(
    &self,
    request: Request<driver_proto::ResolveWindowRequest>,
  ) -> Result<Response<driver_proto::ResolveWindowResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease = request.lease.filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    let selector = request.selector.ok_or_else(|| Status::invalid_argument("selector is required"))?;
    self.handler.control_plane().resolve_runner_window(&principal, &lease, selector).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct CaptureServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl CaptureServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl CaptureService for CaptureServiceGrpc {
  async fn capture_window(
    &self,
    request: Request<driver_proto::CaptureWindowRequest>,
  ) -> Result<Response<driver_proto::CaptureWindowResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let mut request = request.into_inner();
    let lease =
      request.lease.take().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    request
      .window
      .as_ref()
      .filter(|window| !window.window_id.trim().is_empty())
      .ok_or_else(|| Status::invalid_argument("window is required"))?;
    self.handler.control_plane().capture_runner_window(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn capture_display(
    &self,
    request: Request<driver_proto::CaptureDisplayRequest>,
  ) -> Result<Response<driver_proto::CaptureDisplayResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease = request.lease.filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self
      .handler
      .control_plane()
      .capture_runner_display(&principal, &lease, request.selector)
      .await
      .map(Response::new)
      .map_err(map_control_error)
  }

  async fn capture_region(
    &self,
    request: Request<driver_proto::CaptureRegionRequest>,
  ) -> Result<Response<driver_proto::CaptureRegionResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease = request.lease.filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    let region = request.region.ok_or_else(|| Status::invalid_argument("region is required"))?;
    if [region.x, region.y, region.width, region.height].iter().any(|value| !value.is_finite())
      || region.width <= 0.0
      || region.height <= 0.0
    {
      return Err(Status::invalid_argument("region must be finite with positive width and height"));
    }
    self
      .handler
      .control_plane()
      .capture_runner_region(&principal, &lease, region, request.selector)
      .await
      .map(Response::new)
      .map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct ObjectDetectionServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl ObjectDetectionServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl ObjectDetectionService for ObjectDetectionServiceGrpc {
  async fn detect_objects(
    &self,
    request: Request<inference_proto::DetectObjectsRequest>,
  ) -> Result<Response<inference_proto::DetectObjectsResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    crate::transport::require_host_model_access(&principal)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.detector.is_none() {
      return Err(Status::invalid_argument("detector is required"));
    }
    if request.frame.is_none() {
      return Err(Status::invalid_argument("frame is required"));
    }
    self.handler.control_plane().detect_runner_objects(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct TextRecognitionServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl TextRecognitionServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl TextRecognitionService for TextRecognitionServiceGrpc {
  async fn recognize_text(
    &self,
    request: Request<driver_proto::RecognizeTextRequest>,
  ) -> Result<Response<driver_proto::RecognizeTextResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.capture.is_none() {
      return Err(Status::invalid_argument("capture is required"));
    }
    self.handler.control_plane().recognize_runner_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn find_window_text(
    &self,
    request: Request<driver_proto::FindWindowTextRequest>,
  ) -> Result<Response<driver_proto::FindWindowTextResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    request
      .window
      .as_ref()
      .filter(|window| !window.window_id.trim().is_empty())
      .ok_or_else(|| Status::invalid_argument("window is required"))?;
    if request.query.trim().is_empty() {
      return Err(Status::invalid_argument("query is required"));
    }
    self.handler.control_plane().find_runner_window_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn find_display_text(
    &self,
    request: Request<driver_proto::FindDisplayTextRequest>,
  ) -> Result<Response<driver_proto::FindDisplayTextResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::ControlInspect)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.query.trim().is_empty() {
      return Err(Status::invalid_argument("query is required"));
    }
    self.handler.control_plane().find_runner_display_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct InputServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

#[derive(Clone)]
pub(crate) struct PermissionServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

#[derive(Clone)]
pub(crate) struct MediaControlServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

#[derive(Clone)]
pub(crate) struct OverlayServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl OverlayServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl OverlayService for OverlayServiceGrpc {
  async fn show_overlay(
    &self,
    request: Request<driver_proto::ShowOverlayRequest>,
  ) -> Result<Response<driver_proto::ShowOverlayResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().show_runner_overlay(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn remove_overlay(
    &self,
    request: Request<driver_proto::RemoveOverlayRequest>,
  ) -> Result<Response<driver_proto::RemoveOverlayResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().remove_runner_overlay(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

#[derive(Clone)]
pub(crate) struct ApplicationServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

#[derive(Clone)]
pub(crate) struct AccessibilityServiceGrpc {
  handler: Arc<ApiHandler>,
  authority: RequestAuthority,
}

impl AccessibilityServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl AccessibilityService for AccessibilityServiceGrpc {
  async fn focus_text(&self, request: Request<macos_proto::FocusTextRequest>) -> Result<Response<macos_proto::FocusTextResponse>, Status> {
    use macos_proto::focus_text_request::Selector;

    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.application.trim().is_empty() {
      return Err(Status::invalid_argument("application is required"));
    }
    match request.selector.as_ref() {
      Some(Selector::Query(query)) if !query.trim().is_empty() => {}
      Some(Selector::Path(path)) if !path.trim().is_empty() => {}
      Some(Selector::Query(_)) => return Err(Status::invalid_argument("query must be non-empty")),
      Some(Selector::Path(_)) => return Err(Status::invalid_argument("path must be non-empty")),
      None => return Err(Status::invalid_argument("selector is required")),
    }
    if request.expected_role.as_ref().is_some_and(|role| role.trim().is_empty()) {
      return Err(Status::invalid_argument("expected_role must be non-empty when supplied"));
    }
    self.handler.control_plane().focus_runner_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

impl ApplicationServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl ApplicationService for ApplicationServiceGrpc {
  async fn activate_bundle_id(
    &self,
    request: Request<macos_proto::ActivateBundleIdRequest>,
  ) -> Result<Response<macos_proto::ActivateBundleIdResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.bundle_id.trim().is_empty() {
      return Err(Status::invalid_argument("bundle_id is required"));
    }
    validate_non_negative_duration(request.settle.as_ref(), "settle")?;
    self.handler.control_plane().activate_runner_bundle_id(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

fn validate_non_negative_duration(value: Option<&prost_types::Duration>, field: &'static str) -> Result<(), Status> {
  if value.is_some_and(|value| value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000) {
    return Err(Status::invalid_argument(format!("{field} must be a non-negative protobuf Duration")));
  }
  Ok(())
}

impl MediaControlServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl MediaControlService for MediaControlServiceGrpc {
  async fn get_now_playing(
    &self,
    request: Request<macos_proto::GetNowPlayingRequest>,
  ) -> Result<Response<macos_proto::GetNowPlayingResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().get_runner_now_playing(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn play(&self, request: Request<macos_proto::PlayRequest>) -> Result<Response<macos_proto::PlayResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().play_runner_media(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn pause(&self, request: Request<macos_proto::PauseRequest>) -> Result<Response<macos_proto::PauseResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().pause_runner_media(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn toggle_play_pause(
    &self,
    request: Request<macos_proto::TogglePlayPauseRequest>,
  ) -> Result<Response<macos_proto::TogglePlayPauseResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self
      .handler
      .control_plane()
      .toggle_runner_media_play_pause(&principal, &lease, request)
      .await
      .map(Response::new)
      .map_err(map_control_error)
  }

  async fn next_track(&self, request: Request<macos_proto::NextTrackRequest>) -> Result<Response<macos_proto::NextTrackResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().next_runner_media_track(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn previous_track(
    &self,
    request: Request<macos_proto::PreviousTrackRequest>,
  ) -> Result<Response<macos_proto::PreviousTrackResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().previous_runner_media_track(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

impl PermissionServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl PermissionService for PermissionServiceGrpc {
  async fn probe_permissions(
    &self,
    request: Request<macos_proto::ProbePermissionsRequest>,
  ) -> Result<Response<macos_proto::ProbePermissionsResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    self.handler.control_plane().probe_runner_permissions(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

impl InputServiceGrpc {
  pub(crate) fn new(handler: Arc<ApiHandler>, authority: RequestAuthority) -> Self {
    Self { handler, authority }
  }
}

#[tonic::async_trait]
impl InputService for InputServiceGrpc {
  async fn click_window_point(
    &self,
    request: Request<driver_proto::ClickWindowPointRequest>,
  ) -> Result<Response<driver_proto::ClickWindowPointResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.window.as_ref().is_none_or(|window| window.window_id.trim().is_empty()) {
      return Err(Status::invalid_argument("window is required"));
    }
    if request.point.is_none() {
      return Err(Status::invalid_argument("point is required"));
    }
    if request.options.is_none() {
      return Err(Status::invalid_argument("options are required"));
    }
    self.handler.control_plane().click_runner_window_point(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn click_screen_point(
    &self,
    request: Request<driver_proto::ClickScreenPointRequest>,
  ) -> Result<Response<driver_proto::ClickScreenPointResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.point.is_none() {
      return Err(Status::invalid_argument("point is required"));
    }
    if request.options.is_none() {
      return Err(Status::invalid_argument("options are required"));
    }
    self.handler.control_plane().click_runner_screen_point(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn type_text(&self, request: Request<driver_proto::TypeTextRequest>) -> Result<Response<driver_proto::TypeTextResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.options.is_none() {
      return Err(Status::invalid_argument("options are required"));
    }
    self.handler.control_plane().type_runner_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn paste_text(&self, request: Request<driver_proto::PasteTextRequest>) -> Result<Response<driver_proto::PasteTextResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.text.is_empty() {
      return Err(Status::invalid_argument("text is required"));
    }
    if request.options.is_none() {
      return Err(Status::invalid_argument("options are required"));
    }
    self.handler.control_plane().paste_runner_text(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }

  async fn press_key(&self, request: Request<driver_proto::PressKeyRequest>) -> Result<Response<driver_proto::PressKeyResponse>, Status> {
    let principal = self.authority.principal(&request, ApiScope::OperationsExecute)?;
    let request = request.into_inner();
    let lease =
      request.lease.clone().filter(|lease| !lease.lease_id.is_empty()).ok_or_else(|| Status::invalid_argument("lease is required"))?;
    if request.key.trim().is_empty() {
      return Err(Status::invalid_argument("key is required"));
    }
    self.handler.control_plane().press_runner_key(&principal, &lease, request).await.map(Response::new).map_err(map_control_error)
  }
}

pub(crate) fn map_control_error(error: crate::control_plane::ControlPlaneError) -> Status {
  use crate::control_plane::ControlPlaneError;
  match error {
    ControlPlaneError::InvalidArgument(_) | ControlPlaneError::UnknownDevice(_) => Status::invalid_argument(error.to_string()),
    ControlPlaneError::UnknownRun(_) | ControlPlaneError::UnknownRunner(_) | ControlPlaneError::UnknownRunnerLease(_) => {
      Status::not_found(error.to_string())
    }
    ControlPlaneError::RunnerProviderUnavailable(_) => Status::unimplemented(error.to_string()),
    ControlPlaneError::RunnerCapabilityUnavailable(_) => Status::unimplemented(error.to_string()),
    ControlPlaneError::RunCapacityExhausted(_) => Status::resource_exhausted(error.to_string()),
    ControlPlaneError::RunnerOperation(_) => Status::unavailable(error.to_string()),
    ControlPlaneError::RunnerRpcStatus(status) => status,
  }
}
