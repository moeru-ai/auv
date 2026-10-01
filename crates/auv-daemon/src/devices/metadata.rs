//! Durable non-secret Device entry policy and enrollment metadata.
//!
//! The local enrollment backend must invalidate eligibility durably before
//! overwriting its native vault item, then publish Pending after vault store.
//! It holds the shared account lock across that transaction. No secret or
//! vault error text is serialized here.

use std::collections::HashMap;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use auv::devices::DeviceEntryErrorReason;
use auv::devices::EnrollmentState;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::policy::Enrollment;
#[cfg(windows)]
use super::storage_windows::{self, Creation};

pub(super) struct MetadataStore {
  path: PathBuf,
  state: Mutex<State>,
  poisoned: AtomicBool,
  _process_lock: File,
  #[cfg(windows)]
  _root_guard: File,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
  // Missing policy files and old states start enabled by owner decision.
  #[serde(default = "default_enabled")]
  enabled: bool,
  #[serde(default)]
  enrollments: HashMap<String, StoredEnrollment>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollment {
  user: String,
  generation: u64,
  state: StoredState,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StoredState {
  Pending,
  Ready,
  Suspended,
  Removed,
}

fn default_enabled() -> bool {
  true
}

impl MetadataStore {
  pub(super) fn open(root: &Path) -> Result<Self, DeviceEntryErrorReason> {
    #[cfg(windows)]
    let root_guard = storage_windows::directory(root).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    #[cfg(unix)]
    private_directory(root)?;
    #[cfg(unix)]
    let process_lock = {
      let lock_path = root.join("device-entry-policy.lock");
      let mut lock_options = OpenOptions::new();
      lock_options.create(true).read(true).write(true);
      use std::os::unix::fs::OpenOptionsExt;
      lock_options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
      lock_options.open(lock_path).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?
    };
    #[cfg(windows)]
    let process_lock = storage_windows::file(root, "device-entry-policy.lock", Creation::OpenOrCreate)
      .map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    #[cfg(unix)]
    private_file(&process_lock)?;
    process_lock.try_lock_exclusive().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    let path = root.join("device-entry-policy.json");
    let state = match read_private_file(&path)? {
      Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?,
      None => State {
        enabled: true,
        enrollments: HashMap::new(),
      },
    };

    Ok(Self {
      path,
      state: Mutex::new(state),
      poisoned: AtomicBool::new(false),
      _process_lock: process_lock,
      #[cfg(windows)]
      _root_guard: root_guard,
    })
  }

  /// Only a DeviceLocalService call with a verified OS administrator may call
  /// this. Authorization is deliberately outside this storage primitive.
  pub(super) fn set_enabled(&self, enabled: bool) -> Result<(), DeviceEntryErrorReason> {
    self.update(|state| {
      state.enabled = enabled;
      Ok(())
    })
  }

  /// Invalidate the old enrollment before any vault overwrite. If vault store
  /// fails, the account stays suspended and cannot reuse the old READY state.
  pub(super) fn invalidate_for_enroll(&self, user: &str, os_account_id: &str) -> Result<(), DeviceEntryErrorReason> {
    if user.is_empty() || os_account_id.is_empty() {
      return Err(DeviceEntryErrorReason::ServiceUnavailable);
    }

    self.update(|state| {
      let next = next_generation(state.enrollments.get(os_account_id))?;
      state.enrollments.insert(
        os_account_id.to_owned(),
        StoredEnrollment {
          user: user.to_owned(),
          generation: next,
          state: StoredState::Suspended,
        },
      );
      Ok(())
    })
  }

  /// Publish Pending after the target-local vault write. The immediately
  /// preceding durable state must be Suspended for this same OS account.
  pub(super) fn publish_pending(&self, user: &str, os_account_id: &str) -> Result<Enrollment, DeviceEntryErrorReason> {
    if user.is_empty() || os_account_id.is_empty() {
      return Err(DeviceEntryErrorReason::ServiceUnavailable);
    }

    self.update(|state| {
      let current = state.enrollments.get(os_account_id).ok_or(DeviceEntryErrorReason::ServiceUnavailable)?;

      if current.user != user || !matches!(current.state, StoredState::Suspended) {
        return Err(DeviceEntryErrorReason::ServiceUnavailable);
      }

      let next = next_generation(Some(current))?;
      state.enrollments.insert(
        os_account_id.to_owned(),
        StoredEnrollment {
          user: user.to_owned(),
          generation: next,
          state: StoredState::Pending,
        },
      );
      Ok(Enrollment {
        user: user.to_owned(),
        os_account_id: os_account_id.to_owned(),
        generation: next,
        state: EnrollmentState::Pending,
      })
    })
  }

  /// Revoke eligibility before the native vault item is deleted. The account
  /// lock must cover both operations and caller authorization.
  pub(super) fn remove(&self, os_account_id: &str) -> Result<(), DeviceEntryErrorReason> {
    self.update(|state| {
      if let Some(record) = state.enrollments.get_mut(os_account_id) {
        record.state = StoredState::Removed;
      }

      Ok(())
    })
  }

  /// Local-only metadata inventory. The caller must filter this by the
  /// authenticated OS principal before any record is returned.
  pub(super) fn list_enrollments(&self) -> Result<Vec<Enrollment>, DeviceEntryErrorReason> {
    self.check_health()?;
    let state = self.state.lock().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    let mut records = state.enrollments.iter().filter_map(|(id, record)| enrollment_from_record(id, record)).collect::<Vec<_>>();
    records.sort_by(|left, right| left.os_account_id.cmp(&right.os_account_id));
    Ok(records)
  }

  fn update<T>(&self, operation: impl FnOnce(&mut State) -> Result<T, DeviceEntryErrorReason>) -> Result<T, DeviceEntryErrorReason> {
    self.check_health()?;
    let mut state = self.state.lock().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    let value = operation(&mut state)?;

    if self.write(&state).is_err() {
      // A rename may have succeeded before directory fsync failed. Freeze
      // this process rather than guessing which generation survived on disk.
      self.poisoned.store(true, Ordering::Release);

      return Err(DeviceEntryErrorReason::ServiceUnavailable);
    }

    Ok(value)
  }

  fn check_health(&self) -> Result<(), DeviceEntryErrorReason> {
    if self.poisoned.load(Ordering::Acquire) {
      Err(DeviceEntryErrorReason::ServiceUnavailable)
    } else {
      Ok(())
    }
  }

  fn write(&self, state: &State) -> std::io::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let root = self.path.parent().expect("policy path has parent");
    #[cfg(unix)]
    private_directory(root).map_err(|_| std::io::Error::from(ErrorKind::PermissionDenied))?;
    #[cfg(windows)]
    let _root_guard = storage_windows::directory(root)?;
    let temp_name = format!(".device-entry-policy-{}.tmp", uuid::Uuid::now_v7());
    let temp = root.join(&temp_name);
    #[cfg(unix)]
    let mut file = {
      let mut options = OpenOptions::new();
      options.create_new(true).write(true);
      options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
      options.open(&temp)?
    };
    #[cfg(windows)]
    let mut file = storage_windows::file(root, &temp_name, Creation::New)?;
    let result = (|| {
      serde_json::to_writer(&mut file, state)?;
      file.write_all(b"\n")?;
      file.sync_all()?;
      #[cfg(windows)]
      drop(file);
      #[cfg(unix)]
      fs::rename(&temp, &self.path)?;
      #[cfg(unix)]
      File::open(root)?.sync_all()?;
      #[cfg(windows)]
      storage_windows::replace(root, &temp_name, "device-entry-policy.json")?;
      Ok(())
    })();

    if result.is_err() {
      let _ = fs::remove_file(&temp);
    }

    result
  }
}

// Policy reads and state transitions. DeviceLocalService mutations must use
// the same account lock map as `Policy` and change the generation on every
// enroll or deletion; a vault write must complete before publishing Pending.
impl MetadataStore {
  pub(super) fn enabled(&self) -> Result<bool, DeviceEntryErrorReason> {
    self.check_health()?;
    Ok(self.state.lock().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?.enabled)
  }

  pub(super) fn enrollment(&self, os_account_id: &str) -> Result<Option<Enrollment>, DeviceEntryErrorReason> {
    self.check_health()?;
    let state = self.state.lock().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
    Ok(state.enrollments.get(os_account_id).and_then(|record| enrollment_from_record(os_account_id, record)))
  }

  pub(super) fn promote_ready(&self, os_account_id: &str, generation: u64) -> Result<(), DeviceEntryErrorReason> {
    self.update(|state| {
      let record = state.enrollments.get_mut(os_account_id).ok_or(DeviceEntryErrorReason::Unenrolled)?;

      if record.generation != generation || !matches!(record.state, StoredState::Pending) {
        return Err(DeviceEntryErrorReason::Unenrolled);
      }

      record.state = StoredState::Ready;
      Ok(())
    })
  }

  pub(super) fn suspend(&self, os_account_id: &str, generation: u64) -> Result<(), DeviceEntryErrorReason> {
    self.update(|state| {
      let record = state.enrollments.get_mut(os_account_id).ok_or(DeviceEntryErrorReason::Unenrolled)?;

      if record.generation != generation {
        return Err(DeviceEntryErrorReason::Unenrolled);
      }

      record.state = StoredState::Suspended;
      Ok(())
    })
  }
}

fn next_generation(current: Option<&StoredEnrollment>) -> Result<u64, DeviceEntryErrorReason> {
  match current {
    None => Ok(1),
    Some(current) => current.generation.checked_add(1).ok_or(DeviceEntryErrorReason::ServiceUnavailable),
  }
}

fn enrollment_from_record(os_account_id: &str, record: &StoredEnrollment) -> Option<Enrollment> {
  let state = match record.state {
    StoredState::Pending => EnrollmentState::Pending,
    StoredState::Ready => EnrollmentState::Ready,
    StoredState::Suspended => EnrollmentState::Suspended,
    StoredState::Removed => return None,
  };

  Some(Enrollment {
    user: record.user.clone(),
    os_account_id: os_account_id.to_owned(),
    generation: record.generation,
    state,
  })
}

#[cfg(unix)]
fn private_directory(root: &Path) -> Result<(), DeviceEntryErrorReason> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let metadata = fs::symlink_metadata(root).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

  if !metadata.file_type().is_dir() || metadata.uid() != current_euid() || metadata.permissions().mode() & 0o077 != 0 {
    return Err(DeviceEntryErrorReason::ServiceUnavailable);
  }

  Ok(())
}

#[cfg(unix)]
fn private_file(file: &File) -> Result<(), DeviceEntryErrorReason> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let metadata = file.metadata().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

  if !metadata.file_type().is_file() || metadata.uid() != current_euid() || metadata.permissions().mode() & 0o077 != 0 {
    return Err(DeviceEntryErrorReason::ServiceUnavailable);
  }

  Ok(())
}

fn read_private_file(path: &Path) -> Result<Option<Vec<u8>>, DeviceEntryErrorReason> {
  #[cfg(unix)]
  use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

  #[cfg(unix)]
  let opened = {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options.open(path)
  };
  #[cfg(windows)]
  let opened = storage_windows::file(path.parent().expect("policy path has parent"), "device-entry-policy.json", Creation::Existing);
  let bytes = match opened {
    Ok(file) => {
      let metadata = file.metadata().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

      if !metadata.file_type().is_file() {
        return Err(DeviceEntryErrorReason::ServiceUnavailable);
      }

      #[cfg(unix)]
      if metadata.uid() != current_euid() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(DeviceEntryErrorReason::ServiceUnavailable);
      }

      let mut bytes = Vec::new();
      file.take(1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;

      if bytes.len() > 1024 * 1024 {
        return Err(DeviceEntryErrorReason::ServiceUnavailable);
      }

      bytes
    }
    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
    Err(_) => return Err(DeviceEntryErrorReason::ServiceUnavailable),
  };

  Ok(Some(bytes))
}

#[cfg(unix)]
fn current_euid() -> u32 {
  // SAFETY: geteuid has no arguments or pointers and cannot access Rust memory.
  unsafe { libc::geteuid() }
}
