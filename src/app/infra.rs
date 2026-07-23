use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{AuvResult, RunStatus, now_millis};
use auv_tracing::Context;

use super::{AppIdentity, AppProbeStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppProbeOperation {
  ProbePermissions,
  ListDisplays,
  ActivateTargetApp,
  ListWindows,
  CaptureAxTree,
  CaptureWindow,
  OcrSample,
}

pub(crate) struct AppProbeStepRequest {
  pub id: &'static str,
  pub command_id: &'static str,
  pub operation: AppProbeOperation,
  pub target_application_id: Option<String>,
  pub inputs: BTreeMap<String, String>,
}

pub(crate) struct AppProbeStepOutput {
  pub summary: String,
  pub artifact_paths: Vec<PathBuf>,
}

/// Executes app-probe capabilities at the platform boundary.
///
/// The orchestrator owns step policy and truthful result shaping. Implementors
/// own direct OS interaction and app-local evidence files only.
pub(crate) trait AppProbeBackend {
  fn perform(&mut self, request: &AppProbeStepRequest, output_dir: &Path) -> AuvResult<AppProbeStepOutput>;
}

pub(crate) struct LocalAppProbeBackend {
  session: auv_driver::LocalDriverSession,
}

impl LocalAppProbeBackend {
  pub(crate) fn open() -> AuvResult<Self> {
    let session = auv_driver::open_local().map_err(|error| format!("failed to open local driver for app probe: {error}"))?;
    Ok(Self { session })
  }
}

impl AppProbeBackend for LocalAppProbeBackend {
  fn perform(&mut self, request: &AppProbeStepRequest, output_dir: &Path) -> AuvResult<AppProbeStepOutput> {
    #[cfg(target_os = "macos")]
    {
      perform_macos_probe_step(&self.session, request, output_dir)
    }

    #[cfg(not(target_os = "macos"))]
    {
      let _ = (&self.session, request, output_dir);
      Err("app probe capabilities are currently available only on macOS".to_string())
    }
  }
}

pub(crate) fn invoke_probe_step<B: AppProbeBackend>(
  backend: &mut B,
  output_dir: &Path,
  request: AppProbeStepRequest,
  allow_failure: bool,
) -> AuvResult<AppProbeStep> {
  let context = Context::current();
  let run_id = context.run_id().map(ToString::to_string).unwrap_or_default();
  let span_id = context.span_id().map(ToString::to_string).unwrap_or_default();
  match backend.perform(&request, output_dir) {
    Ok(output) => Ok(AppProbeStep {
      id: request.id.to_string(),
      command_id: request.command_id.to_string(),
      target_application_id: request.target_application_id,
      inputs: request.inputs,
      run_id,
      span_id,
      status: RunStatus::Completed.as_str().to_string(),
      output_summary: output.summary,
      artifact_paths: output.artifact_paths,
      // NOTICE: These are app-local probe files, not canonical tracing
      // artifacts. `artifact_paths` remains their sole authority.
      artifacts: Vec::new(),
      failure_message: None,
    }),
    Err(error) if allow_failure => Ok(AppProbeStep {
      id: request.id.to_string(),
      command_id: request.command_id.to_string(),
      target_application_id: request.target_application_id,
      inputs: request.inputs,
      run_id,
      span_id,
      status: RunStatus::Failed.as_str().to_string(),
      output_summary: format!("Probe step {} failed", request.id),
      artifact_paths: Vec::new(),
      artifacts: Vec::new(),
      failure_message: Some(error),
    }),
    Err(error) => Err(format!("probe step {} ({}) failed: {error}", request.id, request.command_id)),
  }
}

pub(crate) fn resolve_app_identity(bundle_id: &str) -> AuvResult<AppIdentity> {
  let escaped_bundle_id = bundle_id.replace('"', "\\\"");
  let mut resolution_notes = Vec::new();
  let launch_services_path = match resolve_launch_services_app_path(&escaped_bundle_id) {
    Ok(path) => Some(path),
    Err(error) => {
      resolution_notes.push(format!("LaunchServices could not resolve `{bundle_id}` to an application path: {error}"));
      None
    }
  };
  let app_path = launch_services_path.clone().or_else(|| match resolve_spotlight_app_path(bundle_id) {
    Ok(path) => Some(path),
    Err(error) => {
      resolution_notes.push(format!("Spotlight could not resolve `{bundle_id}` to an installed app bundle: {error}"));
      None
    }
  });
  let launch_services_resolved = launch_services_path.is_some();
  let info = app_path.as_ref().map(|path| read_app_info_plist(path)).transpose()?;
  let app_name = first_non_empty_string(&[
    info.as_ref().and_then(|info| json_string(info, "CFBundleDisplayName")),
    info.as_ref().and_then(|info| json_string(info, "CFBundleName")),
    app_path.as_ref().and_then(|path| path.file_stem().and_then(|value| value.to_str()).map(str::to_string)),
  ])
  .unwrap_or_else(|| bundle_id.to_string());
  let version = info.as_ref().and_then(|info| json_string(info, "CFBundleShortVersionString")).unwrap_or_else(|| "unknown".to_string());
  let build_version = info.as_ref().and_then(|info| json_string(info, "CFBundleVersion")).unwrap_or_else(|| "unknown".to_string());
  let main_executable_path = info
    .as_ref()
    .and_then(|info| json_string(info, "CFBundleExecutable"))
    .and_then(|value| app_path.as_ref().map(|path| path.join("Contents/MacOS").join(value)));
  let url_schemes = info
    .as_ref()
    .and_then(|info| info.get("CFBundleURLTypes"))
    .and_then(Value::as_array)
    .map(|entries| {
      entries
        .iter()
        .filter_map(|entry| entry.get("CFBundleURLSchemes"))
        .filter_map(Value::as_array)
        .flat_map(|schemes| schemes.iter())
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default();
  let apple_script_addressable = run_command_capture(
    "osascript",
    &[
      "-e",
      &format!("tell application id \"{escaped_bundle_id}\" to get name"),
    ],
  )
  .is_ok();
  Ok(AppIdentity {
    bundle_id: bundle_id.to_string(),
    app_name,
    app_path,
    main_executable_path,
    version,
    build_version,
    url_schemes,
    apple_script_addressable,
    launch_services_resolved,
    resolution_notes,
  })
}

fn resolve_launch_services_app_path(escaped_bundle_id: &str) -> AuvResult<PathBuf> {
  let script = format!("POSIX path of (path to application id \"{escaped_bundle_id}\")");
  Ok(PathBuf::from(run_command_capture("osascript", &["-e", &script])?.trim()))
}

fn resolve_spotlight_app_path(bundle_id: &str) -> AuvResult<PathBuf> {
  let query = format!("kMDItemCFBundleIdentifier == \"{bundle_id}\"");
  let raw = run_command_capture("mdfind", &[&query])?;
  raw
    .lines()
    .map(str::trim)
    .find(|line| !line.is_empty())
    .map(PathBuf::from)
    .ok_or_else(|| format!("no Spotlight match for bundle id `{bundle_id}`"))
}

fn read_app_info_plist(app_path: &Path) -> AuvResult<Value> {
  let path = app_path.join("Contents/Info.plist");
  let raw = run_command_capture(
    "plutil",
    &[
      "-convert",
      "json",
      "-o",
      "-",
      path.to_str().ok_or_else(|| format!("non-utf8 Info.plist path {}", path.display()))?,
    ],
  )?;
  serde_json::from_str(&raw).map_err(|error| format!("failed to parse Info.plist JSON for {}: {error}", app_path.display()))
}

pub(crate) fn default_probe_output_dir(project_root: &Path, bundle_id: &str) -> PathBuf {
  project_root.join(".auv").join("app-probes").join(format!("{}-{}", sanitized_name(bundle_id), now_millis()))
}

pub(crate) fn resolve_probe_path(query: &Path) -> AuvResult<PathBuf> {
  if query.is_file() {
    return Ok(query.to_path_buf());
  }
  if query.is_dir() {
    let candidate = query.join("probe.json");
    return candidate
      .exists()
      .then_some(candidate)
      .ok_or_else(|| format!("probe directory {} does not contain probe.json", query.display()));
  }
  Err(format!("probe path does not exist: {}", query.display()))
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> AuvResult<T> {
  let raw = fs::read_to_string(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

pub(crate) fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> AuvResult<()> {
  let rendered =
    serde_json::to_string_pretty(value).map_err(|error| format!("failed to serialize JSON output {}: {error}", path.display()))?;
  fs::write(path, rendered).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub(crate) fn run_command_capture(binary: &str, args: &[&str]) -> AuvResult<String> {
  let output = Command::new(binary).args(args).output().map_err(|error| format!("failed to launch {binary}: {error}"))?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    return Err(format!(
      "{} {:?} exited with status {}{}{}",
      binary,
      args,
      output.status,
      if stderr.is_empty() { "" } else { "; stderr=" },
      if stderr.is_empty() {
        stdout.as_str()
      } else {
        stderr.as_str()
      }
    ));
  }
  String::from_utf8(output.stdout)
    .map(|value| value.trim().to_string())
    .map_err(|error| format!("{binary} produced non-utf8 stdout: {error}"))
}

pub(crate) fn json_string(value: &Value, key: &str) -> Option<String> {
  value.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn first_non_empty_string(values: &[Option<String>]) -> Option<String> {
  values.iter().find_map(|value| {
    let value = value.as_deref()?.trim();
    (!value.is_empty()).then(|| value.to_string())
  })
}

pub(crate) fn sanitized_name(value: &str) -> String {
  let sanitized = value
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
        character
      } else {
        '_'
      }
    })
    .collect::<String>();
  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

#[cfg(target_os = "macos")]
fn perform_macos_probe_step(
  session: &auv_driver::LocalDriverSession,
  request: &AppProbeStepRequest,
  output_dir: &Path,
) -> AuvResult<AppProbeStepOutput> {
  match request.operation {
    AppProbeOperation::ProbePermissions => probe_permissions(session, output_dir),
    AppProbeOperation::ListDisplays => list_displays(session, output_dir),
    AppProbeOperation::ActivateTargetApp => activate_target_app(session, request),
    AppProbeOperation::ListWindows => list_windows(session, request, output_dir),
    AppProbeOperation::CaptureAxTree => capture_ax_tree(session, request, output_dir),
    AppProbeOperation::CaptureWindow => capture_window(session, request, output_dir),
    AppProbeOperation::OcrSample => sample_ocr(session, request, output_dir),
  }
}

#[cfg(target_os = "macos")]
fn probe_permissions(session: &auv_driver::LocalDriverSession, output_dir: &Path) -> AuvResult<AppProbeStepOutput> {
  let permissions = session.permission().probe().map_err(|error| format!("failed to probe macOS permissions: {error}"))?;
  let launch_host = std::env::current_exe()
    .ok()
    .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
    .unwrap_or_else(|| "unknown".to_string());
  let report = auv_driver_macos::observe::permission_probe_report(
    permissions.screen_recording.as_str(),
    permissions.screen_capture_kit.as_str(),
    permissions.accessibility.as_str(),
    permissions.automation_to_system_events.as_str(),
    &launch_host,
  );
  let path = output_dir.join("permission-probe.txt");
  fs::write(&path, report).map_err(|error| format!("failed to write permission probe {}: {error}", path.display()))?;
  Ok(AppProbeStepOutput {
    summary: "Probed macOS automation permissions".to_string(),
    artifact_paths: vec![path],
  })
}

#[cfg(target_os = "macos")]
fn list_displays(session: &auv_driver::LocalDriverSession, output_dir: &Path) -> AuvResult<AppProbeStepOutput> {
  let displays = session.display().list().map_err(|error| format!("failed to list displays: {error}"))?;
  let descriptors = displays
    .displays
    .iter()
    .map(|display| {
      let frame = &display.frame;
      serde_json::json!({
        "display_ref": display.name.as_deref().unwrap_or(&display.id),
        "native_display_id": display.id,
        "is_main": display.is_primary,
        "is_builtin": display.is_builtin.unwrap_or(false),
        "global_logical_bounds": {
          "x": frame.origin.x,
          "y": frame.origin.y,
          "width": frame.size.width,
          "height": frame.size.height,
        },
        // NOTICE: The typed display API currently exposes one logical frame;
        // use it for visible bounds until it gains a distinct work-area field.
        "visible_logical_bounds": {
          "x": frame.origin.x,
          "y": frame.origin.y,
          "width": frame.size.width,
          "height": frame.size.height,
        },
        "physical_pixel_size": {
          "width": frame.size.width * display.scale_factor,
          "height": frame.size.height * display.scale_factor,
        },
        "scale_factor": display.scale_factor,
      })
    })
    .collect::<Vec<_>>();
  if descriptors.is_empty() {
    return Err("display probe returned no displays".to_string());
  }
  let path = output_dir.join("display-list.json");
  write_pretty_json(&path, &descriptors)?;
  Ok(AppProbeStepOutput {
    summary: format!("Observed {} display(s)", descriptors.len()),
    artifact_paths: vec![path],
  })
}

#[cfg(target_os = "macos")]
fn activate_target_app(session: &auv_driver::LocalDriverSession, request: &AppProbeStepRequest) -> AuvResult<AppProbeStepOutput> {
  let bundle_id = required_target_application_id(request)?;
  let settle_ms = input_u64(request, "settle_ms", 250)?;
  activate_bundle_id(session, bundle_id, Duration::from_millis(settle_ms))?;
  Ok(AppProbeStepOutput {
    summary: format!("Activated {bundle_id}"),
    artifact_paths: Vec::new(),
  })
}

#[cfg(target_os = "macos")]
fn activate_bundle_id(session: &auv_driver::LocalDriverSession, bundle_id: &str, settle: Duration) -> AuvResult<()> {
  use auv_driver_macos::ApplicationControl;

  match session {
    auv_driver::LocalDriverSession::Macos(session) => {
      session.activate_bundle_id(bundle_id, settle).map_err(|error| format!("failed to activate app {bundle_id}: {error}"))?
    }
  }
  Ok(())
}

#[cfg(target_os = "macos")]
fn list_windows(
  session: &auv_driver::LocalDriverSession,
  request: &AppProbeStepRequest,
  output_dir: &Path,
) -> AuvResult<AppProbeStepOutput> {
  let limit = input_usize(request, "limit", 20)?;
  let target = request.target_application_id.as_deref().filter(|value| !value.trim().is_empty());
  let windows = session.window().list().map_err(|error| format!("failed to list windows: {error}"))?;
  let windows = windows
    .into_iter()
    .filter(|window| target.is_none_or(|bundle_id| window.app_bundle_id.as_deref() == Some(bundle_id)))
    .take(limit)
    .collect::<Vec<_>>();
  let mut report = vec![
    format!("observedAt={}", now_millis()),
    "frontmostAppName=".to_string(),
    "frontmostAppBundleId=".to_string(),
    "frontmostWindowTitle=".to_string(),
    format!("windowCount={}", windows.len()),
  ];
  report.extend(windows.iter().map(|window| {
    format!(
      "window\t{}\t{}\t{}\t{}\t0\t{}\t{}\t{}\t{}\t{}",
      report_field(window.app_name.as_deref().unwrap_or("")),
      window.process_id.unwrap_or_default(),
      report_field(window.app_bundle_id.as_deref().unwrap_or("")),
      window.reference.id.parse::<i64>().unwrap_or_default(),
      report_field(window.title.as_deref().unwrap_or("")),
      window.frame.origin.x.round() as i64,
      window.frame.origin.y.round() as i64,
      window.frame.size.width.round() as i64,
      window.frame.size.height.round() as i64,
    )
  }));
  let path = output_dir.join("window-list.txt");
  fs::write(&path, report.join("\n") + "\n").map_err(|error| format!("failed to write window list {}: {error}", path.display()))?;
  Ok(AppProbeStepOutput {
    summary: format!("Observed {} matching window(s)", windows.len()),
    artifact_paths: vec![path],
  })
}

#[cfg(target_os = "macos")]
fn capture_ax_tree(
  session: &auv_driver::LocalDriverSession,
  request: &AppProbeStepRequest,
  output_dir: &Path,
) -> AuvResult<AppProbeStepOutput> {
  let target = required_target_application_id(request)?;
  let max_depth = input_i64(request, "max_depth", 6)?;
  let max_children = input_i64(request, "max_children", 24)?;
  let snapshot = session
    .accessibility()
    .capture_app_tree(target, max_depth, max_children)
    .map_err(|error| format!("failed to capture AX tree for {target}: {error}"))?;
  let root_role = snapshot.nodes.first().map(|node| node.role.as_str()).unwrap_or("");
  let mut report = vec![
    format!("observedAt={}", snapshot.observed_at),
    format!("appName={}", report_field(&snapshot.app_name)),
    format!("bundleId={}", report_field(&snapshot.bundle_id)),
    format!("pid={}", snapshot.pid),
    format!("windowTitle={}", report_field(&snapshot.window_title)),
    format!("rootRole={}", report_field(root_role)),
  ];
  report.extend(snapshot.nodes.iter().map(|node| {
    format!(
      "node\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      node.depth,
      report_field(&node.path),
      report_field(&node.role),
      report_field(&node.subrole),
      report_field(&node.title),
      report_field(&node.description),
      report_field(&node.help),
      report_field(&node.identifier),
      report_field(&node.placeholder),
      report_field(&node.value),
      node.focused,
      node.bounds.x,
      node.bounds.y,
      node.bounds.width,
      node.bounds.height,
    )
  }));
  report.push(format!("nodeCount={}", snapshot.nodes.len()));
  let path = output_dir.join("ax-tree.txt");
  fs::write(&path, report.join("\n") + "\n").map_err(|error| format!("failed to write AX tree {}: {error}", path.display()))?;
  Ok(AppProbeStepOutput {
    summary: format!("Captured {} AX node(s)", snapshot.nodes.len()),
    artifact_paths: vec![path],
  })
}

#[cfg(target_os = "macos")]
fn capture_window(
  session: &auv_driver::LocalDriverSession,
  request: &AppProbeStepRequest,
  output_dir: &Path,
) -> AuvResult<AppProbeStepOutput> {
  if input_bool(request, "activate_target_before_capture", false)? {
    let bundle_id = required_target_application_id(request)?;
    activate_bundle_id(session, bundle_id, Duration::from_millis(250))?;
  }
  let window = resolve_probe_window(session, request)?;
  let capture = session.window().capture(&window).map_err(|error| format!("failed to capture probe window: {error}"))?;
  let label = input(request, "label").unwrap_or("app-probe-capture");
  let path = output_dir.join(format!("{}.png", sanitized_name(label)));
  capture.image.save(&path).map_err(|error| format!("failed to write window capture {}: {error}", path.display()))?;
  Ok(AppProbeStepOutput {
    summary: format!("Captured {}x{} window image", capture.image.width(), capture.image.height()),
    artifact_paths: vec![path],
  })
}

#[cfg(target_os = "macos")]
fn sample_ocr(session: &auv_driver::LocalDriverSession, request: &AppProbeStepRequest, output_dir: &Path) -> AuvResult<AppProbeStepOutput> {
  let window = resolve_probe_window(session, request)?;
  let capture = session.window().capture(&window).map_err(|error| format!("failed to capture OCR sample window: {error}"))?;
  let region = auv_driver::RatioRect::new(
    input_f64(request, "region_left_ratio", 0.0)?,
    input_f64(request, "region_top_ratio", 0.0)?,
    input_f64(request, "region_right_ratio", 1.0)? - input_f64(request, "region_left_ratio", 0.0)?,
    input_f64(request, "region_bottom_ratio", 1.0)? - input_f64(request, "region_top_ratio", 0.0)?,
  );
  let recognition =
    session.vision().recognize_text_in_capture(&capture, region).map_err(|error| format!("failed to recognize OCR sample: {error}"))?;
  let query = input(request, "query").unwrap_or("");
  let normalized_query = query.to_ascii_lowercase();
  let min_confidence = input_f64(request, "min_confidence", 0.55)?;
  let max_observations = input_usize(request, "max_observations", 20)?;
  let matches = recognition
    .regions
    .iter()
    .filter(|region| region.confidence.unwrap_or_default() as f64 >= min_confidence)
    .filter(|region| normalized_query.is_empty() || region.text.to_ascii_lowercase().contains(&normalized_query))
    .take(max_observations)
    .collect::<Vec<_>>();
  let label = input(request, "label").unwrap_or("app-probe-ocr-sample");
  let image_path = output_dir.join(format!("{}.png", sanitized_name(label)));
  capture.image.save(&image_path).map_err(|error| format!("failed to write OCR sample image {}: {error}", image_path.display()))?;
  let mut report = vec![
    format!("recognizedAt={}", now_millis()),
    format!("imagePath={}", image_path.display()),
    format!("imageWidth={}", capture.image.width()),
    format!("imageHeight={}", capture.image.height()),
    format!("query={}", report_field(query)),
    "exact=false".to_string(),
    "caseSensitive=false".to_string(),
  ];
  report.extend(matches.iter().enumerate().map(|(index, region)| {
    format!(
      "match\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      index,
      report_field(&region.text),
      region.confidence.unwrap_or_default(),
      region.bounds.origin.x.round() as i64,
      region.bounds.origin.y.round() as i64,
      region.bounds.size.width.round() as i64,
      region.bounds.size.height.round() as i64,
    )
  }));
  report.push(format!("matchCount={}", matches.len()));
  let report_path = output_dir.join("ocr-sample.txt");
  fs::write(&report_path, report.join("\n") + "\n")
    .map_err(|error| format!("failed to write OCR sample report {}: {error}", report_path.display()))?;
  Ok(AppProbeStepOutput {
    summary: format!("Observed {} OCR match(es) for {query:?}", matches.len()),
    artifact_paths: vec![image_path, report_path],
  })
}

#[cfg(target_os = "macos")]
fn resolve_probe_window(session: &auv_driver::LocalDriverSession, request: &AppProbeStepRequest) -> AuvResult<auv_driver::Window> {
  let mut selector = auv_driver::WindowSelector {
    main_visible: true,
    ..auv_driver::WindowSelector::default()
  };
  if let Some(bundle_id) = request.target_application_id.as_deref().filter(|value| !value.trim().is_empty()) {
    selector = selector.owned_by(auv_driver::App::bundle_id(bundle_id));
  }
  if let Some(title) = input(request, "title").filter(|value| !value.trim().is_empty()) {
    selector = selector.title_contains(title);
  }
  session.window().resolve(selector).map_err(|error| format!("failed to resolve app-probe window: {error}"))
}

fn required_target_application_id(request: &AppProbeStepRequest) -> AuvResult<&str> {
  request
    .target_application_id
    .as_deref()
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("probe step {} requires a non-empty target application id", request.id))
}

fn input<'a>(request: &'a AppProbeStepRequest, key: &str) -> Option<&'a str> {
  request.inputs.get(key).map(String::as_str)
}

fn input_u64(request: &AppProbeStepRequest, key: &str, default: u64) -> AuvResult<u64> {
  input(request, key).map_or(Ok(default), |value| value.parse::<u64>().map_err(|error| format!("invalid {key}={value:?}: {error}")))
}

fn input_bool(request: &AppProbeStepRequest, key: &str, default: bool) -> AuvResult<bool> {
  input(request, key).map_or(Ok(default), |value| value.parse::<bool>().map_err(|error| format!("invalid {key}={value:?}: {error}")))
}

fn input_i64(request: &AppProbeStepRequest, key: &str, default: i64) -> AuvResult<i64> {
  input(request, key).map_or(Ok(default), |value| value.parse::<i64>().map_err(|error| format!("invalid {key}={value:?}: {error}")))
}

fn input_usize(request: &AppProbeStepRequest, key: &str, default: usize) -> AuvResult<usize> {
  input(request, key).map_or(Ok(default), |value| value.parse::<usize>().map_err(|error| format!("invalid {key}={value:?}: {error}")))
}

fn input_f64(request: &AppProbeStepRequest, key: &str, default: f64) -> AuvResult<f64> {
  let value =
    input(request, key).map_or(Ok(default), |value| value.parse::<f64>().map_err(|error| format!("invalid {key}={value:?}: {error}")))?;
  value.is_finite().then_some(value).ok_or_else(|| format!("invalid {key}: expected a finite number"))
}

fn report_field(value: &str) -> String {
  value.replace(['\t', '\n', '\r'], " ")
}
