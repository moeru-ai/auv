//! Subprocesses and file installation shared by the release commands.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// Fails unless every command is on `PATH`.
pub fn require_commands(commands: &[&str]) -> Result<(), String> {
  let path = std::env::var_os("PATH").unwrap_or_default();
  for command in commands {
    if !std::env::split_paths(&path).any(|directory| directory.join(command).is_file()) {
      return Err(format!("required command not found: {command}"));
    }
  }
  Ok(())
}

/// Runs a command to completion and returns its standard output.
pub fn output<I, S>(program: &str, arguments: I) -> Result<String, String>
where
  I: IntoIterator<Item = S>,
  S: AsRef<OsStr>,
{
  let mut command = Command::new(program);
  command.args(arguments);
  checked(command)
}

/// Runs a prepared command; a non-zero exit is an error carrying its stderr.
pub fn checked(mut command: Command) -> Result<String, String> {
  let shown = format!("{command:?}");
  let output = command.output().map_err(|error| format!("failed to run {shown}: {error}"))?;
  if !output.status.success() {
    return Err(format!("{shown} failed ({}): {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
  }
  String::from_utf8(output.stdout).map_err(|error| format!("{shown} printed invalid UTF-8: {error}"))
}

/// Copies `source` to `destination`, creating parent directories, and sets
/// the Unix mode as `install -D -m` does.
pub fn install(source: &Path, destination: &Path, mode: u32) -> Result<(), String> {
  if let Some(parent) = destination.parent() {
    create_dir(parent)?;
  }
  std::fs::copy(source, destination)
    .map_err(|error| format!("failed to copy {} to {}: {error}", source.display(), destination.display()))?;
  set_mode(destination, mode)
}

pub fn create_dir(path: &Path) -> Result<(), String> {
  std::fs::create_dir_all(path).map_err(|error| format!("failed to create {}: {error}", path.display()))
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt as _;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    .map_err(|error| format!("failed to set mode on {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<(), String> {
  Ok(())
}

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
  use std::os::unix::fs::PermissionsExt as _;
  std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool {
  path.is_file()
}

/// Whether `path` is a non-empty regular file, as `test -s` checks.
pub fn is_non_empty_file(path: &Path) -> bool {
  std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}
