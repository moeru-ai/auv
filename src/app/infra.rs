use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{AuvResult, now_millis};

use super::AppIdentity;

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

fn sanitized_name(value: &str) -> String {
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
