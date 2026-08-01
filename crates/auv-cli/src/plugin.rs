//! Discovery and execution of external `auv-*` command plugins.

use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use auv_api_client::AuvContext;
use auv_api_proto::auv::api::core::v1::Device;
#[cfg(test)]
use auv_api_proto::auv::api::core::v1::DeviceRef;

use crate::cli::ParentContextOptions;

const BUILTIN_COMMANDS: &[&str] = &[
  "doctor",
  "invoke",
  "api-server",
  "serve",
  "devices",
  "run",
  "runner",
  "mcp",
  "plugin",
];

pub async fn execute(command_name: &OsStr, arguments: &[OsString], parent_context: &ParentContextOptions) -> Result<i32, String> {
  let executable_name = executable_name(command_name);
  let executable = resolve(&executable_name)
    .ok_or_else(|| format!("unknown command {:?}; no {:?} executable was found on PATH", command_name, executable_name))?;
  let auv_path = env::current_exe().map_err(|error| format!("failed to resolve the auv executable path: {error}"))?;
  let resolved = resolve_context(parent_context).await?;
  let context = serde_json::to_string(&resolved.context).map_err(|error| format!("failed to encode AUV_CONTEXT: {error}"))?;

  let exit = execute_resolved(&executable, arguments, &auv_path, &context, resolved.implicit_run_id.is_none())?;
  resolved.finish(exit == 0).await?;
  Ok(exit)
}

#[cfg(unix)]
fn execute_resolved(
  executable: &Path,
  arguments: &[OsString],
  auv_path: &Path,
  context: &str,
  replace_process: bool,
) -> Result<i32, String> {
  use std::os::unix::process::{CommandExt, ExitStatusExt};

  let mut command = Command::new(executable);
  command.args(arguments).env("AUV_CONTEXT", context).env("AUV_PATH", auv_path);
  if replace_process {
    let error = command.exec();
    return Err(format!("failed to execute plugin {}: {error}", executable.display()));
  }
  // TODO(plugin-signal-forwarding): an implicitly owned Run requires the root
  // process to wait and close it after ordinary child exit. Add explicit
  // signal forwarding plus a bounded cleanup path before claiming terminal Run
  // cleanup for abrupt root/plugin termination.
  let status = command.status().map_err(|error| format!("failed to execute plugin {}: {error}", executable.display()))?;
  Ok(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
}

#[cfg(windows)]
fn execute_resolved(
  executable: &Path,
  arguments: &[OsString],
  auv_path: &Path,
  context: &str,
  _replace_process: bool,
) -> Result<i32, String> {
  let status = Command::new(executable)
    .args(arguments)
    .env("AUV_CONTEXT", context)
    .env("AUV_PATH", auv_path)
    .status()
    .map_err(|error| format!("failed to execute plugin {}: {error}", executable.display()))?;
  Ok(status.code().unwrap_or(1))
}

pub(crate) struct ResolvedExecutionContext {
  pub(crate) context: AuvContext,
  pub(crate) implicit_run_id: Option<String>,
}

impl ResolvedExecutionContext {
  pub(crate) async fn finish(self, succeeded: bool) -> Result<(), String> {
    let Some(run_id) = self.implicit_run_id else {
      return Ok(());
    };
    let outcome = if succeeded {
      auv_api_proto::auv::api::core::v1::RunOutcome::Succeeded
    } else {
      auv_api_proto::auv::api::core::v1::RunOutcome::Failed
    };
    let mut client = auv_api_client::Client::from_context(self.context).await.map_err(|error| error.to_string())?;
    client.stop_run(run_id, outcome).await.map_err(|status| format!("failed to stop the implicit Run: {status}"))?;
    Ok(())
  }
}

async fn resolve_context(parent: &ParentContextOptions) -> Result<ResolvedExecutionContext, String> {
  let context = AuvContext {
    invocation_id: Some(format!("invocation_{}", uuid::Uuid::now_v7())),
    ..AuvContext::default()
  };
  if parent == &ParentContextOptions::default() {
    // TODO(implicit-plugin-run): create a one-shot Run for unqualified plugin
    // calls once the frontend-owned implicit Run lifecycle is approved. This
    // slice keeps daemon-free plugins usable while resolving every explicit
    // Device/Run selection through the daemon.
    return Ok(ResolvedExecutionContext {
      context,
      implicit_run_id: None,
    });
  }

  resolve_selected_context(parent, None, true, context).await
}

pub(crate) async fn resolve_invoke_context(parent: &ParentContextOptions) -> Result<ResolvedExecutionContext, String> {
  resolve_selected_context(parent, None, true, AuvContext::default()).await
}

pub(crate) async fn resolve_builtin_context(parent: &ParentContextOptions, endpoint: Option<&str>) -> Result<AuvContext, String> {
  if parent == &ParentContextOptions::default() {
    return Ok(AuvContext::default());
  }
  Ok(resolve_selected_context(parent, endpoint, false, AuvContext::default()).await?.context)
}

async fn resolve_selected_context(
  parent: &ParentContextOptions,
  explicit_endpoint: Option<&str>,
  create_implicit_run: bool,
  mut context: AuvContext,
) -> Result<ResolvedExecutionContext, String> {
  context.device_id = parent.device_id.clone();
  context.device_name = parent.device_name.clone();
  context.run_id = parent.run_id.clone();
  if let Some(endpoint) = explicit_endpoint {
    context.daemon_endpoint = Some(endpoint.to_string());
  }
  let mut client = auv_api_client::Client::from_context(context).await.map_err(|error| error.to_string())?;
  let mut context = client.context().cloned().unwrap_or_default();
  if create_implicit_run {
    let auv = client.placement();
    let run = auv
      .run(auv_api_client::placement::RunOptions {
        selection: parent.run_id.clone().map(auv_api_client::placement::RunSelection::Existing).unwrap_or_default(),
        device: auv_api_client::placement::DeviceSelector {
          id: parent.device_id.clone(),
          name: parent.device_name.clone(),
        },
        labels: Default::default(),
      })
      .await
      .map_err(|error| error.to_string())?;
    context.device_id = run.device().and_then(|device| device.r#ref.as_ref()).map(|reference| reference.device_id.clone());
    context.device_name = run.device().and_then(|device| (!device.name.is_empty()).then(|| device.name.clone()));
    context.run_id = run.resource().r#ref.as_ref().map(|reference| reference.run_id.clone());
    let implicit_run_id = run.is_owned().then(|| context.run_id.clone()).flatten();
    return Ok(ResolvedExecutionContext {
      context,
      implicit_run_id,
    });
  }
  let devices = client.list_devices().await.map_err(|status| format!("ListDevices failed: {status}"))?;
  let run = match parent.run_id.as_deref() {
    Some(run_id) => Some(client.get_run(run_id).await.map_err(|status| format!("Run {run_id:?} is not available: {status}"))?),
    None => None,
  };
  let explicit_device = parent.device_id.is_some() || parent.device_name.is_some();
  let selected_device = if explicit_device || run.is_none() {
    Some(select_device(&devices, parent)?)
  } else {
    None
  };
  let selected_device_id = match selected_device {
    Some(device) => Some(
      device
        .r#ref
        .as_ref()
        .map(|reference| reference.device_id.clone())
        .ok_or_else(|| "selected Device omitted its stable ID".to_string())?,
    ),
    None => run.as_ref().and_then(inherited_run_device_id).map(str::to_string),
  };
  if let (Some(run), Some(device_id)) = (&run, &selected_device_id)
    && !run.devices.iter().any(|candidate| candidate.device_id == *device_id)
  {
    let run_id = run.r#ref.as_ref().map(|reference| reference.run_id.as_str()).unwrap_or("<missing>");
    return Err(format!("Run {run_id:?} does not include selected Device {device_id:?}"));
  }
  context.device_id = selected_device_id;
  context.device_name = selected_device.and_then(|device| (!device.name.is_empty()).then(|| device.name.clone()));
  context.run_id = run.and_then(|run| run.r#ref.map(|reference| reference.run_id));
  Ok(ResolvedExecutionContext {
    context,
    implicit_run_id: None,
  })
}

fn select_device<'a>(devices: &'a [Device], parent: &ParentContextOptions) -> Result<&'a Device, String> {
  let by_id = match parent.device_id.as_deref() {
    Some(device_id) => Some(
      devices
        .iter()
        .find(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == device_id))
        .ok_or_else(|| format!("unknown Device ID {device_id:?}"))?,
    ),
    None => None,
  };

  let by_name = match parent.device_name.as_deref() {
    Some(device_name) => {
      let matches = devices.iter().filter(|device| device.name == device_name).collect::<Vec<_>>();
      match matches.as_slice() {
        [] => return Err(format!("unknown Device name {device_name:?}")),
        [device] => Some(*device),
        matches => {
          let candidate_ids = matches
            .iter()
            .filter_map(|device| device.r#ref.as_ref().map(|reference| reference.device_id.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
          return Err(format!("Device name {device_name:?} is ambiguous; candidate IDs: {candidate_ids}"));
        }
      }
    }
    None => None,
  };

  match (by_id, by_name) {
    (Some(by_id), Some(by_name)) if !std::ptr::eq(by_id, by_name) => {
      Err(format!("--device and --device-id select different Devices ({:?} and {:?})", device_id(by_name), device_id(by_id)))
    }
    (Some(device), _) | (_, Some(device)) => Ok(device),
    (None, None) => {
      let local = devices.iter().filter(|device| device.local).collect::<Vec<_>>();
      match local.as_slice() {
        [device] => Ok(*device),
        [] => Err("the selected daemon exposes no implicit local Device".to_string()),
        _ => Err("the selected daemon exposes more than one implicit local Device".to_string()),
      }
    }
  }
}

fn device_id(device: &Device) -> &str {
  device.r#ref.as_ref().map(|reference| reference.device_id.as_str()).unwrap_or("<missing>")
}

fn inherited_run_device_id(run: &auv_api_proto::auv::api::core::v1::Run) -> Option<&str> {
  match run.devices.as_slice() {
    [device] => Some(device.device_id.as_str()),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn device(id: &str, name: &str, local: bool) -> Device {
    Device {
      r#ref: Some(DeviceRef {
        device_id: id.to_string(),
      }),
      name: name.to_string(),
      local,
      ..Device::default()
    }
  }

  #[test]
  fn duplicate_device_name_reports_candidate_ids() {
    let devices = [
      device("device_a", "studio", true),
      device("device_b", "studio", false),
    ];
    let error = select_device(
      &devices,
      &ParentContextOptions {
        device_name: Some("studio".to_string()),
        ..ParentContextOptions::default()
      },
    )
    .expect_err("duplicate name must be ambiguous");
    assert!(error.contains("ambiguous"));
    assert!(error.contains("device_a, device_b"));
  }

  #[test]
  fn device_name_and_id_must_select_the_same_device() {
    let devices = [
      device("device_a", "desktop", true),
      device("device_b", "laptop", false),
    ];
    let error = select_device(
      &devices,
      &ParentContextOptions {
        device_name: Some("desktop".to_string()),
        device_id: Some("device_b".to_string()),
        run_id: None,
      },
    )
    .expect_err("different selectors must fail");
    assert!(error.contains("select different Devices"));
  }

  #[test]
  fn run_only_context_inherits_one_device_but_leaves_multi_device_placement_to_the_scheduler() {
    let one = auv_api_proto::auv::api::core::v1::Run {
      devices: vec![DeviceRef {
        device_id: "device_remote".to_string(),
      }],
      ..Default::default()
    };
    assert_eq!(inherited_run_device_id(&one), Some("device_remote"));

    let multiple = auv_api_proto::auv::api::core::v1::Run {
      devices: vec![
        DeviceRef {
          device_id: "device_a".to_string(),
        },
        DeviceRef {
          device_id: "device_b".to_string(),
        },
      ],
      ..Default::default()
    };
    assert_eq!(inherited_run_device_id(&multiple), None);
  }
}

pub fn list() -> Result<i32, String> {
  let path = env::var_os("PATH").ok_or_else(|| "PATH is not set; no AUV plugins can be discovered".to_string())?;
  let mut seen = HashMap::<OsString, PathBuf>::new();
  let mut warnings = Vec::new();
  let mut plugins = Vec::new();

  for directory in env::split_paths(&path) {
    let Ok(entries) = fs::read_dir(&directory) else {
      continue;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
      let name = entry.file_name();
      let Some(command_name) = plugin_command_name(&name) else {
        continue;
      };
      let command_key = command_name.clone();
      let path = entry.path();
      if !is_executable(&path) {
        warnings.push(format!("{} is named like an AUV plugin but is not executable", path.display()));
        continue;
      }
      if let Some(visible) = seen.get(&command_key) {
        warnings.push(format!("{} is shadowed by {} earlier on PATH", path.display(), visible.display()));
        continue;
      }
      if BUILTIN_COMMANDS.iter().any(|builtin| command_name == OsStr::new(builtin)) {
        warnings.push(format!("{} collides with built-in command `{}`", path.display(), command_name.to_string_lossy()));
      }
      seen.insert(command_key, path.clone());
      plugins.push(path);
    }
  }

  if plugins.is_empty() {
    println!("No AUV plugins were found on PATH.");
  } else {
    println!("The following AUV-compatible plugins are available:");
    for plugin in plugins {
      println!("{}", plugin.display());
    }
  }
  for warning in &warnings {
    eprintln!("warning: {warning}");
  }

  Ok(i32::from(!warnings.is_empty()))
}

fn executable_name(command_name: &OsStr) -> OsString {
  let mut executable = OsString::from("auv-");
  executable.push(command_name);
  executable
}

#[cfg(unix)]
fn resolve(executable_name: &OsStr) -> Option<PathBuf> {
  let path = env::var_os("PATH")?;
  env::split_paths(&path).find_map(|directory| {
    let candidate = directory.join(executable_name);
    is_executable(&candidate).then_some(candidate)
  })
}

#[cfg(windows)]
fn resolve(executable_name: &OsStr) -> Option<PathBuf> {
  let path = env::var_os("PATH")?;
  let extensions = windows_executable_extensions();
  env::split_paths(&path).find_map(|directory| {
    let direct = directory.join(executable_name);
    if is_executable(&direct) {
      return Some(direct);
    }
    extensions.iter().find_map(|extension| {
      let mut candidate_name = executable_name.to_os_string();
      candidate_name.push(extension);
      let candidate = directory.join(candidate_name);
      is_executable(&candidate).then_some(candidate)
    })
  })
}

#[cfg(unix)]
fn plugin_command_name(file_name: &OsStr) -> Option<OsString> {
  use std::os::unix::ffi::{OsStrExt, OsStringExt};

  file_name.as_bytes().strip_prefix(b"auv-").map(|name| OsString::from_vec(name.to_vec()))
}

#[cfg(windows)]
fn plugin_command_name(file_name: &OsStr) -> Option<OsString> {
  let path = Path::new(file_name);
  path.extension()?;
  path.file_stem()?.to_str()?.strip_prefix("auv-").map(OsString::from)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt;

  fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
  path.is_file()
    && path.extension().and_then(OsStr::to_str).is_some_and(|extension| {
      windows_executable_extensions().iter().any(|candidate| candidate.trim_start_matches('.').eq_ignore_ascii_case(extension))
    })
}

#[cfg(windows)]
fn windows_executable_extensions() -> Vec<String> {
  const SUPPORTED: &[&str] = &[".COM", ".EXE", ".BAT", ".CMD"];
  env::var_os("PATHEXT")
    .map(|value| {
      value
        .to_string_lossy()
        .split(';')
        .filter(|extension| SUPPORTED.iter().any(|supported| supported.eq_ignore_ascii_case(extension)))
        .map(str::to_owned)
        .collect()
    })
    .unwrap_or_else(|| SUPPORTED.iter().map(|extension| (*extension).to_string()).collect())
}
