//! Transport-aware Rust client for AUV Device, Run, Runner, and capability APIs.
//!
//! The client hides tonic channel construction. Core Driver calls can enter
//! the resource hierarchy through [`Client::runner`].

pub mod discovery;
pub mod driver;
pub mod placement;
pub mod profile;

use std::path::PathBuf;
use std::str::FromStr;

use auv_api_proto::auv::api::core::v1 as core_proto;
use auv_api_proto::auv::api::core::v1::device_service_client::DeviceServiceClient;
use auv_api_proto::auv::api::core::v1::discovery_service_client::DiscoveryServiceClient;
use auv_api_proto::auv::api::core::v1::run_service_client::RunServiceClient;
use auv_api_proto::auv::api::core::v1::runner_class_service_client::RunnerClassServiceClient;
use auv_api_proto::auv::api::core::v1::runner_service_client::RunnerServiceClient;
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
use auv_api_proto::v1::inference as inference_proto;
use auv_api_proto::v1::inference::object_detection_service_client::ObjectDetectionServiceClient;
use prost::Message;
use tonic::metadata::{Binary, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::{Channel, Endpoint};

/// Binary gRPC metadata used by generated custom Runner clients to select an
/// already-admitted Runner lease at the daemon gateway.
pub const RUNNER_LEASE_METADATA: &str = "auv-runner-lease-bin";

/// Adds daemon routing metadata without changing an application-owned request
/// message. Custom protobuf schemas therefore do not import AUV core lease
/// types merely to reach their daemon-owned Runner.
#[derive(Clone, Debug)]
pub struct RunnerLeaseInterceptor {
  encoded: MetadataValue<Binary>,
}

impl RunnerLeaseInterceptor {
  pub fn new(lease: core_proto::RunnerLeaseRef) -> Result<Self, tonic::Status> {
    if lease.lease_id.is_empty() {
      return Err(tonic::Status::invalid_argument("Runner lease must include lease_id"));
    }
    Ok(Self {
      encoded: MetadataValue::from_bytes(&lease.encode_to_vec()),
    })
  }
}

impl Interceptor for RunnerLeaseInterceptor {
  fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    if request.metadata().get_bin(RUNNER_LEASE_METADATA).is_some() {
      return Err(tonic::Status::invalid_argument("Runner lease metadata is already present"));
    }
    request.metadata_mut().insert_bin(RUNNER_LEASE_METADATA, self.encoded.clone());
    Ok(request)
  }
}

pub type RunnerTransport = tonic::service::interceptor::InterceptedService<Channel, RunnerLeaseInterceptor>;

/// Resolved, non-secret context inherited by an AUV plugin invocation.
///
/// This value is passed inline through `AUV_CONTEXT`. It intentionally stores
/// only stable references; credentials are resolved by the selected client
/// profile and must never be serialized into this process contract.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AuvContext {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub device_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub run_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub daemon_endpoint: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_profile: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub credential_profile: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub invocation_id: Option<String>,
}

impl AuvContext {
  pub fn from_env() -> Result<Self, ContextError> {
    let value = std::env::var("AUV_CONTEXT").map_err(ContextError::Environment)?;
    serde_json::from_str(&value).map_err(ContextError::Decode)
  }
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
  #[error("AUV_CONTEXT is unavailable or is not valid Unicode: {0}")]
  Environment(std::env::VarError),
  #[error("AUV_CONTEXT is not valid JSON: {0}")]
  Decode(serde_json::Error),
  #[error("could not resolve an AUV daemon endpoint from the context or local discovery")]
  EndpointNotDiscovered,
  #[error("context daemon endpoint {context:?} does not match paired Device profile endpoint {profile:?}")]
  ProfileEndpointMismatch { context: String, profile: String },
  #[error(transparent)]
  Profile(#[from] profile::ProfileError),
  #[error("paired daemon ListDevices failed: {0}")]
  RemoteDeviceList(tonic::Status),
  #[error("paired daemon did not expose configured canonical Device ID {0:?}")]
  CanonicalDeviceMissing(String),
  #[error("paired daemon Device {device_id:?} reports name {actual:?}, expected configured canonical name {expected:?}")]
  CanonicalDeviceNameMismatch {
    device_id: String,
    actual: String,
    expected: String,
  },
  #[error("Device selection {selector:?} is ambiguous across local and paired profiles; candidate IDs: {candidate_ids}")]
  DeviceSelectionAmbiguous {
    selector: String,
    candidate_ids: String,
  },
  #[error("Device selection does not match the local daemon or a paired Device profile")]
  DeviceNotConfigured,
  #[error("Run {0:?} was not found on the local daemon or any configured paired Device")]
  RunNotFound(String),
  #[error("Run {run_id:?} exists on more than one daemon: {locations}")]
  RunAmbiguous { run_id: String, locations: String },
  #[error("failed to look up Run on {location}: {status}")]
  RunLookup {
    location: String,
    status: tonic::Status,
  },
  #[error(transparent)]
  Discovery(#[from] discovery::DiscoveryError),
  #[error(transparent)]
  Connect(#[from] tonic::transport::Error),
}

/// Mutual-TLS credentials for one previously paired remote daemon.
#[derive(Clone, Debug)]
pub struct PairedConnectConfig {
  pub endpoint: http::Uri,
  pub server_name: String,
  pub server_ca_certificate_pem: Vec<u8>,
  pub client_certificate_pem: Vec<u8>,
  pub client_private_key_pem: Vec<u8>,
}

/// Address of an AUV API server from the client's point of view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectEndpoint {
  /// gRPC over HTTP/2 to a TCP endpoint, for example `http://127.0.0.1:9847`.
  Tcp(http::Uri),
  /// gRPC over HTTP/2 carried by a local Unix domain socket.
  #[cfg(unix)]
  Unix(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndpointParseError {
  #[error("invalid AUV API endpoint URI: {0}")]
  InvalidUri(String),
  #[error("Unix endpoint path must be absolute: {0}")]
  RelativeUnixPath(String),
  #[error("remote TCP endpoints require the future TLS and paired-authority transport: {0}")]
  RemoteAuthorityRequired(String),
  #[error("unsupported AUV API endpoint scheme: {0}")]
  UnsupportedScheme(String),
}

impl FromStr for ConnectEndpoint {
  type Err = EndpointParseError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    if let Some(path) = value.strip_prefix("unix://") {
      let path = PathBuf::from(path);
      if !path.is_absolute() {
        return Err(EndpointParseError::RelativeUnixPath(value.to_string()));
      }
      #[cfg(unix)]
      return Ok(Self::Unix(path));
      #[cfg(not(unix))]
      return Err(EndpointParseError::UnsupportedScheme("unix".to_string()));
    }

    let uri = value.parse::<http::Uri>().map_err(|error| EndpointParseError::InvalidUri(error.to_string()))?;
    let scheme = uri.scheme_str().unwrap_or_default();
    if scheme != "http" {
      return Err(EndpointParseError::UnsupportedScheme(scheme.to_string()));
    }
    let host = uri.host().ok_or_else(|| EndpointParseError::InvalidUri("TCP endpoint omitted host".to_string()))?;
    if !matches!(uri.path(), "" | "/") || uri.query().is_some() {
      return Err(EndpointParseError::InvalidUri("TCP endpoint must not include a path or query".to_string()));
    }
    let ip_host = host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host);
    let loopback = host.eq_ignore_ascii_case("localhost") || ip_host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !loopback {
      return Err(EndpointParseError::RemoteAuthorityRequired(value.to_string()));
    }
    Ok(Self::Tcp(uri))
  }
}

impl std::fmt::Display for ConnectEndpoint {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Tcp(uri) => write!(formatter, "http://{}", uri.authority().expect("validated TCP endpoint has authority")),
      #[cfg(unix)]
      Self::Unix(path) => write!(formatter, "unix://{}", path.display()),
    }
  }
}

// Endpoint precedence and descriptor discovery remain process-lifecycle policy
// in auv-cli; this transport client connects only to its caller's selection.

/// Transport client connected to one AUV daemon.
#[derive(Clone, Debug)]
pub struct Client {
  context: Option<AuvContext>,
  paired_remote: bool,
  channel: Channel,
  discovery: DiscoveryServiceClient<Channel>,
  device: DeviceServiceClient<Channel>,
  runner: RunnerServiceClient<Channel>,
  runner_class: RunnerClassServiceClient<Channel>,
  run: RunServiceClient<Channel>,
  display: DisplayServiceClient<Channel>,
  capture: CaptureServiceClient<Channel>,
  window: WindowServiceClient<Channel>,
  text_recognition: TextRecognitionServiceClient<Channel>,
  input: InputServiceClient<Channel>,
  permission: PermissionServiceClient<Channel>,
  application: ApplicationServiceClient<Channel>,
  accessibility: AccessibilityServiceClient<Channel>,
  media_control: MediaControlServiceClient<Channel>,
  overlay: OverlayServiceClient<Channel>,
  detector: ObjectDetectionServiceClient<Channel>,
}

impl Client {
  fn from_channel(channel: Channel) -> Self {
    Self {
      context: None,
      paired_remote: false,
      channel: channel.clone(),
      discovery: DiscoveryServiceClient::new(channel.clone()),
      device: DeviceServiceClient::new(channel.clone()),
      runner: RunnerServiceClient::new(channel.clone()),
      runner_class: RunnerClassServiceClient::new(channel.clone()),
      run: RunServiceClient::new(channel.clone()),
      display: DisplayServiceClient::new(channel.clone()),
      capture: CaptureServiceClient::new(channel.clone()).max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES),
      window: WindowServiceClient::new(channel.clone()),
      text_recognition: TextRecognitionServiceClient::new(channel.clone())
        .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
        .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES),
      input: InputServiceClient::new(channel.clone()),
      permission: PermissionServiceClient::new(channel.clone()),
      application: ApplicationServiceClient::new(channel.clone()),
      accessibility: AccessibilityServiceClient::new(channel.clone()),
      media_control: MediaControlServiceClient::new(channel.clone()),
      overlay: OverlayServiceClient::new(channel.clone()),
      detector: ObjectDetectionServiceClient::new(channel.clone())
        .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
        .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES),
    }
  }

  /// Lists the typed, discoverable API methods visible to the current
  /// principal from daemon-trusted Runner descriptors.
  pub async fn list_services(&mut self) -> Result<Vec<core_proto::ApiService>, tonic::Status> {
    Ok(self.discovery.list_services(core_proto::ListServicesRequest {}).await?.into_inner().services)
  }

  pub async fn list_devices(&mut self) -> Result<Vec<core_proto::Device>, tonic::Status> {
    Ok(self.device.list_devices(core_proto::ListDevicesRequest {}).await?.into_inner().devices)
  }

  pub async fn get_device(&mut self, device_id: impl Into<String>) -> Result<core_proto::Device, tonic::Status> {
    self
      .device
      .get_device(core_proto::GetDeviceRequest {
        device: Some(core_proto::DeviceRef {
          device_id: device_id.into(),
        }),
      })
      .await?
      .into_inner()
      .device
      .ok_or_else(|| tonic::Status::internal("GetDevice response omitted Device"))
  }

  pub async fn create_runner(&mut self, request: core_proto::CreateRunnerRequest) -> Result<core_proto::Runner, tonic::Status> {
    self
      .runner
      .create_runner(request)
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("CreateRunner response omitted Runner"))
  }

  pub async fn list_runner_classes(&mut self, device: Option<core_proto::DeviceRef>) -> Result<Vec<core_proto::RunnerClass>, tonic::Status> {
    Ok(self.runner_class.list_runner_classes(core_proto::ListRunnerClassesRequest { device }).await?.into_inner().runner_classes)
  }

  pub async fn get_runner_class(
    &mut self,
    runner_class: impl Into<String>,
    device: Option<core_proto::DeviceRef>,
  ) -> Result<core_proto::RunnerClass, tonic::Status> {
    self
      .runner_class
      .get_runner_class(core_proto::GetRunnerClassRequest {
        device,
        runner_class: Some(core_proto::RunnerClassRef {
          runner_class: runner_class.into(),
        }),
      })
      .await?
      .into_inner()
      .runner_class
      .ok_or_else(|| tonic::Status::internal("GetRunnerClass response omitted RunnerClass"))
  }

  pub async fn create_run(&mut self, request: core_proto::CreateRunRequest) -> Result<core_proto::Run, tonic::Status> {
    self.run.create_run(request).await?.into_inner().run.ok_or_else(|| tonic::Status::internal("CreateRun response omitted Run"))
  }

  pub async fn list_runs(&mut self) -> Result<Vec<core_proto::Run>, tonic::Status> {
    Ok(self.run.list_runs(core_proto::ListRunsRequest {}).await?.into_inner().runs)
  }

  pub async fn get_run(&mut self, run_id: impl Into<String>) -> Result<core_proto::Run, tonic::Status> {
    self
      .run
      .get_run(core_proto::GetRunRequest {
        run: Some(core_proto::RunRef {
          run_id: run_id.into(),
        }),
      })
      .await?
      .into_inner()
      .run
      .ok_or_else(|| tonic::Status::internal("GetRun response omitted Run"))
  }

  pub async fn stop_run(&mut self, run_id: impl Into<String>, outcome: core_proto::RunOutcome) -> Result<core_proto::Run, tonic::Status> {
    self
      .run
      .stop_run(core_proto::StopRunRequest {
        run: Some(core_proto::RunRef {
          run_id: run_id.into(),
        }),
        outcome: outcome as i32,
      })
      .await?
      .into_inner()
      .run
      .ok_or_else(|| tonic::Status::internal("StopRun response omitted Run"))
  }

  pub async fn list_runners(&mut self) -> Result<Vec<core_proto::Runner>, tonic::Status> {
    Ok(self.runner.list_runners(core_proto::ListRunnersRequest {}).await?.into_inner().runners)
  }

  pub async fn get_runner(&mut self, runner_id: impl Into<String>) -> Result<core_proto::Runner, tonic::Status> {
    self
      .runner
      .get_runner(core_proto::GetRunnerRequest {
        runner: Some(core_proto::RunnerRef {
          runner_id: runner_id.into(),
        }),
      })
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("GetRunner response omitted Runner"))
  }

  pub async fn delete_runner(&mut self, runner_id: impl Into<String>) -> Result<core_proto::Runner, tonic::Status> {
    self
      .runner
      .delete_runner(core_proto::DeleteRunnerRequest {
        runner: Some(core_proto::RunnerRef {
          runner_id: runner_id.into(),
        }),
      })
      .await?
      .into_inner()
      .runner
      .ok_or_else(|| tonic::Status::internal("DeleteRunner response omitted Runner"))
  }

  pub async fn claim_runner(&mut self, claim: core_proto::RunnerClaim) -> Result<core_proto::ClaimRunnerResponse, tonic::Status> {
    Ok(self.runner.claim_runner(core_proto::ClaimRunnerRequest { claim: Some(claim) }).await?.into_inner())
  }

  pub async fn release_runner_lease(&mut self, lease: core_proto::RunnerLeaseRef) -> Result<bool, tonic::Status> {
    Ok(self.runner.release_runner_lease(core_proto::ReleaseRunnerLeaseRequest { lease: Some(lease) }).await?.into_inner().released)
  }

  /// Enters the typed capability hierarchy for one admitted Runner lease.
  ///
  /// Transport selection has already happened at the Device/client layer, so
  /// child capability code is identical for local and paired remote Devices.
  pub fn runner(&self, lease: core_proto::RunnerLeaseRef) -> Result<driver::RunnerClient, tonic::Status> {
    driver::RunnerClient::new(self.clone(), lease)
  }

  pub async fn list_runner_displays(&mut self, lease: core_proto::RunnerLeaseRef) -> Result<Vec<driver_proto::Display>, tonic::Status> {
    Ok(self.display.list_displays(driver_proto::ListDisplaysRequest { lease: Some(lease) }).await?.into_inner().displays)
  }

  pub async fn list_runner_windows(&mut self, lease: core_proto::RunnerLeaseRef) -> Result<Vec<driver_proto::Window>, tonic::Status> {
    Ok(self.window.list_windows(driver_proto::ListWindowsRequest { lease: Some(lease) }).await?.into_inner().windows)
  }

  pub async fn resolve_runner_window(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    selector: driver_proto::WindowSelector,
  ) -> Result<driver_proto::Window, tonic::Status> {
    self
      .window
      .resolve_window(driver_proto::ResolveWindowRequest {
        lease: Some(lease),
        selector: Some(selector),
      })
      .await?
      .into_inner()
      .window
      .ok_or_else(|| tonic::Status::internal("ResolveWindow response omitted Window"))
  }

  pub async fn capture_runner_window(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    window: driver_proto::WindowRef,
  ) -> Result<driver_proto::CaptureWindowResponse, tonic::Status> {
    Ok(
      self
        .capture
        .capture_window(driver_proto::CaptureWindowRequest {
          lease: Some(lease),
          window: Some(window),
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn capture_runner_display(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    selector: Option<driver_proto::DisplaySelector>,
  ) -> Result<driver_proto::CaptureDisplayResponse, tonic::Status> {
    Ok(
      self
        .capture
        .capture_display(driver_proto::CaptureDisplayRequest {
          lease: Some(lease),
          selector,
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn capture_runner_region(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    region: driver_proto::ScreenRect,
    selector: Option<driver_proto::DisplaySelector>,
  ) -> Result<driver_proto::CaptureRegionResponse, tonic::Status> {
    Ok(
      self
        .capture
        .capture_region(driver_proto::CaptureRegionRequest {
          lease: Some(lease),
          region: Some(region),
          selector,
        })
        .await?
        .into_inner(),
    )
  }

  pub async fn recognize_runner_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::RecognizeTextRequest,
  ) -> Result<driver_proto::RecognizeTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.text_recognition.recognize_text(request).await?.into_inner())
  }

  pub async fn detect_runner_objects(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: inference_proto::DetectObjectsRequest,
  ) -> Result<inference_proto::DetectObjectsResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.detector.detect_objects(request).await?.into_inner())
  }

  pub async fn find_runner_window_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::FindWindowTextRequest,
  ) -> Result<driver_proto::FindWindowTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.text_recognition.find_window_text(request).await?.into_inner())
  }

  pub async fn find_runner_display_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::FindDisplayTextRequest,
  ) -> Result<driver_proto::FindDisplayTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.text_recognition.find_display_text(request).await?.into_inner())
  }

  pub async fn click_runner_window_point(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::ClickWindowPointRequest,
  ) -> Result<driver_proto::ClickWindowPointResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.input.click_window_point(request).await?.into_inner())
  }

  pub async fn click_runner_screen_point(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::ClickScreenPointRequest,
  ) -> Result<driver_proto::ClickScreenPointResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.input.click_screen_point(request).await?.into_inner())
  }

  pub async fn type_runner_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::TypeTextRequest,
  ) -> Result<driver_proto::TypeTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.input.type_text(request).await?.into_inner())
  }

  pub async fn paste_runner_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::PasteTextRequest,
  ) -> Result<driver_proto::PasteTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.input.paste_text(request).await?.into_inner())
  }

  pub async fn press_runner_key(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::PressKeyRequest,
  ) -> Result<driver_proto::PressKeyResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.input.press_key(request).await?.into_inner())
  }

  pub async fn probe_runner_permissions(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<macos_proto::ProbePermissionsResponse, tonic::Status> {
    Ok(self.permission.probe_permissions(macos_proto::ProbePermissionsRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn get_runner_now_playing(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<macos_proto::GetNowPlayingResponse, tonic::Status> {
    Ok(self.media_control.get_now_playing(macos_proto::GetNowPlayingRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn show_runner_overlay(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: driver_proto::ShowOverlayRequest,
  ) -> Result<driver_proto::ShowOverlayResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.overlay.show_overlay(request).await?.into_inner())
  }

  pub async fn remove_runner_overlay(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<driver_proto::RemoveOverlayResponse, tonic::Status> {
    Ok(self.overlay.remove_overlay(driver_proto::RemoveOverlayRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn focus_runner_text(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    mut request: macos_proto::FocusTextRequest,
  ) -> Result<macos_proto::FocusTextResponse, tonic::Status> {
    request.lease = Some(lease);
    Ok(self.accessibility.focus_text(request).await?.into_inner())
  }

  pub async fn play_runner_media(&mut self, lease: core_proto::RunnerLeaseRef) -> Result<macos_proto::PlayResponse, tonic::Status> {
    Ok(self.media_control.play(macos_proto::PlayRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn pause_runner_media(&mut self, lease: core_proto::RunnerLeaseRef) -> Result<macos_proto::PauseResponse, tonic::Status> {
    Ok(self.media_control.pause(macos_proto::PauseRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn toggle_runner_media_play_pause(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<macos_proto::TogglePlayPauseResponse, tonic::Status> {
    Ok(self.media_control.toggle_play_pause(macos_proto::TogglePlayPauseRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn next_runner_media_track(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<macos_proto::NextTrackResponse, tonic::Status> {
    Ok(self.media_control.next_track(macos_proto::NextTrackRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn previous_runner_media_track(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
  ) -> Result<macos_proto::PreviousTrackResponse, tonic::Status> {
    Ok(self.media_control.previous_track(macos_proto::PreviousTrackRequest { lease: Some(lease) }).await?.into_inner())
  }

  pub async fn activate_runner_bundle_id(
    &mut self,
    lease: core_proto::RunnerLeaseRef,
    bundle_id: impl Into<String>,
    settle: Option<prost_types::Duration>,
  ) -> Result<macos_proto::ActivateBundleIdResponse, tonic::Status> {
    Ok(
      self
        .application
        .activate_bundle_id(macos_proto::ActivateBundleIdRequest {
          lease: Some(lease),
          bundle_id: bundle_id.into(),
          settle,
        })
        .await?
        .into_inner(),
    )
  }

  /// Builds the transport consumed by a custom Runner's generated tonic
  /// client, with routing carried outside its application-owned messages.
  pub fn runner_transport(&self, lease: core_proto::RunnerLeaseRef) -> Result<RunnerTransport, tonic::Status> {
    Ok(tonic::service::interceptor::InterceptedService::new(self.channel.clone(), RunnerLeaseInterceptor::new(lease)?))
  }

  /// Connects to one API server without selecting a Device, Run, or Runner.
  pub async fn connect(endpoint: ConnectEndpoint) -> Result<Self, tonic::transport::Error> {
    let channel = match endpoint {
      ConnectEndpoint::Tcp(uri) => Endpoint::from_shared(uri.to_string())?.connect().await?,
      #[cfg(unix)]
      ConnectEndpoint::Unix(path) => {
        let endpoint = Endpoint::from_static("http://[::]:50051");
        endpoint
          .connect_with_connector(tower::service_fn(move |_: http::Uri| {
            let path = path.clone();
            async move { tokio::net::UnixStream::connect(path).await.map(hyper_util::rt::TokioIo::new) }
          }))
          .await?
      }
    };
    Ok(Self::from_channel(channel))
  }

  /// Connects using an already resolved, non-secret plugin context.
  pub async fn from_context(mut context: AuvContext) -> Result<Self, ContextError> {
    if context.config_profile.is_some() || context.credential_profile.is_some() {
      return Self::from_context_with_profiles(context, &profile::ProfileStore::from_env()?).await;
    }
    if context.daemon_endpoint.is_none() && context.run_id.is_some() && context.device_id.is_none() && context.device_name.is_none() {
      return Self::from_run_context(context).await;
    }
    if context.daemon_endpoint.is_none() && (context.device_id.is_some() || context.device_name.is_some()) {
      let profiles = profile::ProfileStore::from_env()?;
      let configured = match profiles.list_devices() {
        Ok(configured) => configured,
        Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
      };
      let remote_matches = configured
        .iter()
        .filter(|device| context.device_id.as_ref().is_none_or(|id| device.device_id() == id))
        .filter(|device| context.device_name.as_ref().is_none_or(|name| device.device_name() == name))
        .collect::<Vec<_>>();
      let mut local = match discovery::resolve(None)? {
        Some(endpoint) => {
          let endpoint_display = endpoint.to_string();
          let client = Self::connect(endpoint).await?;
          Some((endpoint_display, client))
        }
        None => None,
      };
      let local_devices = match local.as_mut() {
        Some((_, client)) => client.list_devices().await.map_err(ContextError::RemoteDeviceList)?,
        None => Vec::new(),
      };
      let local_matches = matching_devices(&context, &local_devices);
      let candidate_ids = local_matches
        .iter()
        .filter_map(|device| device.r#ref.as_ref().map(|reference| reference.device_id.as_str()))
        .chain(remote_matches.iter().map(|device| device.device_id()))
        .collect::<Vec<_>>();
      match (local_matches.as_slice(), remote_matches.as_slice()) {
        ([local_device], []) => {
          let (endpoint, mut local) = local.expect("local match requires a connected local daemon");
          context_matches_canonical_device(&context, local_device)?;
          context.daemon_endpoint = Some(endpoint);
          local.context = Some(context);
          return Ok(local);
        }
        ([], [remote]) => {
          let mut remote_context = context;
          remote_context.config_profile = Some(remote.config_profile().to_string());
          return Self::from_context_with_profiles(remote_context, &profiles).await;
        }
        ([], []) => return Err(ContextError::DeviceNotConfigured),
        _ => {
          return Err(ContextError::DeviceSelectionAmbiguous {
            selector: context.device_id.clone().or(context.device_name.clone()).unwrap_or_default(),
            candidate_ids: candidate_ids.join(", "),
          });
        }
      }
    }
    let endpoint = match context.daemon_endpoint.as_deref() {
      Some(endpoint) => endpoint.parse().map_err(|source| discovery::DiscoveryError::InvalidEndpoint {
        endpoint: endpoint.to_string(),
        source,
      })?,
      None => {
        let endpoint = discovery::resolve(None)?.ok_or(ContextError::EndpointNotDiscovered)?;
        context.daemon_endpoint = Some(endpoint.to_string());
        endpoint
      }
    };
    let mut client = Self::connect(endpoint).await?;
    client.context = Some(context);
    Ok(client)
  }

  async fn from_run_context(context: AuvContext) -> Result<Self, ContextError> {
    let run_id = context.run_id.clone().expect("run-only resolution requires run_id");
    let mut matches = Vec::<(String, Client)>::new();
    if let Some(endpoint) = discovery::resolve(None)? {
      let endpoint_display = endpoint.to_string();
      let mut local = Self::connect(endpoint).await?;
      match local.get_run(run_id.clone()).await {
        Ok(_) => {
          let mut local_context = context.clone();
          local_context.daemon_endpoint = Some(endpoint_display);
          local.context = Some(local_context);
          matches.push(("local".to_string(), local));
        }
        Err(status) if status.code() == tonic::Code::NotFound => {}
        Err(status) => {
          return Err(ContextError::RunLookup {
            location: "local daemon".to_string(),
            status,
          });
        }
      }
    }
    let profiles = profile::ProfileStore::from_env()?;
    let configured = match profiles.list_devices() {
      Ok(configured) => configured,
      Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(error) => return Err(error.into()),
    };
    for configured in configured {
      let mut remote_context = context.clone();
      remote_context.config_profile = Some(configured.config_profile().to_string());
      let mut remote = Self::from_context_with_profiles(remote_context, &profiles).await?;
      match remote.get_run(run_id.clone()).await {
        Ok(_) => matches.push((configured.config_profile().to_string(), remote)),
        Err(status) if status.code() == tonic::Code::NotFound => {}
        Err(status) => {
          return Err(ContextError::RunLookup {
            location: format!("paired profile {:?}", configured.config_profile()),
            status,
          });
        }
      }
    }
    match matches.len() {
      0 => Err(ContextError::RunNotFound(run_id)),
      1 => Ok(matches.pop().expect("one Run match").1),
      _ => Err(ContextError::RunAmbiguous {
        run_id,
        locations: matches.into_iter().map(|(location, _)| location).collect::<Vec<_>>().join(", "),
      }),
    }
  }

  /// Resolves one explicitly selected paired Device through an explicit
  /// profile store. This is also the deterministic test/embedding seam for
  /// callers that do not want process environment configuration.
  pub async fn from_context_with_profiles(mut context: AuvContext, profiles: &profile::ProfileStore) -> Result<Self, ContextError> {
    let profile = profiles.resolve(&context)?;
    if let Some(endpoint) = context.daemon_endpoint.as_deref() {
      let selected = profile::validate_remote_endpoint(endpoint)?;
      if selected.authority() != profile.endpoint().authority() {
        return Err(ContextError::ProfileEndpointMismatch {
          context: selected.to_string(),
          profile: profile.endpoint().to_string(),
        });
      }
    }
    let mut client = Self::connect_paired(PairedConnectConfig {
      endpoint: profile.endpoint().clone(),
      server_name: profile.server_name().to_string(),
      server_ca_certificate_pem: profile.server_ca_certificate_pem().to_vec(),
      client_certificate_pem: profile.client_certificate_pem().to_vec(),
      client_private_key_pem: profile.client_private_key_pem().to_vec(),
    })
    .await?;
    let devices = client.list_devices().await.map_err(ContextError::RemoteDeviceList)?;
    let canonical = devices
      .into_iter()
      .find(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == profile.device_id()))
      .ok_or_else(|| ContextError::CanonicalDeviceMissing(profile.device_id().to_string()))?;
    if canonical.name != profile.device_name() {
      return Err(ContextError::CanonicalDeviceNameMismatch {
        device_id: profile.device_id().to_string(),
        actual: canonical.name,
        expected: profile.device_name().to_string(),
      });
    }
    context.device_id = Some(profile.device_id().to_string());
    context.device_name = Some(profile.device_name().to_string());
    context.daemon_endpoint = Some(profile.endpoint().to_string());
    context.config_profile = Some(profile.config_profile().to_string());
    context.credential_profile = Some(profile.credential_profile().to_string());
    client.context = Some(context);
    Ok(client)
  }

  /// Parses `AUV_CONTEXT` and connects to its selected daemon.
  pub async fn from_env() -> Result<Self, ContextError> {
    Self::from_context(AuvContext::from_env()?).await
  }

  pub fn context(&self) -> Option<&AuvContext> {
    self.context.as_ref()
  }

  pub(crate) fn is_paired_remote(&self) -> bool {
    self.paired_remote
  }

  /// Enters the Device/Run/Runner placement hierarchy using this already
  /// connected transport.
  pub fn placement(&self) -> placement::AuvClient {
    placement::AuvClient::from_client(self.clone())
  }

  /// Connects to a remote daemon using a server CA and paired client identity.
  /// Plain remote HTTP is intentionally unavailable.
  pub async fn connect_paired(config: PairedConnectConfig) -> Result<Self, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    let mut client = Self::from_channel(channel);
    client.paired_remote = true;
    Ok(client)
  }
}

fn matching_devices<'a>(context: &AuvContext, devices: &'a [core_proto::Device]) -> Vec<&'a core_proto::Device> {
  devices
    .iter()
    .filter(|device| context.device_id.as_ref().is_none_or(|id| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == *id)))
    .filter(|device| context.device_name.as_ref().is_none_or(|name| device.name == *name))
    .collect()
}

fn context_matches_canonical_device(context: &AuvContext, device: &core_proto::Device) -> Result<(), ContextError> {
  if let Some(id) = context.device_id.as_deref()
    && device.r#ref.as_ref().is_none_or(|reference| reference.device_id != id)
  {
    return Err(ContextError::CanonicalDeviceMissing(id.to_string()));
  }
  if let Some(name) = context.device_name.as_deref()
    && device.name != name
  {
    return Err(ContextError::CanonicalDeviceNameMismatch {
      device_id: device.r#ref.as_ref().map(|reference| reference.device_id.clone()).unwrap_or_default(),
      actual: device.name.clone(),
      expected: name.to_string(),
    });
  }
  Ok(())
}

fn install_tls_crypto_provider() {
  // NOTICE: Cargo feature unification can enable rustls ring and aws-lc-rs in
  // one AUV process, which prevents rustls from choosing automatically. This
  // transport deliberately selects tonic's `tls-ring` provider. Remove the
  // explicit install if the workspace standardizes one provider or rustls
  // supports deterministic multi-provider selection.
  // See `https://docs.rs/rustls/0.23/rustls/crypto/struct.CryptoProvider.html`.
  let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod tests {
  use super::{AuvContext, Client, ConnectEndpoint, RUNNER_LEASE_METADATA, RunnerLeaseInterceptor};
  use auv_api_proto::auv::api::core::v1::RunnerLeaseRef;
  use prost::Message;
  use tonic::service::Interceptor as _;

  #[test]
  fn plugin_context_is_inline_additive_json_without_secrets_or_version() {
    let decoded: AuvContext =
      serde_json::from_str(r#"{"device_id":"device_01H","run_id":"run_01H","daemon_endpoint":"unix:///tmp/auv.sock","future_field":true}"#)
        .expect("decode additive context");
    assert_eq!(decoded.device_id.as_deref(), Some("device_01H"));
    assert_eq!(decoded.run_id.as_deref(), Some("run_01H"));

    let encoded = serde_json::to_value(decoded).expect("encode context");
    assert_eq!(encoded["daemon_endpoint"], "unix:///tmp/auv.sock");
    assert!(encoded.get("version").is_none());
    assert!(encoded.get("token").is_none());
    assert!(encoded.get("credential").is_none());
  }

  #[test]
  fn endpoint_parser_round_trips_loopback_tcp() {
    for value in [
      "http://127.0.0.1:9847",
      "http://[::1]:9847",
      "http://localhost:9847",
    ] {
      let endpoint = value.parse::<ConnectEndpoint>().expect(value);
      assert_eq!(endpoint.to_string(), value);
    }
  }

  #[cfg(unix)]
  #[test]
  fn endpoint_parser_round_trips_absolute_unix_path() {
    let endpoint = "unix:///tmp/auv.sock".parse::<ConnectEndpoint>().expect("Unix endpoint");
    assert_eq!(endpoint.to_string(), "unix:///tmp/auv.sock");
  }

  #[test]
  fn endpoint_parser_rejects_unpaired_remote_and_unsupported_tls() {
    assert!("http://example.com:9847".parse::<ConnectEndpoint>().is_err());
    assert!("https://127.0.0.1:9847".parse::<ConnectEndpoint>().is_err());
  }

  #[test]
  fn custom_runner_interceptor_encodes_one_runner_lease_metadata_value() {
    let lease = RunnerLeaseRef {
      lease_id: "lease_test".to_string(),
      ..Default::default()
    };
    let mut interceptor = RunnerLeaseInterceptor::new(lease.clone()).expect("valid lease");
    let request = interceptor.call(tonic::Request::new(())).expect("inject routing metadata");
    let encoded = request.metadata().get_bin(RUNNER_LEASE_METADATA).expect("lease metadata").to_bytes().expect("binary metadata");
    assert_eq!(RunnerLeaseRef::decode(encoded).unwrap(), lease);
    assert!(RunnerLeaseInterceptor::new(RunnerLeaseRef::default()).is_err());
  }

  #[tokio::test]
  async fn client_constructs_typed_discovery_service() {
    let channel = tonic::transport::Endpoint::from_static("http://[::1]:1").connect_lazy();
    let mut client = Client::from_channel(channel);
    let request = client.list_services();
    drop(request);
  }

  #[tokio::test]
  async fn missing_configuration_profile_store_is_not_silently_ignored() {
    let error = super::Client::from_context_with_profiles(
      AuvContext {
        config_profile: Some("workstation".to_string()),
        ..Default::default()
      },
      &super::profile::ProfileStore::from_paths("missing-config", "missing-credentials"),
    )
    .await
    .expect_err("missing profile store must fail before transport");
    assert!(matches!(error, super::ContextError::Profile(super::profile::ProfileError::Open { .. })));
  }
}
