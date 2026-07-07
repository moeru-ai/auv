//! Inspect server session descriptor (cross-process discovery).
//!
//! When `inspect serve --enable-write` runs, it writes a session file containing
//! the local server URL, store root, write-enabled state, optional write token,
//! process id, and start time. Other CLI runs read this descriptor to discover
//! a running inspect server they should report to.
//!
//! The descriptor lives in a user-private location (`XDG_RUNTIME_DIR` /
//! `~/Library/Caches/AUV/` / `XDG_CACHE_HOME` / `~/.cache/auv/`) and is written
//! with owner-only permissions. Readers reject descriptors that are not regular
//! files, are not owned by the current user, or grant group/other access.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::model::AuvResult;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectServerSession {
  pub url: String,
  pub store_root: String,
  pub write_enabled: bool,
  pub write_token: Option<String>,
  pub pid: u32,
  pub started_at_millis: u64,
}

pub fn default_session_path() -> PathBuf {
  if let Some(path) = std::env::var_os("AUV_INSPECT_SESSION") {
    return PathBuf::from(path);
  }
  if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
    return PathBuf::from(path).join("auv").join("inspect-session.json");
  }
  #[cfg(target_os = "macos")]
  if let Some(home) = std::env::var_os("HOME") {
    return PathBuf::from(home).join("Library").join("Caches").join("AUV").join("inspect-session.json");
  }
  if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
    return PathBuf::from(path).join("auv").join("inspect-session.json");
  }
  if let Some(home) = std::env::var_os("HOME") {
    return PathBuf::from(home).join(".cache").join("auv").join("inspect-session.json");
  }
  std::env::temp_dir().join(format!("auv-{}", current_user_id_for_path())).join("inspect-session.json")
}

pub fn write_inspect_session(session: &InspectServerSession) -> AuvResult<()> {
  let path = default_session_path();
  if let Some(parent) = path.parent()
    && !parent.as_os_str().is_empty()
  {
    create_private_session_directory(parent)?;
  }
  let bytes = serde_json::to_vec_pretty(session).map_err(|error| format!("failed to encode inspect session: {error}"))?;
  write_inspect_session_bytes(&path, &bytes)
}

fn write_inspect_session_bytes(path: &Path, bytes: &[u8]) -> AuvResult<()> {
  let temp_path = inspect_session_temp_path(path)?;
  let write_result = (|| {
    let mut file = create_inspect_session_temp_file(&temp_path)?;
    file.write_all(bytes).map_err(|error| format!("failed to write inspect session {}: {error}", temp_path.display()))?;
    file.sync_all().map_err(|error| format!("failed to sync inspect session {}: {error}", temp_path.display()))?;
    drop(file);
    replace_inspect_session_file(&temp_path, path)
  })();

  if let Err(error) = write_result {
    let _ = std::fs::remove_file(&temp_path);
    return Err(error);
  }

  Ok(())
}

fn inspect_session_temp_path(path: &Path) -> AuvResult<PathBuf> {
  let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
  let file_name = path.file_name().ok_or_else(|| format!("inspect session path {} has no file name", path.display()))?;
  Ok(parent.join(format!(".{}.{}.{}.tmp", file_name.to_string_lossy(), std::process::id(), crate::model::now_millis())))
}

#[cfg(unix)]
fn create_inspect_session_temp_file(path: &Path) -> AuvResult<std::fs::File> {
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

  let file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(path)
    .map_err(|error| format!("failed to create inspect session temp file {}: {error}", path.display()))?;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    .map_err(|error| format!("failed to restrict inspect session temp file {}: {error}", path.display()))?;
  Ok(file)
}

#[cfg(not(unix))]
fn create_inspect_session_temp_file(path: &Path) -> AuvResult<std::fs::File> {
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)
    .map_err(|error| format!("failed to create inspect session temp file {}: {error}", path.display()))
}

#[cfg(unix)]
fn replace_inspect_session_file(temp_path: &Path, path: &Path) -> AuvResult<()> {
  std::fs::rename(temp_path, path)
    .map_err(|error| format!("failed to replace inspect session {} from {}: {error}", path.display(), temp_path.display()))
}

#[cfg(not(unix))]
fn replace_inspect_session_file(temp_path: &Path, path: &Path) -> AuvResult<()> {
  let _ = std::fs::remove_file(path);
  std::fs::rename(temp_path, path)
    .map_err(|error| format!("failed to replace inspect session {} from {}: {error}", path.display(), temp_path.display()))
}

pub fn read_inspect_session() -> AuvResult<Option<InspectServerSession>> {
  let path = default_session_path();
  if !path.exists() {
    return Ok(None);
  }
  validate_inspect_session_file(&path)?;
  let raw = std::fs::read_to_string(&path).map_err(|error| format!("failed to read inspect session {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map(Some).map_err(|error| format!("failed to parse inspect session {}: {error}", path.display()))
}

#[cfg(unix)]
fn current_user_id_for_path() -> u32 {
  unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn current_user_id_for_path() -> u32 {
  0
}

#[cfg(unix)]
fn create_private_session_directory(path: &Path) -> AuvResult<()> {
  use std::os::unix::fs::PermissionsExt;

  std::fs::create_dir_all(path).map_err(|error| format!("failed to create inspect session directory: {error}"))?;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    .map_err(|error| format!("failed to restrict inspect session directory {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn create_private_session_directory(path: &Path) -> AuvResult<()> {
  std::fs::create_dir_all(path).map_err(|error| format!("failed to create inspect session directory: {error}"))
}

#[cfg(unix)]
fn validate_inspect_session_file(path: &Path) -> AuvResult<()> {
  use std::os::unix::fs::MetadataExt;

  let metadata = std::fs::symlink_metadata(path).map_err(|error| format!("failed to stat inspect session {}: {error}", path.display()))?;
  if !metadata.file_type().is_file() {
    return Err(format!("unsafe inspect session {}: descriptor is not a regular file", path.display()));
  }
  if metadata.uid() != current_user_id_for_path() {
    return Err(format!("unsafe inspect session {}: descriptor is not owned by the current user", path.display()));
  }
  if metadata.mode() & 0o077 != 0 {
    return Err(format!("unsafe inspect session {}: descriptor permissions must not grant group/other access", path.display()));
  }
  Ok(())
}

#[cfg(not(unix))]
fn validate_inspect_session_file(_path: &Path) -> AuvResult<()> {
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::sync::Mutex;

  use super::{InspectServerSession, read_inspect_session, write_inspect_session};

  static ENV_LOCK: Mutex<()> = Mutex::new(());

  #[cfg(unix)]
  #[test]
  fn read_inspect_session_rejects_world_readable_env_override() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!("auv-inspect-session-unsafe-mode-{}", crate::model::now_millis()));
    std::fs::create_dir_all(&root).expect("session test directory should write");
    let path = root.join("session.json");
    std::fs::write(
      &path,
      serde_json::to_string(&InspectServerSession {
        url: "http://127.0.0.1:8765".to_string(),
        store_root: root.display().to_string(),
        write_enabled: true,
        write_token: Some("secret".to_string()),
        pid: 123,
        started_at_millis: 456,
      })
      .expect("session should encode"),
    )
    .expect("session should write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("session file permissions should change");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &path);
    }

    let error = read_inspect_session().expect_err("unsafe session file should reject");

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = std::fs::remove_dir_all(root);
    assert!(error.contains("unsafe inspect session"));
  }

  #[cfg(unix)]
  #[test]
  fn write_inspect_session_replaces_file_with_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!("auv-inspect-session-permissions-{}", crate::model::now_millis()));
    let path = root.join("session.json");
    std::fs::create_dir_all(&root).expect("session test directory should write");
    std::fs::write(&path, "{}").expect("existing session file should write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("existing session file permissions should change");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &path);
    }

    write_inspect_session(&InspectServerSession {
      url: "http://127.0.0.1:8765".to_string(),
      store_root: root.display().to_string(),
      write_enabled: true,
      write_token: Some("secret".to_string()),
      pid: 123,
      started_at_millis: 456,
    })
    .expect("session should write");

    let mode = std::fs::metadata(&path).expect("session file should exist").permissions().mode() & 0o777;
    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = std::fs::remove_dir_all(root);

    assert_eq!(mode, 0o600);
  }
}
