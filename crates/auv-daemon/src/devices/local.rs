//! Composition of the target-local Device service and its private state.
//!
//! This OS-local transport serves only `DeviceLocalService`. The paired Device
//! router and HTTP gateway cannot reach it. Policy state is shared with the
//! Daemon so the installed host binds remote lock and unlock to these instances.
//!
//! NOTICE(device-local-uid): The 0700 control directory admits the daemon
//! owner UID and root. A root-owned multi-account daemon cannot yet accept
//! another user's self-enrollment over this path. A separate searchable IPC
//! parent and per-user host access need their own authorization gate.

#[cfg(unix)]
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::Arc;

use auv::devices::{DeviceEntryErrorReason, EnsureUserSessionUnlockedEffect, UserSession, UserSessionTarget};
#[cfg(windows)]
use auv_api_client::device_local::named_pipe_name;
#[cfg(unix)]
use auv_api_client::device_local::unix_socket_path;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use auv_api_server::control::CallerId;
use auv_api_server::control::Pairing;
use auv_api_server::device_local::{
  self, AuditEntry as LocalAuditEntry, AuditPage as LocalAuditPage, DeviceLocalControl, Enrollment as LocalEnrollment, LocalControlError,
  LocalOsPrincipal,
};
use tokio_util::sync::CancellationToken;

use super::audit::Audit;
#[cfg(target_os = "linux")]
use super::host_linux::LinuxSessionHost;
#[cfg(target_os = "macos")]
use super::host_macos::MacosSessionHost;
#[cfg(target_os = "windows")]
use super::host_windows::WindowsSessionHost;
use super::metadata::MetadataStore;
use super::policy::AccountLocks;
use super::policy::Enrollment as StoredEnrollment;
use super::policy::Policy;

// Keep local wire mapping and authorization in one place; the platform hosts
// still own account lookup, vault access, and credential verification.
pub(super) fn local_enrollment(value: StoredEnrollment) -> LocalEnrollment {
  LocalEnrollment {
    user: value.user,
    os_account_id: value.os_account_id,
    state: value.state,
  }
}

/// Keep the account lock with a blocking vault mutation after its RPC future
/// is canceled. Metadata changes belong in `change` too, so a later request
/// cannot interleave before the native operation and publication finish.
pub(super) async fn complete_account_mutation<T: Send + 'static>(
  guard: tokio::sync::OwnedMutexGuard<()>,
  change: impl FnOnce() -> Result<T, LocalControlError> + Send + 'static,
) -> Result<T, LocalControlError> {
  tokio::task::spawn_blocking(move || {
    let _guard = guard;
    change()
  })
  .await
  .map_err(|_| LocalControlError::HostUnavailable)?
}

pub(super) fn audit_page(
  audit: &Audit,
  principal: &LocalOsPrincipal,
  cursor: u64,
  limit: usize,
) -> Result<LocalAuditPage, LocalControlError> {
  if !(1..=100).contains(&limit) {
    return Err(LocalControlError::InvalidAccount);
  }

  let page = audit.read_for_principal(principal, cursor, limit).map_err(|_| LocalControlError::Persistence)?;
  Ok(LocalAuditPage {
    entries: page
      .entries
      .into_iter()
      .map(|entry| LocalAuditEntry {
        event: entry.event,
        attempt_id: entry.attempt_id,
        caller: entry.caller,
        os_account_id: entry.os_account_id,
        user: entry.user,
        session_selector: entry.session_selector,
        result: entry.result,
        at_unix_millis: entry.at_unix_millis,
      })
      .collect(),
    next_cursor: page.next_cursor,
  })
}

#[cfg(unix)]
pub(super) fn unix_uid(principal: &LocalOsPrincipal) -> Result<u32, LocalControlError> {
  match principal {
    LocalOsPrincipal::UnixUid(uid) => Ok(*uid),
    _ => Err(LocalControlError::PermissionDenied),
  }
}

#[cfg(unix)]
pub(super) fn authorize_unix(principal: &LocalOsPrincipal, account_uid: u32) -> Result<(), LocalControlError> {
  let caller_uid = unix_uid(principal)?;

  if caller_uid == 0 || caller_uid == account_uid {
    Ok(())
  } else {
    Err(LocalControlError::PermissionDenied)
  }
}

#[cfg(unix)]
pub(super) fn require_unix_admin(principal: &LocalOsPrincipal) -> Result<(), LocalControlError> {
  // A verified root peer is the initial OS-admin subset. Group membership
  // needs a separately approved authorization gate.
  if unix_uid(principal)? == 0 {
    Ok(())
  } else {
    Err(LocalControlError::PermissionDenied)
  }
}

#[cfg(unix)]
pub(super) fn unix_account_id(uid: u32) -> String {
  format!("uid:{uid}")
}

/// One daemon-process owner of enrollment metadata, account locks, and audit.
pub(crate) struct LocalState {
  control: Arc<dyn DeviceLocalControl>,
  #[cfg(target_os = "macos")]
  policy: Policy<MacosSessionHost>,
  #[cfg(target_os = "linux")]
  policy: Policy<LinuxSessionHost>,
  #[cfg(target_os = "windows")]
  policy: Policy<WindowsSessionHost>,
  #[cfg(unix)]
  socket: PathBuf,
  #[cfg(windows)]
  pipe_name: String,
}

impl LocalState {
  pub(crate) fn open(store_root: &Path, pairing: Option<Arc<dyn Pairing>>) -> Result<Self, String> {
    #[cfg(unix)]
    let root = {
      let control_root = store_root.join("control");
      let root = control_root.join("device-entry");
      private_directory(&control_root)?;
      private_directory(&root)?;
      root
    };
    #[cfg(windows)]
    let root = super::storage_windows::root_path().map_err(|_| "failed to locate Windows Device entry storage".to_string())?;
    let metadata = Arc::new(MetadataStore::open(&root).map_err(|error| format!("failed to open Device entry policy: {error}"))?);
    let audit = Arc::new(Audit::open(&root).map_err(|error| format!("failed to open Device entry audit: {error}"))?);
    let account_locks = Arc::new(AccountLocks::new());
    let policy_gate = Arc::new(tokio::sync::RwLock::new(()));

    #[cfg(target_os = "linux")]
    let control: Arc<dyn DeviceLocalControl> = Arc::new(super::enrollment_linux::LinuxLocalEnrollment::new(
      Arc::clone(&metadata),
      Arc::clone(&account_locks),
      Arc::clone(&audit),
      Arc::clone(&policy_gate),
    ));
    #[cfg(target_os = "macos")]
    let control: Arc<dyn DeviceLocalControl> = Arc::new(super::enrollment_macos::MacosLocalEnrollment::new(
      Arc::clone(&metadata),
      Arc::clone(&account_locks),
      Arc::clone(&audit),
      Arc::clone(&policy_gate),
    ));
    #[cfg(target_os = "windows")]
    let control: Arc<dyn DeviceLocalControl> = Arc::new(super::enrollment_windows::WindowsLocalEnrollment::new(
      Arc::clone(&metadata),
      Arc::clone(&account_locks),
      Arc::clone(&audit),
      Arc::clone(&policy_gate),
    ));
    #[cfg(target_os = "macos")]
    let policy = Policy::new(MacosSessionHost::new(), metadata, audit, account_locks, policy_gate, pairing);
    #[cfg(target_os = "linux")]
    let policy = Policy::new(LinuxSessionHost::new(), metadata, audit, account_locks, policy_gate, pairing);
    #[cfg(target_os = "windows")]
    let policy = Policy::new(WindowsSessionHost, metadata, audit, account_locks, policy_gate, pairing);
    Ok(Self {
      control,
      policy,
      #[cfg(unix)]
      socket: unix_socket_path(store_root).map_err(|error| format!("failed to locate Device-local socket: {error}"))?,
      #[cfg(windows)]
      pipe_name: named_pipe_name(store_root),
    })
  }

  pub(crate) fn list_user_sessions(&self) -> Result<Vec<UserSession>, DeviceEntryErrorReason> {
    self.policy.list()
  }

  pub(crate) fn get_user_session(&self, selector: &str) -> Result<UserSession, DeviceEntryErrorReason> {
    self.policy.get(selector)
  }

  pub(crate) async fn ensure_user_session_unlocked(
    &self,
    caller: &CallerId,
    target: UserSessionTarget,
  ) -> Result<EnsureUserSessionUnlockedEffect, DeviceEntryErrorReason> {
    self.policy.ensure(caller, target).await
  }

  #[cfg(all(test, unix))]
  pub(crate) fn socket_path(&self) -> &Path {
    &self.socket
  }

  pub(crate) async fn serve(&self, shutdown: CancellationToken) -> Result<(), String> {
    #[cfg(unix)]
    {
      let _directory = SocketDirectory::create(&self.socket)?;
      device_local::serve_unix(&self.socket, Arc::clone(&self.control), shutdown).await
    }

    #[cfg(windows)]
    {
      device_local::serve_named_pipe(&self.pipe_name, Arc::clone(&self.control), shutdown).await
    }
  }
}

/// Owns only the short socket directory created or verified for this daemon.
/// A killed daemon may leave it behind; the next start rechecks it before use.
#[cfg(unix)]
struct SocketDirectory {
  path: PathBuf,
  device: u64,
  inode: u64,
}

#[cfg(unix)]
impl SocketDirectory {
  fn create(socket: &Path) -> Result<Self, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let path = socket.parent().ok_or_else(|| "Device-local socket requires a parent directory".to_string())?;
    let mut builder = fs::DirBuilder::new();

    match builder.mode(0o700).create(path) {
      Ok(()) => {}
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
      Err(error) => return Err(format!("failed to create Device-local socket directory: {error}")),
    }

    let metadata = fs::symlink_metadata(path).map_err(|error| format!("failed to inspect Device-local socket directory: {error}"))?;
    // SAFETY: geteuid has no arguments or pointers.
    if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o777 != 0o700 {
      return Err("Device-local socket directory must be owned by this daemon with mode 0700".into());
    }

    Ok(Self {
      path: path.to_owned(),
      device: metadata.dev(),
      inode: metadata.ino(),
    })
  }
}

#[cfg(unix)]
impl Drop for SocketDirectory {
  fn drop(&mut self) {
    use std::os::unix::fs::MetadataExt;

    if let Ok(metadata) = fs::symlink_metadata(&self.path)
      && metadata.file_type().is_dir()
      && metadata.dev() == self.device
      && metadata.ino() == self.inode
    {
      let _ = fs::remove_dir(&self.path);
    }
  }
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<(), String> {
  fs::create_dir_all(path).map_err(|error| format!("failed to create Device local directory: {error}"))?;
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let metadata = fs::symlink_metadata(path).map_err(|error| format!("failed to inspect Device local directory: {error}"))?;
  // SAFETY: geteuid has no arguments or pointers.
  if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
    return Err("Device local directory must be owned by this daemon".into());
  }

  fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| format!("failed to restrict Device local directory: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
  use super::*;

  // ROOT CAUSE:
  //
  // Canceling the RPC released its account guard while spawn_blocking still
  // wrote the vault, allowing a later mutation to overtake it. The worker
  // now owns the guard until its metadata and vault change has completed.
  #[tokio::test]
  async fn canceled_blocking_enrollment_keeps_account_serialized() {
    use std::sync::mpsc;
    use std::time::Duration;

    let account_locks = Arc::new(AccountLocks::new());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let request = tokio::spawn({
      let account_locks = Arc::clone(&account_locks);
      async move {
        let guard = account_locks.lock("uid:501").await.unwrap();
        complete_account_mutation(guard, move || {
          entered_tx.send(()).unwrap();
          release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
          Ok(())
        })
        .await
        .unwrap();
      }
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(2)).unwrap()).await.unwrap();
    request.abort();
    let _ = request.await;
    let concurrent = tokio::time::timeout(Duration::from_millis(100), account_locks.lock("uid:501")).await;
    release_tx.send(()).unwrap();

    assert!(concurrent.is_err(), "the blocking vault mutation must retain the account lock after request cancellation");

    tokio::time::timeout(Duration::from_secs(2), account_locks.lock("uid:501")).await.unwrap().unwrap();
  }

  #[test]
  fn short_socket_directory_rejects_substitution_and_permissive_mode() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("socket-parent/socket");
    let parent = socket.parent().unwrap();
    symlink(root.path(), parent).unwrap();

    assert!(SocketDirectory::create(&socket).is_err());

    fs::remove_file(parent).unwrap();
    fs::create_dir(parent).unwrap();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o777)).unwrap();

    assert!(SocketDirectory::create(&socket).is_err());
  }
}
