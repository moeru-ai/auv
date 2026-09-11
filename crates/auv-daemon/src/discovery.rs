use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs2::FileExt;

pub use auv::discovery::default_path;

pub struct PublishedDescriptor {
  path: PathBuf,
  instance_id: String,
  _lock: File,
}

impl PublishedDescriptor {
  pub fn publish(path: PathBuf, endpoint: String) -> Result<Self, String> {
    let parent = path.parent().ok_or_else(|| format!("daemon descriptor path has no parent: {}", path.display()))?;
    let parent_existed = parent.exists();
    fs::create_dir_all(parent).map_err(|error| format!("failed to create daemon state directory {}: {error}", parent.display()))?;
    if !parent_existed {
      set_private_directory(parent)?;
    }

    let lock_path = path.with_extension("lock");
    let lock = OpenOptions::new()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(&lock_path)
      .map_err(|error| format!("failed to open daemon lock {}: {error}", lock_path.display()))?;
    set_private_file(&lock_path)?;
    lock.try_lock_exclusive().map_err(|error| format!("another AUV API server owns {}: {error}", lock_path.display()))?;

    let instance_id = uuid::Uuid::now_v7().to_string();
    let descriptor = auv::discovery::Descriptor::for_current_process(endpoint, instance_id.clone());
    let temporary = path.with_extension(format!("tmp-{instance_id}"));
    let mut file = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&temporary)
      .map_err(|error| format!("failed to create daemon descriptor {}: {error}", temporary.display()))?;
    set_private_file(&temporary)?;
    serde_json::to_writer(&mut file, &descriptor).map_err(|error| format!("failed to encode daemon descriptor: {error}"))?;
    file.write_all(b"\n").map_err(|error| format!("failed to finish daemon descriptor: {error}"))?;
    file.sync_all().map_err(|error| format!("failed to sync daemon descriptor: {error}"))?;
    // TODO(windows-discovery-replace): use a platform replace primitive when
    // Windows daemon hosting is enabled; Unix rename atomically replaces stale
    // descriptors, while std::fs::rename does not replace on Windows.
    fs::rename(&temporary, &path).map_err(|error| format!("failed to publish daemon descriptor {}: {error}", path.display()))?;

    Ok(Self {
      path,
      instance_id,
      _lock: lock,
    })
  }
}

impl Drop for PublishedDescriptor {
  fn drop(&mut self) {
    let owned =
      auv::discovery::read_descriptor(&self.path).ok().flatten().is_some_and(|descriptor| descriptor.instance_id() == self.instance_id);
    if owned {
      let _ = fs::remove_file(&self.path);
    }
  }
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    .map_err(|error| format!("failed to protect daemon state directory {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), String> {
  Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), String> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|error| format!("failed to protect daemon state file {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), String> {
  Ok(())
}

#[cfg(test)]
#[path = "discovery_test.rs"]
mod tests;
