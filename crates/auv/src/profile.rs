//! Connection profiles used by the domain-facing operation interface.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::AuvContext;

const CONFIG_PROFILES_ENV: &str = "AUV_CONFIG_PROFILES_FILE";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Location of the public, non-secret Device connection-profile store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileStore {
  config_path: PathBuf,
}

impl ProfileStore {
  pub fn from_path(config_path: impl Into<PathBuf>) -> Self {
    Self {
      config_path: config_path.into(),
    }
  }

  /// Uses an explicit environment override, otherwise the platform's per-user
  /// configuration directory.
  pub fn from_env() -> Result<Self, ProfileError> {
    let directories = directories::ProjectDirs::from("ai", "moeru", "auv").ok_or(ProfileError::ConfigDirectoryUnavailable)?;
    let config_path = environment_path(CONFIG_PROFILES_ENV)?.unwrap_or_else(|| directories.config_dir().join("device-profiles.json"));
    Ok(Self::from_path(config_path))
  }

  pub fn config_path(&self) -> &Path {
    &self.config_path
  }

  pub fn list_devices(&self) -> Result<Vec<ConfiguredDevice>, ProfileError> {
    let configs: ConfigDocument = read_json(&self.config_path, "config profile store")?;
    validate_unique_device_ids(&configs.profiles)?;
    configs
      .profiles
      .into_iter()
      .map(|(config_profile, profile)| {
        validate_required("config_profile", &config_profile)?;
        validate_required("device_id", &profile.device_id)?;
        profile.device_id.parse::<crate::resource::DeviceId>()?;
        let endpoint = validate_remote_endpoint(&profile.endpoint)?;
        Ok(ConfiguredDevice {
          config_profile,
          device_id: profile.device_id,
          device_name: profile.device_name,
          endpoint,
        })
      })
      .collect()
  }

  pub fn get_device(&self, name: &str) -> Result<ConfiguredDevice, ProfileError> {
    self
      .list_devices()?
      .into_iter()
      .find(|device| device.config_profile == name)
      .ok_or_else(|| ProfileError::UnknownConfigProfile(name.to_string()))
  }

  pub fn create(&self, name: &str, input: DeviceProfileInput) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write(&self.config_path)?;
    if configs.profiles.contains_key(name) {
      return Err(ProfileError::ConfigProfileAlreadyExists(name.to_string()));
    }
    validate_profile_input(name, &input)?;
    configs.profiles.insert(name.to_string(), input.into());
    validate_unique_device_ids(&configs.profiles)?;
    write_document(&self.config_path, &configs)
  }

  pub fn update(&self, name: &str, input: DeviceProfileInput) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write(&self.config_path)?;
    if !configs.profiles.contains_key(name) {
      return Err(ProfileError::UnknownConfigProfile(name.to_string()));
    }
    validate_profile_input(name, &input)?;
    configs.profiles.insert(name.to_string(), input.into());
    validate_unique_device_ids(&configs.profiles)?;
    write_document(&self.config_path, &configs)
  }

  /// Creates or replaces one named profile in a single locked write.
  pub fn upsert(&self, name: &str, input: DeviceProfileInput) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write(&self.config_path)?;
    validate_profile_input(name, &input)?;
    configs.profiles.insert(name.to_string(), input.into());
    validate_unique_device_ids(&configs.profiles)?;
    write_document(&self.config_path, &configs)
  }

  pub fn delete(&self, name: &str) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write(&self.config_path)?;
    if configs.profiles.remove(name).is_none() {
      return Err(ProfileError::UnknownConfigProfile(name.to_string()));
    }
    write_document(&self.config_path, &configs)
  }

  /// Resolves exactly one paired remote Device and its opaque bearer.
  pub fn resolve(&self, context: &AuvContext) -> Result<ResolvedRemoteProfile, ProfileError> {
    let configs: ConfigDocument = read_json(&self.config_path, "config profile store")?;
    validate_unique_device_ids(&configs.profiles)?;
    let (config_name, config) = select_config(&configs.profiles, context)?;
    config.device_id.parse::<crate::resource::DeviceId>()?;
    validate_context_matches(config_name, config, context)?;
    Ok(ResolvedRemoteProfile {
      config_profile: config_name.to_string(),
      device_id: config.device_id.clone(),
      device_name: config.device_name.clone(),
      endpoint: validate_remote_endpoint(&config.endpoint)?,
      device_credential: config.device_credential.clone(),
    })
  }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DeviceProfileInput {
  pub device_id: String,
  pub device_name: String,
  pub endpoint: String,
  pub device_credential: String,
}

impl std::fmt::Debug for DeviceProfileInput {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("DeviceProfileInput")
      .field("device_id", &self.device_id)
      .field("device_name", &self.device_name)
      .field("endpoint", &self.endpoint)
      .field("device_credential", &"[REDACTED]")
      .finish()
  }
}

/// Non-secret summary of one paired Device profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredDevice {
  config_profile: String,
  device_id: String,
  device_name: String,
  endpoint: http::Uri,
}

impl ConfiguredDevice {
  pub fn config_profile(&self) -> &str {
    &self.config_profile
  }

  pub fn device_id(&self) -> &str {
    &self.device_id
  }

  pub fn device_name(&self) -> &str {
    &self.device_name
  }

  pub fn endpoint(&self) -> &http::Uri {
    &self.endpoint
  }
}

#[derive(Clone)]
pub struct ResolvedRemoteProfile {
  config_profile: String,
  device_id: String,
  device_name: String,
  endpoint: http::Uri,
  device_credential: String,
}

impl std::fmt::Debug for ResolvedRemoteProfile {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("ResolvedRemoteProfile")
      .field("config_profile", &self.config_profile)
      .field("device_id", &self.device_id)
      .field("device_name", &self.device_name)
      .field("endpoint", &self.endpoint)
      .field("device_credential", &"[REDACTED]")
      .finish()
  }
}

impl ResolvedRemoteProfile {
  pub fn config_profile(&self) -> &str {
    &self.config_profile
  }

  pub fn device_id(&self) -> &str {
    &self.device_id
  }

  pub fn device_name(&self) -> &str {
    &self.device_name
  }

  pub fn endpoint(&self) -> &http::Uri {
    &self.endpoint
  }

  pub fn device_credential(&self) -> &str {
    &self.device_credential
  }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
  #[error("could not resolve the current user's AUV configuration directory")]
  ConfigDirectoryUnavailable,
  #[error("{name} is not valid Unicode: {source}")]
  InvalidEnvironment {
    name: &'static str,
    #[source]
    source: std::env::VarError,
  },
  #[error("failed to open {kind} {path}: {source}")]
  Open {
    kind: &'static str,
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("failed to read {kind} {path}: {source}")]
  Read {
    kind: &'static str,
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("{kind} {path} is not valid JSON: {source}")]
  Decode {
    kind: &'static str,
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("failed to encode config profile store: {0}")]
  Encode(#[source] serde_json::Error),
  #[error("failed to write {kind} {path}: {source}")]
  Write {
    kind: &'static str,
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("profile store path has no usable parent: {0}")]
  NoParent(PathBuf),
  #[error("unknown configuration profile {0:?}")]
  UnknownConfigProfile(String),
  #[error("configuration profile {0:?} already exists")]
  ConfigProfileAlreadyExists(String),
  #[error("paired remote Device selection is ambiguous; candidate IDs: {0}")]
  AmbiguousDevice(String),
  #[error("paired remote Device selection does not match a configured profile")]
  DeviceNotConfigured,
  #[error("canonical Device ID {device_id:?} is assigned to more than one config profile: {profiles}")]
  DuplicateDeviceId { device_id: String, profiles: String },
  #[error("AUV context {field} {actual:?} conflicts with profile value {expected:?}")]
  ContextConflict {
    field: &'static str,
    actual: String,
    expected: String,
  },
  #[error("paired endpoint must be an http URI with an authority and no path or query: {0:?}")]
  InvalidEndpoint(String),
  #[error("profile field {0} must not be empty")]
  EmptyField(&'static str),
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct ConfigDocument {
  profiles: BTreeMap<String, DeviceProfile>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct DeviceProfile {
  device_id: String,
  device_name: String,
  endpoint: String,
  device_credential: String,
}

impl From<DeviceProfileInput> for DeviceProfile {
  fn from(value: DeviceProfileInput) -> Self {
    Self {
      device_id: value.device_id,
      device_name: value.device_name,
      endpoint: value.endpoint,
      device_credential: value.device_credential,
    }
  }
}

fn environment_path(name: &'static str) -> Result<Option<PathBuf>, ProfileError> {
  match std::env::var(name) {
    Ok(path) => Ok(Some(PathBuf::from(path))),
    Err(std::env::VarError::NotPresent) => Ok(None),
    Err(source) => Err(ProfileError::InvalidEnvironment { name, source }),
  }
}

fn select_config<'a>(
  profiles: &'a BTreeMap<String, DeviceProfile>,
  context: &AuvContext,
) -> Result<(&'a str, &'a DeviceProfile), ProfileError> {
  if let Some(name) = context.config_profile.as_deref() {
    return profiles
      .get_key_value(name)
      .map(|(name, profile)| (name.as_str(), profile))
      .ok_or_else(|| ProfileError::UnknownConfigProfile(name.to_string()));
  }
  let matches = profiles
    .iter()
    .filter(|(_, profile)| {
      let selector = match (&context.device_id, &context.device_name) {
        (Some(id), Some(name)) => crate::resource::DeviceSelector::by_id_and_name(id, name.clone()),
        (Some(id), None) => crate::resource::DeviceSelector::by_id(id),
        (None, Some(name)) => crate::resource::DeviceSelector::by_name(name.clone()),
        (None, None) => return true,
      };
      selector.matches_wire(&profile.device_id, &profile.device_name)
    })
    .collect::<Vec<_>>();
  match matches.as_slice() {
    [(name, profile)] => Ok((name.as_str(), *profile)),
    [] => Err(ProfileError::DeviceNotConfigured),
    matches => {
      Err(ProfileError::AmbiguousDevice(matches.iter().map(|(_, profile)| profile.device_id.as_str()).collect::<Vec<_>>().join(", ")))
    }
  }
}

fn validate_unique_device_ids(profiles: &BTreeMap<String, DeviceProfile>) -> Result<(), ProfileError> {
  let mut owners = BTreeMap::<&str, Vec<&str>>::new();
  for (name, profile) in profiles {
    owners.entry(profile.device_id.as_str()).or_default().push(name.as_str());
  }
  if let Some((device_id, profiles)) = owners.into_iter().find(|(_, profiles)| profiles.len() > 1) {
    return Err(ProfileError::DuplicateDeviceId {
      device_id: device_id.to_string(),
      profiles: profiles.join(", "),
    });
  }
  Ok(())
}

fn validate_context_matches(profile_name: &str, profile: &DeviceProfile, context: &AuvContext) -> Result<(), ProfileError> {
  for (field, actual, expected) in [
    ("device_id", context.device_id.as_deref(), profile.device_id.as_str()),
    ("config_profile", context.config_profile.as_deref(), profile_name),
  ] {
    if let Some(actual) = actual
      && if field == "device_id" {
        !crate::resource::DeviceSelector::by_id(actual).matches_wire(expected, &profile.device_name)
      } else {
        actual != expected
      }
    {
      return Err(ProfileError::ContextConflict {
        field,
        actual: actual.to_string(),
        expected: expected.to_string(),
      });
    }
  }
  Ok(())
}

pub(crate) fn validate_remote_endpoint(value: &str) -> Result<http::Uri, ProfileError> {
  let uri = value.parse::<http::Uri>().map_err(|_| ProfileError::InvalidEndpoint(value.to_string()))?;
  if uri.scheme_str() != Some("http") || uri.authority().is_none() || !matches!(uri.path(), "" | "/") || uri.query().is_some() {
    return Err(ProfileError::InvalidEndpoint(value.to_string()));
  }
  Ok(uri)
}

fn validate_required(field: &'static str, value: &str) -> Result<(), ProfileError> {
  if value.trim().is_empty() {
    Err(ProfileError::EmptyField(field))
  } else {
    Ok(())
  }
}

fn validate_profile_input(name: &str, input: &DeviceProfileInput) -> Result<(), ProfileError> {
  validate_required("config_profile", name)?;
  validate_required("device_id", &input.device_id)?;
  input.device_id.parse::<crate::resource::DeviceId>()?;
  validate_required("device_credential", &input.device_credential)?;
  validate_remote_endpoint(&input.endpoint)?;
  Ok(())
}

struct MutationLock {
  _file: File,
}

impl MutationLock {
  fn acquire(config_path: &Path) -> Result<Self, ProfileError> {
    use fs2::FileExt as _;

    let parent = config_path.parent().ok_or_else(|| ProfileError::NoParent(config_path.to_path_buf()))?;
    std::fs::create_dir_all(parent).map_err(|source| ProfileError::Write {
      kind: "profile mutation lock",
      path: parent.to_path_buf(),
      source,
    })?;
    let lock_path = config_path.with_extension("lock");
    let file = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&lock_path).map_err(|source| {
      ProfileError::Write {
        kind: "profile mutation lock",
        path: lock_path.clone(),
        source,
      }
    })?;
    file.lock_exclusive().map_err(|source| ProfileError::Write {
      kind: "profile mutation lock",
      path: lock_path,
      source,
    })?;
    Ok(Self { _file: file })
  }
}

fn read_document_for_write(path: &Path) -> Result<ConfigDocument, ProfileError> {
  match read_json(path, "config profile store") {
    Ok(document) => Ok(document),
    Err(ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Ok(ConfigDocument::default()),
    Err(error) => Err(error),
  }
}

fn write_document(path: &Path, document: &ConfigDocument) -> Result<(), ProfileError> {
  let bytes = serde_json::to_vec_pretty(document).map_err(ProfileError::Encode)?;
  let parent = path.parent().ok_or_else(|| ProfileError::NoParent(path.to_path_buf()))?;
  std::fs::create_dir_all(parent).map_err(|source| ProfileError::Write {
    kind: "config profile store",
    path: parent.to_path_buf(),
    source,
  })?;
  let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
  let file_name = path.file_name().and_then(|value| value.to_str()).ok_or_else(|| ProfileError::NoParent(path.to_path_buf()))?;
  let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), sequence));
  let result = (|| {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|source| ProfileError::Write {
      kind: "config profile store",
      path: temporary.clone(),
      source,
    })?;
    file.write_all(&bytes).and_then(|_| file.write_all(b"\n")).and_then(|_| file.sync_all()).map_err(|source| ProfileError::Write {
      kind: "config profile store",
      path: temporary.clone(),
      source,
    })?;
    publish_document(&temporary, path).map_err(|source| ProfileError::Write {
      kind: "config profile store",
      path: path.to_path_buf(),
      source,
    })?;
    sync_parent(parent).map_err(|source| ProfileError::Write {
      kind: "config profile store",
      path: parent.to_path_buf(),
      source,
    })
  })();
  if result.is_err() {
    let _ = std::fs::remove_file(&temporary);
  }
  result
}

#[cfg(not(windows))]
fn publish_document(temporary: &Path, path: &Path) -> std::io::Result<()> {
  std::fs::rename(temporary, path)
}

#[cfg(windows)]
fn publish_document(temporary: &Path, path: &Path) -> std::io::Result<()> {
  use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
  use windows::core::HSTRING;

  let temporary = HSTRING::from(temporary);
  let path = HSTRING::from(path);
  // SAFETY: Both HSTRING values own valid, NUL-terminated UTF-16 paths for the
  // duration of the call. The mutation lock serializes writers, and the
  // temporary file is created beside the destination so the move stays on the
  // same volume. No pointers escape this call.
  unsafe { MoveFileExW(&temporary, &path, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) }.map_err(std::io::Error::other)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> std::io::Result<()> {
  File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> std::io::Result<()> {
  // Windows does not permit opening an ordinary directory through File::open.
  // MOVEFILE_WRITE_THROUGH above supplies the durability boundary there.
  Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &'static str) -> Result<T, ProfileError> {
  let bytes = std::fs::read(path).map_err(|source| ProfileError::Open {
    kind,
    path: path.to_path_buf(),
    source,
  })?;
  serde_json::from_slice(&bytes).map_err(|source| ProfileError::Decode {
    kind,
    path: path.to_path_buf(),
    source,
  })
}

#[cfg(test)]
#[path = "profile_test.rs"]
mod tests;
