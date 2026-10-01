//! Private persistence implementation for paired-Device authentication data.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::fs::{self, File};
#[cfg(windows)]
use std::io::Read;
use std::io::{ErrorKind, Write};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

#[cfg(windows)]
use crate::devices::storage_windows::{self, Creation};
use fs2::FileExt;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
#[cfg(windows)]
use windows::core::PCWSTR;

use super::{PairingError, PairingRecord};

const STORE_VERSION: u32 = 1;

#[cfg(windows)]
#[derive(Clone, Copy)]
enum WindowsMode {
  Ordinary,
  System,
}

pub(super) struct FileStore {
  path: PathBuf,
  _lifetime_lock: File,
  snapshot: RwLock<StoreFile>,
  mutation: Mutex<()>,
  #[cfg(windows)]
  mode: WindowsMode,
  #[cfg(windows)]
  _root_guard: Option<File>,
}

impl FileStore {
  pub(super) fn open(path: PathBuf) -> Result<Self, PairingError> {
    #[cfg(windows)]
    {
      return Self::open_windows(path, WindowsMode::Ordinary);
    }

    #[cfg(unix)]
    {
      let parent = path.parent().ok_or_else(|| update_error(&path, "pairing store path has no parent"))?;
      fs::create_dir_all(parent).map_err(|error| update_error(&path, format!("failed to create parent directory: {error}")))?;
      set_private_directory(parent)?;
      let lock_path = path.with_extension("lock");
      let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| update_error(&path, format!("failed to open lock {}: {error}", lock_path.display())))?;
      set_private_file(&lock_path)?;
      lock.try_lock_exclusive().map_err(|error| update_error(&path, format!("another pairing-store owner holds the lock: {error}")))?;
      let snapshot = read_store(&path)?;
      Ok(Self {
        path,
        _lifetime_lock: lock,
        snapshot: RwLock::new(snapshot),
        mutation: Mutex::new(()),
      })
    }
  }

  #[cfg(windows)]
  pub(super) fn open_system(path: PathBuf) -> Result<Self, PairingError> {
    Self::open_windows(path, WindowsMode::System)
  }

  #[cfg(windows)]
  fn open_windows(path: PathBuf, mode: WindowsMode) -> Result<Self, PairingError> {
    let parent = path.parent().ok_or_else(|| update_error(&path, "pairing store path has no parent"))?;
    let root_guard = match mode {
      WindowsMode::Ordinary => {
        fs::create_dir_all(parent).map_err(|error| update_error(&path, format!("failed to create parent directory: {error}")))?;
        None
      }
      WindowsMode::System => {
        let expected =
          storage_windows::root_path().map_err(|error| update_error(&path, format!("failed to locate protected root: {error}")))?;

        if path != expected.join("pairings.json") {
          return Err(update_error(&path, "Windows system pairing store must use the fixed ProgramData Device entry path"));
        }

        Some(storage_windows::directory(parent).map_err(|error| update_error(&path, format!("failed to open protected root: {error}")))?)
      }
    };
    let lock = match mode {
      WindowsMode::Ordinary => OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
        .map_err(|error| update_error(&path, format!("failed to open pairing lock: {error}")))?,
      WindowsMode::System => storage_windows::file(parent, "pairings.lock", Creation::OpenOrCreate)
        .map_err(|error| update_error(&path, format!("failed to open protected lock: {error}")))?,
    };
    lock.try_lock_exclusive().map_err(|error| update_error(&path, format!("another pairing-store owner holds the lock: {error}")))?;
    let snapshot = read_store(&path, mode)?;
    Ok(Self {
      path,
      _lifetime_lock: lock,
      snapshot: RwLock::new(snapshot),
      mutation: Mutex::new(()),
      mode,
      _root_guard: root_guard,
    })
  }

  pub(super) fn devices(&self) -> Vec<PairingRecord> {
    self.snapshot.read().expect("pairing snapshot lock poisoned").devices.clone()
  }

  pub(super) fn with_snapshot<T>(&self, read: impl FnOnce(&StoreFile) -> T) -> T {
    read(&self.snapshot.read().expect("pairing snapshot lock poisoned"))
  }

  pub(super) fn update<T>(&self, mutate: impl FnOnce(&mut StoreFile) -> Result<T, PairingError>) -> Result<T, PairingError> {
    let _mutation = self.mutation.lock().expect("pairing mutation lock poisoned");
    let mut next = self.snapshot.read().expect("pairing snapshot lock poisoned").clone();
    let result = mutate(&mut next)?;
    next.revision = next.revision.checked_add(1).ok_or_else(|| update_error(&self.path, "pairing store revision overflow"))?;
    validate_store(&next)?;
    #[cfg(unix)]
    let persistence = write_store(&self.path, &next);
    #[cfg(windows)]
    let persistence = write_store(&self.path, &next, self.mode);

    if persistence.is_ok() || matches!(persistence, Err(PairingError::CommittedButDurabilityUnknown { .. })) {
      *self.snapshot.write().expect("pairing snapshot lock poisoned") = next;
    }
    persistence.map(|()| result)
  }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct StoreFile {
  version: u32,
  pub(super) revision: u64,
  pub(super) devices: Vec<PairingRecord>,
  #[serde(default)]
  pub(super) tokens: Vec<PairingTokenRecord>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct PairingTokenRecord {
  pub(super) digest: String,
  pub(super) expires_at: Option<u64>,
}

impl Default for StoreFile {
  fn default() -> Self {
    Self {
      version: STORE_VERSION,
      revision: 0,
      devices: Vec::new(),
      tokens: Vec::new(),
    }
  }
}

fn validate_store(store: &StoreFile) -> Result<(), PairingError> {
  if store.version != STORE_VERSION {
    return Err(PairingError::UnsupportedVersion {
      version: store.version,
      path: PathBuf::from("<pairing-store>"),
    });
  }
  let mut pair_ids = HashSet::new();
  let mut credential_digests = HashMap::new();
  for record in &store.devices {
    if record.pair_id.trim().is_empty() {
      return Err(PairingError::EmptyPairId);
    }
    if !pair_ids.insert(record.pair_id.clone()) {
      return Err(PairingError::DuplicatePairId(record.pair_id.clone()));
    }
    for credential in &record.device_credentials {
      if credential.credential_sha256.len() != 64 || !credential.credential_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(update_error(Path::new("<pairing-store>"), "invalid Device credential digest"));
      }
      if let Some(existing) = credential_digests.insert(credential.credential_sha256.clone(), record.pair_id.clone()) {
        return Err(PairingError::DuplicateCredential(existing));
      }
    }
  }
  let mut token_digests = HashSet::new();
  for token in &store.tokens {
    if token.digest.len() != 64 || !token.digest.bytes().all(|byte| byte.is_ascii_hexdigit()) || !token_digests.insert(token.digest.clone())
    {
      return Err(update_error(Path::new("<pairing-store>"), "invalid or duplicate pairing token digest"));
    }
  }
  Ok(())
}

fn read_store(path: &Path, #[cfg(windows)] mode: WindowsMode) -> Result<StoreFile, PairingError> {
  #[cfg(unix)]
  let opened = read_store_bytes(path);
  #[cfg(windows)]
  let opened = read_store_bytes(path, mode);
  let bytes = match opened {
    Ok(bytes) => bytes,
    Err(source) if source.kind() == ErrorKind::NotFound => return Ok(StoreFile::default()),
    Err(source) => {
      return Err(PairingError::Read {
        path: path.to_path_buf(),
        source,
      });
    }
  };
  let store = serde_json::from_slice::<StoreFile>(&bytes).map_err(|source| PairingError::Decode {
    path: path.to_path_buf(),
    source,
  })?;
  if store.version != STORE_VERSION {
    return Err(PairingError::UnsupportedVersion {
      version: store.version,
      path: path.to_path_buf(),
    });
  }
  validate_store(&store)?;
  Ok(store)
}

#[cfg(unix)]
fn read_store_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
  fs::read(path)
}

#[cfg(windows)]
fn read_store_bytes(path: &Path, mode: WindowsMode) -> std::io::Result<Vec<u8>> {
  match mode {
    WindowsMode::Ordinary => fs::read(path),
    WindowsMode::System => {
      let root = path.parent().ok_or_else(|| std::io::Error::from(ErrorKind::InvalidInput))?;
      let file = storage_windows::file(root, "pairings.json", Creation::Existing)?;
      let mut bytes = Vec::new();
      file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;

      if bytes.len() > 1024 * 1024 {
        return Err(std::io::Error::from(ErrorKind::InvalidData));
      }

      Ok(bytes)
    }
  }
}

fn write_store(path: &Path, store: &StoreFile, #[cfg(windows)] mode: WindowsMode) -> Result<(), PairingError> {
  let temporary_path = path.with_extension(format!("tmp-{}", uuid::Uuid::now_v7()));
  let mut temporary = TemporaryStore::new(temporary_path.clone());
  #[cfg(unix)]
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&temporary_path)
    .map_err(|error| update_error(path, format!("failed to create temporary store: {error}")))?;
  #[cfg(windows)]
  let mut file = match mode {
    WindowsMode::Ordinary => OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&temporary_path)
      .map_err(|error| update_error(path, format!("failed to create temporary store: {error}")))?,
    WindowsMode::System => storage_windows::file(
      path.parent().expect("validated pairing-store parent"),
      temporary_path.file_name().expect("temporary child name").to_str().expect("ASCII temporary child name"),
      Creation::New,
    )
    .map_err(|error| update_error(path, format!("failed to create protected temporary store: {error}")))?,
  };
  #[cfg(unix)]
  set_private_file(&temporary_path)?;
  serde_json::to_writer_pretty(&mut file, store).map_err(|error| update_error(path, format!("failed to encode store: {error}")))?;
  file.write_all(b"\n").and_then(|_| file.sync_all()).map_err(|error| update_error(path, format!("failed to sync store: {error}")))?;
  drop(file);
  #[cfg(unix)]
  fs::rename(&temporary_path, path).map_err(|error| update_error(path, format!("failed to publish store: {error}")))?;
  #[cfg(windows)]
  match mode {
    WindowsMode::Ordinary => {
      let source: Vec<u16> = temporary_path.as_os_str().encode_wide().chain(Some(0)).collect();
      let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
      // NOTICE(pairing-windows-publish): std::fs cannot open a directory for
      // fsync on Windows. Publish the synced file with WRITE_THROUGH, as the
      // protected Device entry store does. Revisit if a reviewed directory
      // durability primitive becomes available.
      // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
      // SAFETY: Both NUL-terminated paths remain live for this synchronous
      // call, and the temporary file has been closed after sync_all.
      unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr()), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }
        .map_err(|error| update_error(path, format!("failed to publish store: {error}")))?;
    }
    WindowsMode::System => storage_windows::replace(
      path.parent().expect("validated pairing-store parent"),
      temporary_path.file_name().expect("temporary child name").to_str().expect("ASCII temporary child name"),
      "pairings.json",
    )
    .map_err(|error| update_error(path, format!("failed to publish protected store: {error}")))?,
  };
  temporary.committed = true;
  #[cfg(unix)]
  {
    let parent = path.parent().expect("validated pairing-store parent");
    File::open(parent).and_then(|directory| directory.sync_all()).map_err(|error| PairingError::CommittedButDurabilityUnknown {
      revision: store.revision,
      message: error.to_string(),
    })
  }

  #[cfg(windows)]
  Ok(())
}

struct TemporaryStore {
  path: PathBuf,
  committed: bool,
}

impl TemporaryStore {
  fn new(path: PathBuf) -> Self {
    Self {
      path,
      committed: false,
    }
  }
}

impl Drop for TemporaryStore {
  fn drop(&mut self) {
    if !self.committed {
      let _ = fs::remove_file(&self.path);
    }
  }
}

fn update_error(path: &Path, message: impl Into<String>) -> PairingError {
  PairingError::Update {
    path: path.to_path_buf(),
    message: message.into(),
  }
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), PairingError> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    .map_err(|error| update_error(path, format!("failed to set directory permissions: {error}")))
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), PairingError> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|error| update_error(path, format!("failed to set file permissions: {error}")))
}

#[cfg(all(test, windows))]
mod windows_tests {
  use super::*;

  #[test]
  fn ordinary_pairing_open_accepts_foreground_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairings.json");
    let store = FileStore::open(path.clone()).unwrap();

    assert!(matches!(store.mode, WindowsMode::Ordinary));

    let result = store.update(|snapshot| {
      snapshot.tokens.push(PairingTokenRecord {
        digest: "a".repeat(64),
        expires_at: None,
      });
      Ok(())
    });
    // ROOT CAUSE:
    //
    // On Windows, ordinary pairing published the file, then tried to open its
    // parent directory through std::fs. That returned AccessDenied, so token
    // creation failed even though the new revision was already committed.
    // The publish operation must report success after a durable replacement.
    assert!(result.is_ok(), "ordinary pairing update failed: {result:?}");
    assert!(fs::read_to_string(path).unwrap().contains(&"a".repeat(64)));
  }

  #[test]
  fn system_pairing_open_rejects_foreground_path_without_creating_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairings.json");

    assert!(FileStore::open_system(path).is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
  }

  #[test]
  fn system_mode_rejects_unprotected_pairing_read_and_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairings.json");
    fs::write(&path, b"{}\n").unwrap();

    assert!(read_store_bytes(&path, WindowsMode::System).is_err());
    assert!(write_store(&path, &StoreFile::default(), WindowsMode::System).is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
  }
}
