//! Private persistence implementation for paired-Device authentication data.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use fs2::FileExt;

use super::pairing::{PairingError, PairingRecord};

const STORE_VERSION: u32 = 1;

pub(super) struct FileStore {
  path: PathBuf,
  _lifetime_lock: File,
  snapshot: RwLock<StoreFile>,
  mutation: Mutex<()>,
}

impl FileStore {
  pub(super) fn open(path: PathBuf) -> Result<Self, PairingError> {
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
    lock
      .try_lock_exclusive()
      .map_err(|error| update_error(&path, format!("another pairing-store owner holds {}: {error}", lock_path.display())))?;
    let snapshot = read_store(&path)?;
    Ok(Self {
      path,
      _lifetime_lock: lock,
      snapshot: RwLock::new(snapshot),
      mutation: Mutex::new(()),
    })
  }

  pub(super) fn path(&self) -> &Path {
    &self.path
  }

  pub(super) fn revision(&self) -> u64 {
    self.snapshot.read().expect("pairing snapshot lock poisoned").revision
  }

  pub(super) fn devices(&self) -> Vec<PairingRecord> {
    self.snapshot.read().expect("pairing snapshot lock poisoned").devices.clone()
  }

  pub(super) fn with_snapshot<T>(&self, read: impl FnOnce(&StoreFile) -> T) -> T {
    read(&self.snapshot.read().expect("pairing snapshot lock poisoned"))
  }

  pub(super) fn update(&self, mutate: impl FnOnce(&mut StoreFile) -> Result<(), PairingError>) -> Result<(), PairingError> {
    let _mutation = self.mutation.lock().expect("pairing mutation lock poisoned");
    let mut next = self.snapshot.read().expect("pairing snapshot lock poisoned").clone();
    mutate(&mut next)?;
    next.revision = next.revision.checked_add(1).ok_or_else(|| update_error(&self.path, "pairing store revision overflow"))?;
    validate_store(&next)?;
    let persistence = write_store(&self.path, &next);
    if persistence.is_ok() || matches!(persistence, Err(PairingError::CommittedButDurabilityUnknown { .. })) {
      *self.snapshot.write().expect("pairing snapshot lock poisoned") = next;
    }
    persistence
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

fn read_store(path: &Path) -> Result<StoreFile, PairingError> {
  let bytes = match fs::read(path) {
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

fn write_store(path: &Path, store: &StoreFile) -> Result<(), PairingError> {
  let temporary_path = path.with_extension(format!("tmp-{}", uuid::Uuid::now_v7()));
  let mut temporary = TemporaryStore::new(temporary_path.clone());
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&temporary_path)
    .map_err(|error| update_error(path, format!("failed to create temporary store: {error}")))?;
  set_private_file(&temporary_path)?;
  serde_json::to_writer_pretty(&mut file, store).map_err(|error| update_error(path, format!("failed to encode store: {error}")))?;
  file.write_all(b"\n").and_then(|_| file.sync_all()).map_err(|error| update_error(path, format!("failed to sync store: {error}")))?;
  drop(file);
  fs::rename(&temporary_path, path).map_err(|error| update_error(path, format!("failed to publish store: {error}")))?;
  temporary.committed = true;
  let parent = path.parent().expect("validated pairing-store parent");
  File::open(parent).and_then(|directory| directory.sync_all()).map_err(|error| PairingError::CommittedButDurabilityUnknown {
    revision: store.revision,
    message: error.to_string(),
  })
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

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), PairingError> {
  Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), PairingError> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    .map_err(|error| update_error(path, format!("failed to set file permissions: {error}")))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), PairingError> {
  Ok(())
}
