use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use auv_driver_common::error::DriverResult;
use fs2::FileExt;

use crate::error::backend;

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestoreTokenKind {
  ScreenCast,
  RemoteDesktopInput,
}

impl RestoreTokenKind {
  fn file_name(self) -> &'static str {
    match self {
      Self::ScreenCast => "screencast-token",
      Self::RemoteDesktopInput => "remote-desktop-input-token",
    }
  }
}

/// Serializes one portal restore-token use and publishes the replacement token
/// atomically. Portal tokens are single-use, so the lock intentionally spans
/// the restore request and the durable rotation.
#[derive(Clone, Debug)]
pub(crate) struct RestoreTokenStore {
  root: PathBuf,
}

impl RestoreTokenStore {
  pub(crate) fn new(root: PathBuf) -> Self {
    Self { root }
  }

  pub(super) fn rotate<T>(
    &self,
    kind: RestoreTokenKind,
    operation: impl FnOnce(Option<&str>) -> DriverResult<(T, Option<String>)>,
  ) -> DriverResult<T> {
    create_private_directory(&self.root)?;
    let lock_path = self.root.join(format!("{}.lock", kind.file_name()));
    let lock = OpenOptions::new()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(&lock_path)
      .map_err(|error| backend(format!("failed to open portal restore-token lock {}: {error}", lock_path.display())))?;
    set_private_file(&lock_path)?;
    lock.lock_exclusive().map_err(|error| backend(format!("failed to lock portal restore-token store {}: {error}", lock_path.display())))?;

    let token_path = self.root.join(kind.file_name());
    let current = read_token(&token_path)?;
    let result = operation(current.as_deref());
    let result = match result {
      Ok((value, replacement)) => {
        replace_token(&token_path, replacement.as_deref())?;
        Ok(value)
      }
      Err(error) => Err(error),
    };
    let unlock = FileExt::unlock(&lock)
      .map_err(|error| backend(format!("failed to unlock portal restore-token store {}: {error}", lock_path.display())));
    match (result, unlock) {
      (Ok(value), Ok(())) => Ok(value),
      (Err(error), Ok(())) => Err(error),
      (Ok(_), Err(error)) => Err(error),
      (Err(error), Err(unlock_error)) => Err(backend(format!("{error}; additionally {unlock_error}"))),
    }
  }
}

fn read_token(path: &Path) -> DriverResult<Option<String>> {
  match fs::read_to_string(path) {
    Ok(token) if token.is_empty() => Err(backend(format!("portal restore-token file {} is empty", path.display()))),
    Ok(token) => Ok(Some(token)),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
    Err(error) => Err(backend(format!("failed to read portal restore token {}: {error}", path.display()))),
  }
}

fn replace_token(path: &Path, replacement: Option<&str>) -> DriverResult<()> {
  let Some(replacement) = replacement else {
    return match fs::remove_file(path) {
      Ok(()) => Ok(()),
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(error) => Err(backend(format!("failed to remove consumed portal restore token {}: {error}", path.display()))),
    };
  };
  if replacement.is_empty() {
    return Err(backend("portal returned an empty restore token"));
  }
  let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
  let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
  let write_result = (|| {
    let mut file = OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&temporary)
      .map_err(|error| backend(format!("failed to create portal restore-token temporary file {}: {error}", temporary.display())))?;
    set_private_file(&temporary)?;
    file
      .write_all(replacement.as_bytes())
      .map_err(|error| backend(format!("failed to write portal restore-token temporary file {}: {error}", temporary.display())))?;
    file
      .sync_all()
      .map_err(|error| backend(format!("failed to sync portal restore-token temporary file {}: {error}", temporary.display())))?;
    fs::rename(&temporary, path).map_err(|error| backend(format!("failed to publish portal restore token {}: {error}", path.display())))?;
    Ok(())
  })();
  if write_result.is_err() {
    let _ = fs::remove_file(&temporary);
  }
  write_result
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> DriverResult<()> {
  use std::os::unix::fs::PermissionsExt;

  fs::create_dir_all(path).map_err(|error| backend(format!("failed to create portal state directory {}: {error}", path.display())))?;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    .map_err(|error| backend(format!("failed to protect portal state directory {}: {error}", path.display())))
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> DriverResult<()> {
  use std::os::unix::fs::PermissionsExt;

  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|error| backend(format!("failed to protect portal state file {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn successful_restore_rotates_the_single_use_token_without_a_version_wrapper() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let store = RestoreTokenStore::new(temporary.path().to_path_buf());

    store
      .rotate(RestoreTokenKind::ScreenCast, |current| {
        assert_eq!(current, None);
        Ok(((), Some("first-opaque-token".to_string())))
      })
      .expect("first authorization token");
    store
      .rotate(RestoreTokenKind::ScreenCast, |current| {
        assert_eq!(current, Some("first-opaque-token"));
        Ok(((), Some("replacement-opaque-token".to_string())))
      })
      .expect("restored authorization token");

    assert_eq!(fs::read_to_string(temporary.path().join("screencast-token")).unwrap(), "replacement-opaque-token");
  }

  #[test]
  fn failed_restore_keeps_the_previous_token_for_a_later_retry() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let store = RestoreTokenStore::new(temporary.path().to_path_buf());
    store.rotate(RestoreTokenKind::RemoteDesktopInput, |_| Ok(((), Some("input-token".to_string())))).expect("initial token");

    let error = store
      .rotate::<()>(RestoreTokenKind::RemoteDesktopInput, |current| {
        assert_eq!(current, Some("input-token"));
        Err(backend("portal request failed"))
      })
      .expect_err("failed portal request");

    assert!(error.to_string().contains("portal request failed"));
    assert_eq!(fs::read_to_string(temporary.path().join("remote-desktop-input-token")).unwrap(), "input-token");
  }

  #[test]
  fn successful_start_without_a_replacement_removes_a_consumed_token() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let store = RestoreTokenStore::new(temporary.path().to_path_buf());
    store.rotate(RestoreTokenKind::ScreenCast, |_| Ok(((), Some("old-token".to_string())))).unwrap();

    store
      .rotate(RestoreTokenKind::ScreenCast, |current| {
        assert_eq!(current, Some("old-token"));
        Ok(((), None))
      })
      .expect("successful non-persistent start");

    assert!(!temporary.path().join("screencast-token").exists());
  }
}
