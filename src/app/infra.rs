// File: src/app/infra.rs
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{AuvResult, ExecutionTarget, InvokeRequest, RunStatus, now_millis};
use crate::runtime::Runtime;
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::run_builder::{RecordingRun, RunFinish, SpanFinish, SpanRef};
use auv_tracing_driver::store::sanitized_artifact_name;
use auv_tracing_driver::trace::{SPAN_API_VERSION, SpanRecordV1Alpha1, TraceState, TraceStatusCode, new_span_id, string_attr};

use super::{AppIdentity, AppProbeArtifact, AppProbeStep};

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
  let info = app_path.as_ref().map(|app_path| read_app_info_plist(app_path.as_path())).transpose()?;
  let app_name = first_non_empty_string(&[
    info.as_ref().and_then(|info| json_string(info, "CFBundleDisplayName")),
    info.as_ref().and_then(|info| json_string(info, "CFBundleName")),
    app_path.as_ref().and_then(|app_path| app_path.file_stem().and_then(|value| value.to_str()).map(|value| value.to_string())),
  ])
  .unwrap_or_else(|| bundle_id.to_string());
  let version = info.as_ref().and_then(|info| json_string(info, "CFBundleShortVersionString")).unwrap_or_else(|| "unknown".to_string());
  let build_version = info.as_ref().and_then(|info| json_string(info, "CFBundleVersion")).unwrap_or_else(|| "unknown".to_string());
  let main_executable_path = info
    .as_ref()
    .and_then(|info| json_string(info, "CFBundleExecutable"))
    .and_then(|value| app_path.as_ref().map(|app_path| app_path.join("Contents/MacOS").join(value)));
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
        .map(ToString::to_string)
        .collect::<Vec<_>>()
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
  let path_script = format!("POSIX path of (path to application id \"{escaped_bundle_id}\")");
  let app_path_raw = run_command_capture("osascript", &["-e", &path_script])?;
  Ok(PathBuf::from(app_path_raw.trim()))
}

fn resolve_spotlight_app_path(bundle_id: &str) -> AuvResult<PathBuf> {
  let query = format!("kMDItemCFBundleIdentifier == \"{bundle_id}\"");
  let raw = run_command_capture("mdfind", &[&query])?;
  let candidate =
    raw.lines().map(str::trim).find(|line| !line.is_empty()).ok_or_else(|| format!("no Spotlight match for bundle id `{bundle_id}`"))?;
  Ok(PathBuf::from(candidate))
}

fn read_app_info_plist(app_path: &Path) -> AuvResult<Value> {
  let info_plist_path = app_path.join("Contents/Info.plist");
  let info_json = run_command_capture(
    "plutil",
    &[
      "-convert",
      "json",
      "-o",
      "-",
      info_plist_path.to_str().ok_or_else(|| format!("non-utf8 Info.plist path {}", info_plist_path.display()))?,
    ],
  )?;
  serde_json::from_str(&info_json).map_err(|error| format!("failed to parse Info.plist JSON for {}: {error}", app_path.display()))
}

pub(crate) fn default_probe_output_dir(project_root: &Path, bundle_id: &str) -> PathBuf {
  project_root.join(".auv").join("app-probes").join(format!("{}-{}", sanitized_artifact_name(bundle_id), now_millis()))
}

pub(crate) fn invoke_probe_step(
  runtime: &Runtime,
  run: &mut RecordingRun,
  parent: &SpanRef,
  step_id: &str,
  command_id: &str,
  target_application_id: Option<String>,
  inputs: BTreeMap<String, String>,
  allow_failure: bool,
) -> AuvResult<AppProbeStep> {
  let step_span = run.start_span(
    parent,
    app_span_record(
      "auv.probe.step",
      BTreeMap::from([
        ("auv.probe.step_id".to_string(), string_attr(step_id)),
        ("auv.step.id".to_string(), string_attr(step_id)),
        ("auv.step.kind".to_string(), string_attr("probe")),
      ]),
    ),
  )?;
  let request = InvokeRequest {
    command_id: command_id.to_string(),
    target: ExecutionTarget {
      application_id: target_application_id.clone(),
      target_label: None,
    },
    inputs: inputs.clone(),
    dry_run: false,
  };
  let registry = auv_cli_invoke::default_registry();
  let result = match auv_cli_invoke::invoke_recorded_in_span(runtime.recording(), &registry, run, &step_span, request) {
    Ok(result) => result,
    Err(error) => {
      if let Err(finish_error) = run.finish_span(
        &step_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!("Probe step {step_id} failed")),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!("{error}; additionally failed to finish failed probe step span: {finish_error}"));
      }
      if !allow_failure {
        return Err(error.clone());
      }
      return Ok(AppProbeStep {
        id: step_id.to_string(),
        command_id: command_id.to_string(),
        target_application_id,
        inputs,
        run_id: run.id().to_string(),
        span_id: step_span.id().to_string(),
        status: RunStatus::Failed.as_str().to_string(),
        output_summary: format!("Probe step {step_id} failed"),
        artifact_paths: Vec::new(),
        artifacts: Vec::new(),
        failure_message: Some(error),
      });
    }
  };
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  run.finish_span(
    &step_span,
    SpanFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  if result.status != RunStatus::Completed && !allow_failure {
    return Err(format!(
      "probe step {} ({}) failed: {}",
      step_id,
      command_id,
      result.failure_message.clone().unwrap_or_else(|| result.output_summary.clone())
    ));
  }
  let artifact_paths = result.artifact_paths.clone();
  Ok(AppProbeStep {
    id: step_id.to_string(),
    command_id: command_id.to_string(),
    target_application_id,
    inputs,
    run_id: run.id().to_string(),
    span_id: step_span.id().to_string(),
    status: result.status.as_str().to_string(),
    output_summary: result.output_summary,
    artifact_paths: artifact_paths.clone(),
    artifacts: result
      .artifacts
      .iter()
      .zip(artifact_paths.iter())
      .map(|(artifact, path)| AppProbeArtifact {
        artifact_id: artifact.artifact_id.as_str().to_string(),
        span_id: artifact.span_id.as_str().to_string(),
        path: path.clone(),
        role: artifact.role.clone(),
        captured_event_id: artifact.event_id.as_ref().map(|event_id| event_id.as_str().to_string()),
      })
      .collect(),
    failure_message: result.failure_message,
  })
}

pub(crate) fn resolve_probe_path(query: &Path) -> AuvResult<PathBuf> {
  if query.is_file() {
    return Ok(query.to_path_buf());
  }
  if query.is_dir() {
    let candidate = query.join("probe.json");
    if candidate.exists() {
      return Ok(candidate);
    }
    return Err(format!("probe directory {} does not contain probe.json", query.display()));
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

pub(crate) fn stage_app_artifact(
  recording: &RecordingHandle,
  run: &mut RecordingRun,
  span: &SpanRef,
  role: &str,
  path: &Path,
  preferred_name: &str,
) -> AuvResult<()> {
  recording.stage_artifact_file(run, span, role, path, preferred_name, Some(format!("Generated app workflow artifact {role}")))?;
  Ok(())
}

pub(crate) fn finish_failed_app_run<T>(recording: &RecordingHandle, run: RecordingRun, error: String, summary: String) -> AuvResult<T> {
  if let Err(finish_error) = recording.finish_run(
    run,
    RunFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(summary),
      failure: Some(error.clone()),
    },
  ) {
    return Err(format!("{error}; additionally failed to persist failed workflow run: {finish_error}"));
  }
  Err(error)
}

pub(crate) fn app_span_record(name: impl Into<String>, attributes: auv_tracing_driver::run_builder::Attributes) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.into(),
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
}

pub(crate) fn run_command_capture(binary: &str, args: &[&str]) -> AuvResult<String> {
  let output = Command::new(binary).args(args).output().map_err(|error| format!("failed to launch {}: {error}", binary))?;
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
    .map_err(|error| format!("{} produced non-utf8 stdout: {error}", binary))
}

pub(crate) fn json_string(value: &Value, key: &str) -> Option<String> {
  value.get(key).and_then(Value::as_str).map(ToString::to_string)
}

pub(crate) fn first_non_empty_string(values: &[Option<String>]) -> Option<String> {
  values.iter().find_map(|value| {
    let value = value.as_deref()?.trim();
    if value.is_empty() {
      None
    } else {
      Some(value.to_string())
    }
  })
}
