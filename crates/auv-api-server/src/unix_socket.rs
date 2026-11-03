//! Claiming a Unix socket pathname for a local listener: stale-socket
//! recovery and binding without a window where other users can connect.

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::UnixListener;

pub(crate) fn current_euid() -> u32 {
  // SAFETY: geteuid has no pointers, allocation, or preconditions.
  unsafe { libc::geteuid() }
}

/// Removes `path` only when it is a socket owned by this user whose listener
/// is gone (connect is refused). A missing path is fine. A live listener, a
/// non-socket, another user's socket, or an unknown liveness result is an
/// error and the path is left in place. `label` names the socket in errors.
pub(crate) async fn clear_stale(path: &Path, label: &str) -> Result<(), String> {
  let existing = match std::fs::symlink_metadata(path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(error) => return Err(format!("cannot inspect {label} path: {error}")),
  };

  if !existing.file_type().is_socket() || existing.uid() != current_euid() {
    return Err(format!("{label} path is not an owned socket"));
  }

  // NOTICE(stale-socket-liveness): ECONNREFUSED means no process listens on
  // the inode. macOS also returns it when a live listener's accept backlog is
  // full; tokio's backlog of 1024 makes that unlikely for an idle restart, and
  // the inode recheck below still refuses a socket that was replaced.
  match tokio::time::timeout(std::time::Duration::from_millis(250), tokio::net::UnixStream::connect(path)).await {
    Ok(Ok(_)) => return Err(format!("{label} already has a live listener")),
    Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
    Ok(Err(_)) | Err(_) => return Err(format!("{label} liveness could not be established")),
  }

  // NOTICE(stale-socket-recovery): A process killed without Drop (SIGKILL, a
  // crash) leaves a socket inode. Recheck the inode before removing the
  // refused listener's pathname so a socket another process bound meanwhile
  // is never unlinked.
  let current = std::fs::symlink_metadata(path).map_err(|_| format!("{label} changed during stale recovery"))?;

  if !current.file_type().is_socket()
    || current.uid() != existing.uid()
    || current.dev() != existing.dev()
    || current.ino() != existing.ino()
  {
    return Err(format!("{label} changed during stale recovery"));
  }

  std::fs::remove_file(path).map_err(|error| format!("cannot remove stale {label}: {error}"))
}

/// Binds a listener at `path` with mode 0600 such that no other user can
/// connect at any moment, including between `bind` and `chmod`. Missing
/// parents are created with mode 0700. The caller clears stale sockets first;
/// an existing path is an error.
pub(crate) fn bind_private(path: &Path) -> Result<UnixListener, String> {
  let parent = match path.parent() {
    Some(parent) if !parent.as_os_str().is_empty() => parent,
    _ => Path::new("."),
  };
  std::fs::DirBuilder::new()
    .recursive(true)
    .mode(0o700)
    .create(parent)
    .map_err(|error| format!("failed to create API socket directory {}: {error}", parent.display()))?;

  // `bind` creates the socket with the process umask, and umask is
  // process-wide, so it cannot be tightened for one thread. Instead the socket
  // is first bound where only this user can reach it, chmod-ed, and only then
  // given its public name.
  let parent_metadata =
    std::fs::metadata(parent).map_err(|error| format!("failed to inspect API socket directory {}: {error}", parent.display()))?;
  if parent_metadata.uid() == current_euid() && parent_metadata.permissions().mode() & 0o077 == 0 {
    // Other users cannot traverse the parent, so the socket is unreachable
    // for them while its own mode is still the umask default.
    let listener = UnixListener::bind(path).map_err(|error| format!("failed to bind API Unix socket {}: {error}", path.display()))?;
    protect(path)?;
    return Ok(listener);
  }

  let staging = Staging::create(parent)?;
  let listener =
    UnixListener::bind(&staging.socket).map_err(|error| format!("failed to bind API Unix socket {}: {error}", path.display()))?;
  protect(&staging.socket)?;
  // `link` fails with EEXIST instead of replacing, so a pathname another
  // process created meanwhile is never clobbered. The listener stays bound to
  // the same inode under its new name.
  std::fs::hard_link(&staging.socket, path).map_err(|error| format!("failed to publish API Unix socket {}: {error}", path.display()))?;
  Ok(listener)
}

fn protect(path: &Path) -> Result<(), String> {
  // Local transport skips pairing, so the socket itself must not grant access
  // to group/other users while peer-caller projection remains deferred.
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    .map_err(|error| format!("failed to protect API Unix socket {}: {error}", path.display()))
}

/// A 0700 directory beside the final socket, removed with its socket on drop.
struct Staging {
  directory: PathBuf,
  socket: PathBuf,
}

impl Staging {
  fn create(parent: &Path) -> Result<Self, String> {
    let mut suffix = [0_u8; 4];
    getrandom::fill(&mut suffix).map_err(|error| format!("failed to name API socket staging directory: {error}"))?;
    // NOTICE(unix-socket-path-length): keep the staging name short; socket
    // paths are limited to about 104 bytes and this adds `.auv-XXXXXXXX/s`.
    let directory = parent.join(format!(".auv-{:08x}", u32::from_ne_bytes(suffix)));
    // mkdir is exclusive; the umask can only clear bits from 0700.
    std::fs::DirBuilder::new()
      .mode(0o700)
      .create(&directory)
      .map_err(|error| format!("failed to create API socket staging directory {}: {error}", directory.display()))?;
    let staging = Self {
      socket: directory.join("s"),
      directory,
    };
    std::fs::set_permissions(&staging.directory, std::fs::Permissions::from_mode(0o700))
      .map_err(|error| format!("failed to protect API socket staging directory {}: {error}", staging.directory.display()))?;
    Ok(staging)
  }
}

impl Drop for Staging {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.socket);
    let _ = std::fs::remove_dir(&self.directory);
  }
}

#[cfg(test)]
#[path = "unix_socket_test.rs"]
mod tests;
