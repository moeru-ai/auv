//! Durable certificate authority records for paired remote clients.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use fs2::FileExt;
use rustls_pki_types::{CertificateDer, pem::PemObject};
use sha2::{Digest, Sha256};

const STORE_VERSION: u32 = 1;

/// Authenticated caller identity used by control and capability admission.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PrincipalId(String);

impl PrincipalId {
  pub fn local_owner() -> Self {
    Self("local-owner".to_string())
  }

  pub(crate) fn paired_device(pair_id: &str) -> Self {
    Self(format!("paired-device:{pair_id}"))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }

  pub(crate) fn is_local_owner(&self) -> bool {
    self == &Self::local_owner()
  }
}

/// Capability checked at the transport boundary before entering a handler.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiScope {
  ControlInspect,
  ControlManage,
  OperationsExecute,
}

/// Canonical SHA-256 digest of one leaf certificate's DER bytes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CertificateFingerprint(String);

impl CertificateFingerprint {
  pub fn from_der(certificate_der: &[u8]) -> Self {
    Self(hex::encode(Sha256::digest(certificate_der)))
  }

  pub fn from_pem(certificate_pem: &[u8]) -> Result<Self, PairingError> {
    let certificate =
      CertificateDer::from_pem_slice(certificate_pem).map_err(|error| PairingError::InvalidCertificatePem(error.to_string()))?;
    Ok(Self::from_der(certificate.as_ref()))
  }

  pub fn parse(value: impl Into<String>) -> Result<Self, PairingError> {
    let value = value.into().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return Err(PairingError::InvalidFingerprint(value));
    }
    Ok(Self(value))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl TryFrom<String> for CertificateFingerprint {
  type Error = PairingError;

  fn try_from(value: String) -> Result<Self, Self::Error> {
    Self::parse(value)
  }
}

impl From<CertificateFingerprint> for String {
  fn from(value: CertificateFingerprint) -> Self {
    value.0
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
  Active,
  Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PairingCredential {
  pub certificate_fingerprint: CertificateFingerprint,
  pub state: CredentialState,
}

/// Stable paired-device identity. Multiple credentials allow certificate
/// rotation without orphaning resources owned by the pair ID.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct PairingRecord {
  pub pair_id: String,
  pub label: String,
  pub enabled: bool,
  pub scopes: Vec<ApiScope>,
  pub credentials: Vec<PairingCredential>,
}

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
  #[error("pair_id must not be empty")]
  EmptyPairId,
  #[error("paired device must have at least one credential: {0}")]
  MissingCredential(String),
  #[error("invalid certificate SHA-256 fingerprint: {0}")]
  InvalidFingerprint(String),
  #[error("invalid PEM leaf certificate: {0}")]
  InvalidCertificatePem(String),
  #[error("paired device was not found: {0}")]
  UnknownPair(String),
  #[error("certificate credential was not found")]
  UnknownCredential,
  #[error("certificate is not paired or has been revoked")]
  Unauthenticated,
  #[error("paired device {pair_id} lacks required scope {scope:?}")]
  MissingScope { pair_id: String, scope: ApiScope },
  #[error("duplicate paired-device ID: {0}")]
  DuplicatePairId(String),
  #[error("certificate fingerprint is already assigned to paired device {0}")]
  DuplicateFingerprint(String),
  #[error("failed to read pairing store {path}: {source}")]
  Read {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("invalid pairing store {path}: {source}")]
  Decode {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("unsupported pairing store version {version} in {path}")]
  UnsupportedVersion { version: u32, path: PathBuf },
  #[error("failed to update pairing store {path}: {message}")]
  Update { path: PathBuf, message: String },
  #[error("pairing store revision {revision} committed, but directory durability could not be confirmed: {message}")]
  CommittedButDurabilityUnknown { revision: u64, message: String },
}

/// Process-owned pairing store with lock-free filesystem authorization reads.
#[derive(Clone)]
pub struct PairingStore {
  inner: Arc<PairingStoreInner>,
}

struct PairingStoreInner {
  path: PathBuf,
  _lifetime_lock: File,
  snapshot: RwLock<StoreFile>,
  mutation: Mutex<()>,
}

impl PairingStore {
  pub fn open(path: PathBuf) -> Result<Self, PairingError> {
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
      inner: Arc::new(PairingStoreInner {
        path,
        _lifetime_lock: lock,
        snapshot: RwLock::new(snapshot),
        mutation: Mutex::new(()),
      }),
    })
  }

  pub fn path(&self) -> &Path {
    &self.inner.path
  }

  pub fn revision(&self) -> u64 {
    self.inner.snapshot.read().expect("pairing snapshot lock poisoned").revision
  }

  pub fn list(&self) -> Vec<PairingRecord> {
    self.inner.snapshot.read().expect("pairing snapshot lock poisoned").devices.clone()
  }

  /// Authorizes against the current immutable snapshot. Mutations replace the
  /// snapshot after atomic persistence, so revocation affects the next RPC and
  /// this hot path performs no filesystem I/O.
  pub fn authorize_der(&self, certificate_der: &[u8], required_scope: ApiScope) -> Result<PrincipalId, PairingError> {
    self.authorize_fingerprint(&CertificateFingerprint::from_der(certificate_der), required_scope)
  }

  pub fn authorize_fingerprint(&self, fingerprint: &CertificateFingerprint, required_scope: ApiScope) -> Result<PrincipalId, PairingError> {
    let snapshot = self.inner.snapshot.read().expect("pairing snapshot lock poisoned");
    let record = snapshot
      .devices
      .iter()
      .find(|record| record.credentials.iter().any(|credential| &credential.certificate_fingerprint == fingerprint))
      .ok_or(PairingError::Unauthenticated)?;
    let credential =
      record.credentials.iter().find(|credential| &credential.certificate_fingerprint == fingerprint).expect("matched credential exists");
    if !record.enabled || credential.state != CredentialState::Active {
      return Err(PairingError::Unauthenticated);
    }
    if !record.scopes.contains(&required_scope) {
      return Err(PairingError::MissingScope {
        pair_id: record.pair_id.clone(),
        scope: required_scope,
      });
    }
    Ok(PrincipalId::paired_device(&record.pair_id))
  }

  pub fn upsert(&self, mut record: PairingRecord) -> Result<(), PairingError> {
    normalize_record(&mut record)?;
    self.update(|store| {
      match store.devices.iter_mut().find(|existing| existing.pair_id == record.pair_id) {
        Some(existing) => *existing = record,
        None => store.devices.push(record),
      }
      Ok(())
    })
  }

  pub fn insert(&self, mut record: PairingRecord) -> Result<(), PairingError> {
    normalize_record(&mut record)?;
    self.update(|store| {
      if store.devices.iter().any(|existing| existing.pair_id == record.pair_id) {
        return Err(PairingError::DuplicatePairId(record.pair_id.clone()));
      }
      store.devices.push(record);
      Ok(())
    })
  }

  pub fn set_enabled(&self, pair_id: &str, enabled: bool) -> Result<(), PairingError> {
    self.update(|store| {
      let record =
        store.devices.iter_mut().find(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      record.enabled = enabled;
      Ok(())
    })
  }

  /// Removes one paired Device and all credentials owned by its stable ID.
  pub fn remove_pair(&self, pair_id: &str) -> Result<(), PairingError> {
    self.update(|store| {
      let index =
        store.devices.iter().position(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      store.devices.remove(index);
      Ok(())
    })
  }

  pub fn set_scopes(&self, pair_id: &str, scopes: Vec<ApiScope>) -> Result<(), PairingError> {
    self.update(|store| {
      let record =
        store.devices.iter_mut().find(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      record.scopes = scopes;
      Ok(())
    })
  }

  pub fn add_credential(&self, pair_id: &str, certificate_fingerprint: CertificateFingerprint) -> Result<(), PairingError> {
    self.update(|store| {
      let record =
        store.devices.iter_mut().find(|record| record.pair_id == pair_id).ok_or_else(|| PairingError::UnknownPair(pair_id.to_string()))?;
      record.credentials.push(PairingCredential {
        certificate_fingerprint,
        state: CredentialState::Active,
      });
      Ok(())
    })
  }

  pub fn revoke_credential(&self, fingerprint: &CertificateFingerprint) -> Result<(), PairingError> {
    self.update(|store| {
      let credential = store
        .devices
        .iter_mut()
        .flat_map(|record| &mut record.credentials)
        .find(|credential| &credential.certificate_fingerprint == fingerprint)
        .ok_or(PairingError::UnknownCredential)?;
      credential.state = CredentialState::Revoked;
      Ok(())
    })
  }

  fn update(&self, mutate: impl FnOnce(&mut StoreFile) -> Result<(), PairingError>) -> Result<(), PairingError> {
    let _mutation = self.inner.mutation.lock().expect("pairing mutation lock poisoned");
    let mut next = self.inner.snapshot.read().expect("pairing snapshot lock poisoned").clone();
    mutate(&mut next)?;
    next.revision = next.revision.checked_add(1).ok_or_else(|| update_error(&self.inner.path, "pairing store revision overflow"))?;
    validate_store(&mut next)?;
    let persistence = write_store(&self.inner.path, &next);
    if persistence.is_ok() || matches!(persistence, Err(PairingError::CommittedButDurabilityUnknown { .. })) {
      *self.inner.snapshot.write().expect("pairing snapshot lock poisoned") = next;
    }
    persistence
  }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct StoreFile {
  version: u32,
  revision: u64,
  devices: Vec<PairingRecord>,
}

impl Default for StoreFile {
  fn default() -> Self {
    Self {
      version: STORE_VERSION,
      revision: 0,
      devices: Vec::new(),
    }
  }
}

fn normalize_record(record: &mut PairingRecord) -> Result<(), PairingError> {
  if record.pair_id.trim().is_empty() {
    return Err(PairingError::EmptyPairId);
  }
  if record.credentials.is_empty() {
    return Err(PairingError::MissingCredential(record.pair_id.clone()));
  }
  record.scopes.sort_unstable();
  record.scopes.dedup();
  record.credentials.sort_by(|left, right| left.certificate_fingerprint.cmp(&right.certificate_fingerprint));
  record.credentials.dedup_by(|left, right| left.certificate_fingerprint == right.certificate_fingerprint);
  Ok(())
}

fn validate_store(store: &mut StoreFile) -> Result<(), PairingError> {
  if store.version != STORE_VERSION {
    return Err(PairingError::UnsupportedVersion {
      version: store.version,
      path: PathBuf::from("<pairing-store>"),
    });
  }
  store.devices.sort_by(|left, right| left.pair_id.cmp(&right.pair_id));
  let mut pair_ids = HashSet::new();
  let mut fingerprints = std::collections::HashMap::new();
  for record in &mut store.devices {
    normalize_record(record)?;
    if !pair_ids.insert(record.pair_id.clone()) {
      return Err(PairingError::DuplicatePairId(record.pair_id.clone()));
    }
    for credential in &record.credentials {
      if let Some(existing) = fingerprints.insert(credential.certificate_fingerprint.clone(), record.pair_id.clone()) {
        return Err(PairingError::DuplicateFingerprint(existing));
      }
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
  let mut store = serde_json::from_slice::<StoreFile>(&bytes).map_err(|source| PairingError::Decode {
    path: path.to_path_buf(),
    source,
  })?;
  if store.version != STORE_VERSION {
    return Err(PairingError::UnsupportedVersion {
      version: store.version,
      path: path.to_path_buf(),
    });
  }
  validate_store(&mut store)?;
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

#[cfg(test)]
mod tests {
  use super::*;

  fn credential(der: &[u8]) -> PairingCredential {
    PairingCredential {
      certificate_fingerprint: CertificateFingerprint::from_der(der),
      state: CredentialState::Active,
    }
  }

  fn record(pair_id: &str, credentials: Vec<PairingCredential>, scopes: Vec<ApiScope>) -> PairingRecord {
    PairingRecord {
      pair_id: pair_id.to_string(),
      label: format!("{pair_id} device"),
      enabled: true,
      scopes,
      credentials,
    }
  }

  #[test]
  fn rotation_preserves_principal_and_revocation_is_live() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairs.json");
    let store = PairingStore::open(path.clone()).unwrap();
    store
      .upsert(record(
        "tablet",
        vec![credential(b"old-cert"), credential(b"new-cert")],
        vec![ApiScope::ControlManage, ApiScope::OperationsExecute],
      ))
      .unwrap();
    let old = store.authorize_der(b"old-cert", ApiScope::OperationsExecute).unwrap();
    let new = store.authorize_der(b"new-cert", ApiScope::OperationsExecute).unwrap();
    assert_eq!(old, new);
    assert_eq!(old.as_str(), "paired-device:tablet");

    store.revoke_credential(&CertificateFingerprint::from_der(b"old-cert")).unwrap();
    assert!(matches!(store.authorize_der(b"old-cert", ApiScope::OperationsExecute), Err(PairingError::Unauthenticated)));
    assert_eq!(store.authorize_der(b"new-cert", ApiScope::OperationsExecute).unwrap(), new);

    drop(store);
    let reopened = PairingStore::open(path).unwrap();
    assert_eq!(reopened.revision(), 2);
    assert_eq!(reopened.authorize_der(b"new-cert", ApiScope::OperationsExecute).unwrap(), new);
  }

  #[test]
  fn scopes_disable_duplicates_lock_and_permissions_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairs.json");
    let store = PairingStore::open(path.clone()).unwrap();
    store.upsert(record("tablet", vec![credential(b"tablet")], vec![ApiScope::ControlInspect])).unwrap();
    assert!(matches!(store.authorize_der(b"tablet", ApiScope::ControlManage), Err(PairingError::MissingScope { .. })));
    assert!(PairingStore::open(path.clone()).is_err());
    assert!(matches!(
      store.upsert(record("other", vec![credential(b"tablet")], vec![ApiScope::ControlInspect])),
      Err(PairingError::DuplicateFingerprint(_))
    ));
    store.set_enabled("tablet", false).unwrap();
    assert!(matches!(store.authorize_der(b"tablet", ApiScope::ControlInspect), Err(PairingError::Unauthenticated)));
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }
  }

  #[test]
  fn removing_a_pair_revokes_its_whole_device_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pairs.json");
    let store = PairingStore::open(path.clone()).unwrap();
    store.upsert(record("tablet", vec![credential(b"tablet")], vec![ApiScope::ControlInspect])).unwrap();

    store.remove_pair("tablet").unwrap();

    assert!(store.list().is_empty());
    assert!(matches!(store.authorize_der(b"tablet", ApiScope::ControlInspect), Err(PairingError::Unauthenticated)));
    assert!(matches!(store.remove_pair("tablet"), Err(PairingError::UnknownPair(pair_id)) if pair_id == "tablet"));
    drop(store);
    assert!(PairingStore::open(path).unwrap().list().is_empty());
  }

  #[test]
  fn pem_fingerprint_and_inserted_rotation_use_leaf_der_identity() {
    let certified = rcgen::generate_simple_self_signed(vec!["paired-client".to_string()]).unwrap();
    let fingerprint = CertificateFingerprint::from_pem(certified.cert.pem().as_bytes()).unwrap();
    assert_eq!(fingerprint, CertificateFingerprint::from_der(certified.cert.der().as_ref()));

    let directory = tempfile::tempdir().unwrap();
    let store = PairingStore::open(directory.path().join("pairs.json")).unwrap();
    store
      .insert(record(
        "tablet",
        vec![PairingCredential {
          certificate_fingerprint: fingerprint,
          state: CredentialState::Active,
        }],
        vec![ApiScope::ControlInspect],
      ))
      .unwrap();
    assert!(matches!(
      store.insert(record("tablet", vec![credential(b"duplicate-pair")], vec![ApiScope::ControlInspect])),
      Err(PairingError::DuplicatePairId(_))
    ));
    store.add_credential("tablet", CertificateFingerprint::from_der(b"replacement")).unwrap();
    assert_eq!(store.authorize_der(b"replacement", ApiScope::ControlInspect).unwrap().as_str(), "paired-device:tablet");
  }
}
