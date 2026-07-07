//! Godot integration crate for AUV.
//!
//! This crate currently owns the AIRI Godot Stage dev observation client.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tungstenite::{Message, connect};

const CAPABILITY_QUERY_MESSAGE_TYPE: &str = "capability.query";
const RENDER_EXPORT_STAGES_MESSAGE_TYPE: &str = "render.export_stages";

#[derive(Debug, Error)]
pub enum GodotDevObservationError {
  #[error("failed to resolve user home directory for AIRI Godot discovery")]
  MissingHomeDirectory,
  #[error("failed to read {path}: {source}")]
  ReadFile {
    path: PathBuf,
    source: std::io::Error,
  },
  #[error("failed to parse {path}: {source}")]
  ParseJson {
    path: PathBuf,
    source: serde_json::Error,
  },
  #[error("current discovery record did not include an instance path")]
  MissingInstancePath,
  #[error("discovery record has unsupported transport {0:?}; expected websocket-json")]
  UnsupportedTransport(String),
  #[error("failed to connect to Godot dev observation endpoint {endpoint}: {source}")]
  Connect {
    endpoint: String,
    source: tungstenite::Error,
  },
  #[error("failed to send capability query: {0}")]
  Send(tungstenite::Error),
  #[error("failed to read capability response: {0}")]
  Read(tungstenite::Error),
  #[error("failed to create render observation output directory {path}: {source}")]
  CreateOutputDir {
    path: PathBuf,
    source: std::io::Error,
  },
  #[error("failed to write render observation manifest {path}: {source}")]
  WriteManifest {
    path: PathBuf,
    source: std::io::Error,
  },
  #[error("failed to write render observation artifact {path}: {message}")]
  WriteArtifactFile { path: PathBuf, message: String },
  #[error("failed to capture Godot final window: {0}")]
  FinalCapture(String),
  #[error("Godot dev observation response was not text")]
  NonTextResponse,
  #[error("failed to parse Godot dev observation response: {0}")]
  ParseResponse(serde_json::Error),
  #[error("Godot dev observation response type was {actual:?}; expected capability.query.response")]
  UnexpectedResponseType { actual: Option<String> },
  #[error("Godot dev observation response type was {actual:?}; expected render.export_stages.response")]
  UnexpectedRenderExportResponseType { actual: Option<String> },
  #[error("Godot dev observation returned {code}: {message}")]
  RemoteError { code: String, message: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentDiscoveryRecord {
  pub schema_version: u32,
  pub kind: String,
  pub pid: u32,
  pub project_path: PathBuf,
  pub instance_path: PathBuf,
  pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDiscoveryRecord {
  pub schema_version: u32,
  pub kind: String,
  pub pid: u32,
  pub project_path: PathBuf,
  pub transport: String,
  pub endpoint: String,
  pub token: String,
  pub started_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityQueryResult {
  pub transport: String,
  pub features: Vec<String>,
  pub render_stages: Vec<String>,
  pub camera_presets: Vec<String>,
  pub process: CapabilityProcessInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProcessInfo {
  pub pid: u32,
  pub project_path: PathBuf,
  pub airi_bridge_connected: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseEnvelope<T> {
  #[serde(rename = "type")]
  message_type: Option<String>,
  request_id: Option<String>,
  status: Option<String>,
  result: Option<T>,
  error: Option<RemoteErrorPayload>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteErrorPayload {
  code: String,
  message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CapabilityQueryRequest<'a> {
  #[serde(rename = "type")]
  message_type: &'a str,
  request_id: String,
  token: &'a str,
  payload: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderExportStagesResult {
  pub output_dir: PathBuf,
  pub exported_files: Vec<RenderExportedFile>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub context: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderExportedFile {
  pub stage: String,
  pub path: PathBuf,
  pub width: i32,
  pub height: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderObservationArtifact {
  pub manifest_path: PathBuf,
  pub output_dir: PathBuf,
  pub request: RenderObservationRequest,
  pub capabilities: CapabilityQueryResult,
  pub final_capture: FinalCaptureResult,
  pub export: RenderExportStagesResult,
  pub context_files: RenderObservationContextFiles,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderObservationRequest {
  pub stages: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalCaptureResult {
  pub path: PathBuf,
  pub width: u32,
  pub height: u32,
  pub backend: String,
  pub fallback_reason: Option<String>,
  pub window: auv_driver::window::Window,
  pub scale_factor: f64,
  pub capture_bounds: auv_driver::geometry::Rect,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderObservationContextFiles {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub context: Option<PathBuf>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub view_snapshot: Option<PathBuf>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scene: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderExportStagesPayload<'a> {
  output_dir: &'a Path,
  stages: &'a [String],
}

pub type Result<T> = std::result::Result<T, GodotDevObservationError>;

pub fn query_current_capabilities() -> Result<CapabilityQueryResult> {
  let instance = read_current_instance()?;
  query_capabilities(&instance)
}

pub fn export_current_render_observation(output_dir: impl AsRef<Path>, stages: Vec<String>) -> Result<RenderObservationArtifact> {
  let instance = read_current_instance()?;
  let capabilities = query_capabilities(&instance)?;
  let selected_stages = if stages.is_empty() {
    capabilities.render_stages.clone()
  } else {
    stages
  };
  let output_dir = output_dir.as_ref().to_path_buf();
  fs::create_dir_all(&output_dir).map_err(|source| GodotDevObservationError::CreateOutputDir {
    path: output_dir.clone(),
    source,
  })?;

  let export = export_render_stages(&instance, &output_dir, &selected_stages)?;
  let context_files = write_context_files(&output_dir, export.context.as_ref())?;
  let final_capture = capture_final_window(&instance, &output_dir.join("final").join("screenshot.png"))?;
  let manifest_path = output_dir.join("manifest.json");
  let artifact = RenderObservationArtifact {
    manifest_path: manifest_path.clone(),
    output_dir,
    request: RenderObservationRequest {
      stages: selected_stages,
    },
    capabilities,
    final_capture,
    export,
    context_files,
  };
  let manifest = serde_json::to_vec_pretty(&artifact).expect("render observation manifest should serialize");
  fs::write(&manifest_path, manifest).map_err(|source| GodotDevObservationError::WriteManifest {
    path: manifest_path,
    source,
  })?;

  Ok(artifact)
}

fn write_context_files(output_dir: &Path, context: Option<&Value>) -> Result<RenderObservationContextFiles> {
  let Some(context) = context else {
    return Ok(RenderObservationContextFiles::default());
  };

  let context_dir = output_dir.join("context");
  fs::create_dir_all(&context_dir).map_err(|source| GodotDevObservationError::CreateOutputDir {
    path: context_dir.clone(),
    source,
  })?;

  let context_path = context_dir.join("context.json");
  write_json_artifact(&context_path, context)?;

  let mut files = RenderObservationContextFiles {
    context: Some(context_path),
    ..RenderObservationContextFiles::default()
  };

  if let Some(view_snapshot) = context.get("viewSnapshot") {
    let view_snapshot_path = context_dir.join("view-snapshot.json");
    write_json_artifact(&view_snapshot_path, view_snapshot)?;
    files.view_snapshot = Some(view_snapshot_path);
  }

  if let Some(scene) = context.get("scene") {
    let scene_path = context_dir.join("scene.json");
    write_json_artifact(&scene_path, scene)?;
    files.scene = Some(scene_path);
  }

  Ok(files)
}

fn write_json_artifact(path: &Path, value: &Value) -> Result<()> {
  let encoded = serde_json::to_vec_pretty(value).map_err(|source| GodotDevObservationError::WriteArtifactFile {
    path: path.to_path_buf(),
    message: source.to_string(),
  })?;
  fs::write(path, encoded).map_err(|source| GodotDevObservationError::WriteArtifactFile {
    path: path.to_path_buf(),
    message: source.to_string(),
  })
}

#[cfg(target_os = "windows")]
fn capture_final_window(instance: &InstanceDiscoveryRecord, path: &Path) -> Result<FinalCaptureResult> {
  use auv_driver::Driver;
  use auv_driver::selector::{App, Window};

  let driver = auv_driver_windows::WindowsDriver::new();
  let session = driver.open_local().map_err(|error| GodotDevObservationError::FinalCapture(error.to_string()))?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::pid(instance.pid)))
    .map_err(|error| GodotDevObservationError::FinalCapture(error.to_string()))?;
  let capture = session.window().capture(&window).map_err(|error| GodotDevObservationError::FinalCapture(error.to_string()))?;

  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|source| GodotDevObservationError::CreateOutputDir {
      path: parent.to_path_buf(),
      source,
    })?;
  }

  capture.image.save(path).map_err(|source| GodotDevObservationError::WriteArtifactFile {
    path: path.to_path_buf(),
    message: source.to_string(),
  })?;

  Ok(FinalCaptureResult {
    path: path.to_path_buf(),
    width: capture.image.width(),
    height: capture.image.height(),
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
    window,
    scale_factor: capture.scale_factor,
    capture_bounds: capture.bounds,
  })
}

#[cfg(not(target_os = "windows"))]
fn capture_final_window(_instance: &InstanceDiscoveryRecord, _path: &Path) -> Result<FinalCaptureResult> {
  Err(GodotDevObservationError::FinalCapture("Godot final window capture is currently implemented for Windows only".to_string()))
}

pub fn read_current_instance() -> Result<InstanceDiscoveryRecord> {
  let current_path = default_discovery_root()?.join("current.json");
  let current = read_json::<CurrentDiscoveryRecord>(&current_path)?;
  if current.instance_path.as_os_str().is_empty() {
    return Err(GodotDevObservationError::MissingInstancePath);
  }

  read_json(&current.instance_path)
}

pub fn query_capabilities(instance: &InstanceDiscoveryRecord) -> Result<CapabilityQueryResult> {
  let request_id = make_request_id("capability");
  let request = CapabilityQueryRequest {
    message_type: CAPABILITY_QUERY_MESSAGE_TYPE,
    request_id: request_id.clone(),
    token: &instance.token,
    payload: Value::Object(Default::default()),
  };
  let envelope = send_request::<CapabilityQueryResult>(instance, &request)?;
  if envelope.status.as_deref() == Some("error") {
    return finish_response(envelope, &request_id);
  }

  if envelope.message_type.as_deref() != Some("capability.query.response") {
    return Err(GodotDevObservationError::UnexpectedResponseType {
      actual: envelope.message_type,
    });
  }

  finish_response(envelope, &request_id)
}

fn export_render_stages(instance: &InstanceDiscoveryRecord, output_dir: &Path, stages: &[String]) -> Result<RenderExportStagesResult> {
  let request_id = make_request_id("render-export");
  let payload = serde_json::to_value(RenderExportStagesPayload { output_dir, stages }).expect("render export payload should serialize");
  let request = CapabilityQueryRequest {
    message_type: RENDER_EXPORT_STAGES_MESSAGE_TYPE,
    request_id: request_id.clone(),
    token: &instance.token,
    payload,
  };
  let envelope = send_request::<RenderExportStagesResult>(instance, &request)?;
  if envelope.status.as_deref() == Some("error") {
    return finish_response(envelope, &request_id);
  }

  if envelope.message_type.as_deref() != Some("render.export_stages.response") {
    return Err(GodotDevObservationError::UnexpectedRenderExportResponseType {
      actual: envelope.message_type,
    });
  }

  finish_response(envelope, &request_id)
}

fn send_request<T>(instance: &InstanceDiscoveryRecord, request: &CapabilityQueryRequest<'_>) -> Result<ResponseEnvelope<T>>
where
  T: for<'de> Deserialize<'de>,
{
  if instance.transport != "websocket-json" {
    return Err(GodotDevObservationError::UnsupportedTransport(instance.transport.clone()));
  }

  let url = format!("ws://{}/", instance.endpoint);
  let (mut socket, _) = connect(url.as_str()).map_err(|source| GodotDevObservationError::Connect {
    endpoint: instance.endpoint.clone(),
    source,
  })?;

  let request_text = serde_json::to_string(&request).expect("capability query request should serialize");
  socket.send(Message::Text(request_text.into())).map_err(GodotDevObservationError::Send)?;

  let response = socket.read().map_err(GodotDevObservationError::Read)?;
  let response_text = response.into_text().map_err(|_| GodotDevObservationError::NonTextResponse)?;
  serde_json::from_str::<ResponseEnvelope<T>>(&response_text).map_err(GodotDevObservationError::ParseResponse)
}

fn finish_response<T>(envelope: ResponseEnvelope<T>, request_id: &str) -> Result<T> {
  if envelope.status.as_deref() == Some("error") {
    let error = envelope.error.unwrap_or(RemoteErrorPayload {
      code: "unknown".to_string(),
      message: "Godot dev observation returned an error without details".to_string(),
    });
    return Err(GodotDevObservationError::RemoteError {
      code: error.code,
      message: error.message,
    });
  }

  if envelope.request_id.as_deref() != Some(request_id) {
    return Err(GodotDevObservationError::RemoteError {
      code: "request_id_mismatch".to_string(),
      message: "Godot dev observation response did not match the request id".to_string(),
    });
  }

  envelope.result.ok_or(GodotDevObservationError::RemoteError {
    code: "missing_result".to_string(),
    message: "Godot dev observation response did not include a result".to_string(),
  })
}

pub fn default_discovery_root() -> Result<PathBuf> {
  let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")).ok_or(GodotDevObservationError::MissingHomeDirectory)?;

  Ok(PathBuf::from(home).join(".airi").join("godot-stage").join("dev"))
}

fn read_json<T>(path: &Path) -> Result<T>
where
  T: for<'de> Deserialize<'de>,
{
  let contents = fs::read_to_string(path).map_err(|source| GodotDevObservationError::ReadFile {
    path: path.to_path_buf(),
    source,
  })?;
  serde_json::from_str(&contents).map_err(|source| GodotDevObservationError::ParseJson {
    path: path.to_path_buf(),
    source,
  })
}

fn make_request_id(kind: &str) -> String {
  let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_nanos()).unwrap_or_default();
  format!("auv-godot-{kind}-{}-{nanos}", std::process::id())
}
