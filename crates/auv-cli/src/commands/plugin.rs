use clap::{Args, Subcommand};

/// Inspect external auv-* command plugins visible on PATH.
#[derive(Clone, Debug, Args)]
pub struct PluginArgs {
  #[command(subcommand)]
  pub command: PluginCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
  /// List external auv-* executables visible on PATH.
  List,
}

pub async fn run(args: PluginArgs) -> Result<i32, String> {
  match args.command {
    PluginCommand::List => list(),
  }
}

use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use auv::AuvContext;
use auv::selection::RootSelection;

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

pub async fn execute(
  command_name: &OsStr,
  arguments: &[OsString],
  parent_context: &RootSelection,
  project_root: &Path,
) -> Result<i32, String> {
  let mut executable_name = OsString::from("auv-");
  executable_name.push(command_name);
  let executable = resolve(&executable_name)
    .ok_or_else(|| format!("unknown command {:?}; no {:?} executable was found on PATH", command_name, executable_name))?;
  let auv_path = env::current_exe().map_err(|error| format!("failed to resolve the auv executable path: {error}"))?;
  let resolved = resolve_context(parent_context).await?;
  let context = serde_json::to_string(&resolved.context).map_err(|error| format!("failed to encode AUV_CONTEXT: {error}"))?;

  let store_root = project_root.join(".auv").join("store");
  let exit = execute_resolved(&executable, arguments, &auv_path, &context, &store_root, resolved.implicit_run_id.is_none())?;
  resolved.finish(exit == 0).await?;
  Ok(exit)
}

#[cfg(unix)]
fn execute_resolved(
  executable: &Path,
  arguments: &[OsString],
  auv_path: &Path,
  context: &str,
  store_root: &Path,
  replace_process: bool,
) -> Result<i32, String> {
  use std::os::unix::process::{CommandExt, ExitStatusExt};

  let mut command = Command::new(executable);
  command.args(arguments).env("AUV_CONTEXT", context).env("AUV_PATH", auv_path).env(auv_tracing::STORE_ROOT_ENV, store_root);
  if replace_process {
    let error = command.exec();
    return Err(format!("failed to execute plugin {}: {error}", executable.display()));
  }
  // TODO(plugin-signal-forwarding): an implicitly owned Run requires the root
  // process to wait and close it after ordinary child exit. Add explicit
  // signal forwarding plus a bounded cleanup path before recording terminal Run
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
  store_root: &Path,
  _replace_process: bool,
) -> Result<i32, String> {
  let status = Command::new(executable)
    .args(arguments)
    .env("AUV_CONTEXT", context)
    .env("AUV_PATH", auv_path)
    .env(auv_tracing::STORE_ROOT_ENV, store_root)
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
      auv::runs::RunOutcome::Succeeded
    } else {
      auv::runs::RunOutcome::Failed
    };
    let client = auv::Client::from_context(self.context).await.map_err(|error| error.to_string())?;
    let selector = auv::resource::RunSelector::parse(&run_id).map_err(|error| error.to_string())?;
    client.runs().stop(&selector, outcome).await.map_err(|error| format!("failed to stop the implicit Run: {error}"))?;
    Ok(())
  }
}

async fn resolve_context(parent: &RootSelection) -> Result<ResolvedExecutionContext, String> {
  let context = AuvContext {
    invocation_id: Some(format!("invocation_{}", uuid::Uuid::now_v7())),
    ..AuvContext::default()
  };
  if parent.is_empty() {
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

pub(crate) async fn resolve_invoke_context(parent: &RootSelection) -> Result<ResolvedExecutionContext, String> {
  resolve_selected_context(parent, None, true, AuvContext::default()).await
}

async fn resolve_selected_context(
  parent: &RootSelection,
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
  let client = auv::Client::from_context(context).await.map_err(|error| error.to_string())?;
  let mut context = client.context().cloned().unwrap_or_default();
  if create_implicit_run {
    let selection = parent
      .run_id
      .as_deref()
      .map(auv::resource::RunSelector::parse)
      .transpose()
      .map_err(|error| error.to_string())?
      .map(auv::client::RunSelection::Existing)
      .unwrap_or_default();
    let device = match (&context.device_id, &context.device_name) {
      (Some(id), Some(name)) => auv::resource::DeviceSelector::by_id_and_name(id, name.clone()),
      (Some(id), None) => auv::resource::DeviceSelector::by_id(id),
      (None, Some(name)) => auv::resource::DeviceSelector::by_name(name.clone()),
      (None, None) => auv::resource::DeviceSelector::default(),
    };
    let run = client
      .run(auv::client::RunOptions {
        selection,
        device,
        labels: Default::default(),
      })
      .await
      .map_err(|error| error.to_string())?;
    context.device_id = run.device().map(|device| device.id.to_string());
    context.device_name = run.device().and_then(|device| (!device.name.is_empty()).then(|| device.name.clone()));
    context.run_id = Some(run.resource().id.to_string());
    let implicit_run_id = run.is_owned().then(|| context.run_id.clone()).flatten();
    return Ok(ResolvedExecutionContext {
      context,
      implicit_run_id,
    });
  }
  let resolved = client.resolve(parent).await.map_err(|error| error.to_string())?;
  context.device_id = resolved.device.as_ref().map(|device| device.id.to_string());
  context.device_name = resolved.device.and_then(|device| (!device.name.is_empty()).then_some(device.name));
  context.run_id = resolved.run.map(|run| run.id.to_string());
  Ok(ResolvedExecutionContext {
    context,
    implicit_run_id: None,
  })
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
