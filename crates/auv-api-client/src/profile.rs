//! Paired remote Device profiles and owner-only credential references.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::AuvContext;

const MAX_PROFILE_STORE_BYTES: u64 = 1024 * 1024;
const MAX_CREDENTIAL_MATERIAL_BYTES: u64 = 1024 * 1024;
const CONFIG_PROFILES_ENV: &str = "AUV_CONFIG_PROFILES_FILE";
const CREDENTIAL_PROFILES_ENV: &str = "AUV_CREDENTIAL_PROFILES_FILE";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Locations of the non-secret Device profile store and credential-reference
/// store. Credential material remains in separate owner-only files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileStore {
  config_path: PathBuf,
  credential_path: PathBuf,
}

impl ProfileStore {
  pub fn from_paths(config_path: impl Into<PathBuf>, credential_path: impl Into<PathBuf>) -> Self {
    Self {
      config_path: config_path.into(),
      credential_path: credential_path.into(),
    }
  }

  /// Uses explicit environment overrides, otherwise the platform's per-user
  /// configuration directory.
  pub fn from_env() -> Result<Self, ProfileError> {
    let directories = directories::ProjectDirs::from("ai", "moeru", "auv").ok_or(ProfileError::ConfigDirectoryUnavailable)?;
    let config_path = environment_path(CONFIG_PROFILES_ENV)?.unwrap_or_else(|| directories.config_dir().join("device-profiles.json"));
    let credential_path =
      environment_path(CREDENTIAL_PROFILES_ENV)?.unwrap_or_else(|| directories.config_dir().join("credential-profiles.json"));
    Ok(Self::from_paths(config_path, credential_path))
  }

  pub fn config_path(&self) -> &Path {
    &self.config_path
  }

  pub fn credential_path(&self) -> &Path {
    &self.credential_path
  }

  /// Lists non-secret configured Device summaries without opening the
  /// credential-reference store or any credential material.
  pub fn list_devices(&self) -> Result<Vec<ConfiguredDevice>, ProfileError> {
    let configs: ConfigDocument = read_json(&self.config_path, MAX_PROFILE_STORE_BYTES, "config profile store")?;
    validate_unique_device_ids(&configs.profiles)?;
    configs
      .profiles
      .into_iter()
      .map(|(config_profile, profile)| {
        validate_required("config_profile", &config_profile)?;
        validate_required("device_id", &profile.device_id)?;
        validate_required("device_name", &profile.device_name)?;
        validate_required("credential_profile", &profile.credential_profile)?;
        validate_server_name(&profile.server_name)?;
        let endpoint = validate_remote_endpoint(&profile.endpoint)?;
        Ok(ConfiguredDevice {
          config_profile,
          credential_profile: profile.credential_profile,
          device_id: profile.device_id,
          device_name: profile.device_name,
          endpoint,
          server_name: profile.server_name,
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

  /// Creates a paired Device profile and, optionally, its credential-path
  /// binding. Credential file contents are never copied into either store.
  pub fn create(&self, name: &str, input: DeviceProfileInput, credentials: Option<CredentialProfileInput>) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write::<ConfigDocument>(&self.config_path, "config profile store")?;
    if configs.profiles.contains_key(name) {
      return Err(ProfileError::ConfigProfileAlreadyExists(name.to_string()));
    }
    validate_profile_input(name, &input, credentials.as_ref())?;
    let mut credential_document = read_document_for_write::<CredentialDocument>(&self.credential_path, "credential profile store")?;
    let original_credentials = serde_json::to_vec(&credential_document).expect("credential document is serializable");
    match credentials {
      Some(_) if credential_document.profiles.contains_key(&input.credential_profile) => {
        return Err(ProfileError::CredentialProfileAlreadyExists(input.credential_profile));
      }
      Some(credentials) => {
        credential_document.profiles.insert(input.credential_profile.clone(), credentials.into());
      }
      None if !credential_document.profiles.contains_key(&input.credential_profile) => {
        return Err(ProfileError::UnknownCredentialProfile(input.credential_profile));
      }
      None => {}
    }
    configs.profiles.insert(name.to_string(), input.into());
    validate_unique_device_ids(&configs.profiles)?;
    write_document(&self.credential_path, &credential_document, "credential profile store")?;
    if let Err(error) = write_document(&self.config_path, &configs, "config profile store") {
      let rollback: CredentialDocument = serde_json::from_slice(&original_credentials).expect("saved credential document decodes");
      write_document(&self.credential_path, &rollback, "credential profile rollback").map_err(|rollback| ProfileError::Rollback {
        operation: error.to_string(),
        rollback: rollback.to_string(),
      })?;
      return Err(error);
    }
    Ok(())
  }

  /// Replaces an existing paired Device profile. Supplying credentials creates
  /// or replaces only the named credential-path binding.
  pub fn update(&self, name: &str, input: DeviceProfileInput, credentials: Option<CredentialProfileInput>) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write::<ConfigDocument>(&self.config_path, "config profile store")?;
    if !configs.profiles.contains_key(name) {
      return Err(ProfileError::UnknownConfigProfile(name.to_string()));
    }
    validate_profile_input(name, &input, credentials.as_ref())?;
    let mut credential_document = read_document_for_write::<CredentialDocument>(&self.credential_path, "credential profile store")?;
    let original_credentials = serde_json::to_vec(&credential_document).expect("credential document is serializable");
    match credentials {
      Some(credentials) => {
        credential_document.profiles.insert(input.credential_profile.clone(), credentials.into());
      }
      None if !credential_document.profiles.contains_key(&input.credential_profile) => {
        return Err(ProfileError::UnknownCredentialProfile(input.credential_profile));
      }
      None => {}
    }
    configs.profiles.insert(name.to_string(), input.into());
    validate_unique_device_ids(&configs.profiles)?;
    write_document(&self.credential_path, &credential_document, "credential profile store")?;
    if let Err(error) = write_document(&self.config_path, &configs, "config profile store") {
      let rollback: CredentialDocument = serde_json::from_slice(&original_credentials).expect("saved credential document decodes");
      write_document(&self.credential_path, &rollback, "credential profile rollback").map_err(|rollback| ProfileError::Rollback {
        operation: error.to_string(),
        rollback: rollback.to_string(),
      })?;
      return Err(error);
    }
    Ok(())
  }

  /// Deletes only the Device profile. Credential bindings are retained because
  /// another future profile may still refer to them and private material is
  /// never deleted implicitly.
  pub fn delete(&self, name: &str) -> Result<(), ProfileError> {
    let _lock = MutationLock::acquire(&self.config_path)?;
    let mut configs = read_document_for_write::<ConfigDocument>(&self.config_path, "config profile store")?;
    if configs.profiles.remove(name).is_none() {
      return Err(ProfileError::UnknownConfigProfile(name.to_string()));
    }
    write_document(&self.config_path, &configs, "config profile store")
  }

  /// Resolves exactly one paired remote Device and reads its bounded TLS
  /// material. Selection can name a config profile directly or use the
  /// canonical Device ID/name and optional credential profile stored in
  /// [`AuvContext`].
  pub fn resolve(&self, context: &AuvContext) -> Result<ResolvedRemoteProfile, ProfileError> {
    let configs: ConfigDocument = read_json(&self.config_path, MAX_PROFILE_STORE_BYTES, "config profile store")?;
    validate_unique_device_ids(&configs.profiles)?;
    let (config_name, config) = select_config(&configs.profiles, context)?;
    validate_context_matches(config_name, config, context)?;
    let credential_name = config.credential_profile.as_str();
    let credentials: CredentialDocument = read_json(&self.credential_path, MAX_PROFILE_STORE_BYTES, "credential profile store")?;
    let credential =
      credentials.profiles.get(credential_name).ok_or_else(|| ProfileError::UnknownCredentialProfile(credential_name.to_string()))?;
    let endpoint = validate_remote_endpoint(&config.endpoint)?;
    validate_required("device_id", &config.device_id)?;
    validate_required("device_name", &config.device_name)?;
    validate_server_name(&config.server_name)?;
    for path in [
      &credential.server_ca_certificate,
      &credential.client_certificate,
      &credential.client_private_key,
    ] {
      if !path.is_absolute() {
        return Err(ProfileError::RelativeCredentialPath(path.clone()));
      }
    }
    Ok(ResolvedRemoteProfile {
      config_profile: config_name.to_string(),
      credential_profile: credential_name.to_string(),
      device_id: config.device_id.clone(),
      device_name: config.device_name.clone(),
      endpoint,
      server_name: config.server_name.clone(),
      server_ca_certificate_pem: read_secure(&credential.server_ca_certificate, MAX_CREDENTIAL_MATERIAL_BYTES, "server CA certificate")?,
      client_certificate_pem: read_secure(&credential.client_certificate, MAX_CREDENTIAL_MATERIAL_BYTES, "client certificate")?,
      client_private_key_pem: read_secure(&credential.client_private_key, MAX_CREDENTIAL_MATERIAL_BYTES, "client private key")?,
    })
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceProfileInput {
  pub device_id: String,
  pub device_name: String,
  pub endpoint: String,
  pub server_name: String,
  pub credential_profile: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialProfileInput {
  pub server_ca_certificate: PathBuf,
  pub client_certificate: PathBuf,
  pub client_private_key: PathBuf,
}

/// Non-secret summary of one paired Device profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredDevice {
  config_profile: String,
  credential_profile: String,
  device_id: String,
  device_name: String,
  endpoint: http::Uri,
  server_name: String,
}

impl ConfiguredDevice {
  pub fn config_profile(&self) -> &str {
    &self.config_profile
  }

  pub fn credential_profile(&self) -> &str {
    &self.credential_profile
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

  pub fn server_name(&self) -> &str {
    &self.server_name
  }
}

#[derive(Clone, Debug)]
pub struct ResolvedRemoteProfile {
  config_profile: String,
  credential_profile: String,
  device_id: String,
  device_name: String,
  endpoint: http::Uri,
  server_name: String,
  server_ca_certificate_pem: Vec<u8>,
  client_certificate_pem: Vec<u8>,
  client_private_key_pem: Vec<u8>,
}

impl ResolvedRemoteProfile {
  pub fn config_profile(&self) -> &str {
    &self.config_profile
  }

  pub fn credential_profile(&self) -> &str {
    &self.credential_profile
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

  pub fn server_name(&self) -> &str {
    &self.server_name
  }

  pub fn server_ca_certificate_pem(&self) -> &[u8] {
    &self.server_ca_certificate_pem
  }

  pub fn client_certificate_pem(&self) -> &[u8] {
    &self.client_certificate_pem
  }

  pub fn client_private_key_pem(&self) -> &[u8] {
    &self.client_private_key_pem
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
  #[error("{kind} {path} is not a regular file")]
  NotRegular { kind: &'static str, path: PathBuf },
  #[error("{kind} {path} is not owned by the current user")]
  WrongOwner { kind: &'static str, path: PathBuf },
  #[error("{kind} {path} is writable by group or other users")]
  InsecurePermissions { kind: &'static str, path: PathBuf },
  #[error("{kind} {path} exceeds the {limit}-byte size limit")]
  TooLarge {
    kind: &'static str,
    path: PathBuf,
    limit: u64,
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
  #[error("failed to encode {kind}: {source}")]
  Encode {
    kind: &'static str,
    #[source]
    source: serde_json::Error,
  },
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
  #[error("unknown credential profile {0:?}")]
  UnknownCredentialProfile(String),
  #[error("configuration profile {0:?} already exists")]
  ConfigProfileAlreadyExists(String),
  #[error("credential profile {0:?} already exists")]
  CredentialProfileAlreadyExists(String),
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
  #[error("paired endpoint must be an https URI with an authority and no path or query: {0:?}")]
  InvalidEndpoint(String),
  #[error("profile field {0} must not be empty")]
  EmptyField(&'static str),
  #[error("profile server_name must be a DNS name or IP address without a port or path")]
  InvalidServerName,
  #[error("credential material path must be absolute: {0}")]
  RelativeCredentialPath(PathBuf),
  #[error("paired Device profiles are unsupported on this platform until owner/ACL validation is implemented")]
  UnsupportedPlatform,
  #[error("profile update failed ({operation}) and credential rollback also failed ({rollback})")]
  Rollback { operation: String, rollback: String },
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigDocument {
  profiles: BTreeMap<String, DeviceProfile>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct DeviceProfile {
  device_id: String,
  device_name: String,
  endpoint: String,
  server_name: String,
  credential_profile: String,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialDocument {
  profiles: BTreeMap<String, CredentialProfile>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialProfile {
  server_ca_certificate: PathBuf,
  client_certificate: PathBuf,
  client_private_key: PathBuf,
}

impl From<DeviceProfileInput> for DeviceProfile {
  fn from(value: DeviceProfileInput) -> Self {
    Self {
      device_id: value.device_id,
      device_name: value.device_name,
      endpoint: value.endpoint,
      server_name: value.server_name,
      credential_profile: value.credential_profile,
    }
  }
}

impl From<CredentialProfileInput> for CredentialProfile {
  fn from(value: CredentialProfileInput) -> Self {
    Self {
      server_ca_certificate: value.server_ca_certificate,
      client_certificate: value.client_certificate,
      client_private_key: value.client_private_key,
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
    .filter(|(_, profile)| context.device_id.as_ref().is_none_or(|id| profile.device_id == *id))
    .filter(|(_, profile)| context.device_name.as_ref().is_none_or(|name| profile.device_name == *name))
    .filter(|(_, profile)| context.credential_profile.as_ref().is_none_or(|name| profile.credential_profile == *name))
    .collect::<Vec<_>>();
  match matches.as_slice() {
    [(name, profile)] => Ok((name.as_str(), *profile)),
    [] => Err(ProfileError::DeviceNotConfigured),
    matches => {
      let candidate_ids = matches.iter().map(|(_, profile)| profile.device_id.as_str()).collect::<Vec<_>>().join(", ");
      Err(ProfileError::AmbiguousDevice(candidate_ids))
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
    ("device_name", context.device_name.as_deref(), profile.device_name.as_str()),
    ("config_profile", context.config_profile.as_deref(), profile_name),
    ("credential_profile", context.credential_profile.as_deref(), profile.credential_profile.as_str()),
  ] {
    if let Some(actual) = actual
      && actual != expected
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
  if uri.scheme_str() != Some("https") || uri.authority().is_none() || !matches!(uri.path(), "" | "/") || uri.query().is_some() {
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

fn validate_server_name(value: &str) -> Result<(), ProfileError> {
  if value.trim().is_empty()
    || (value.parse::<std::net::IpAddr>().is_err() && value.contains(['/', ':']))
    || value.chars().any(char::is_whitespace)
  {
    Err(ProfileError::InvalidServerName)
  } else {
    Ok(())
  }
}

fn validate_profile_input(name: &str, input: &DeviceProfileInput, credentials: Option<&CredentialProfileInput>) -> Result<(), ProfileError> {
  validate_required("config_profile", name)?;
  validate_required("device_id", &input.device_id)?;
  validate_required("device_name", &input.device_name)?;
  validate_required("credential_profile", &input.credential_profile)?;
  validate_remote_endpoint(&input.endpoint)?;
  validate_server_name(&input.server_name)?;
  if let Some(credentials) = credentials {
    for path in [
      &credentials.server_ca_certificate,
      &credentials.client_certificate,
      &credentials.client_private_key,
    ] {
      if !path.is_absolute() {
        return Err(ProfileError::RelativeCredentialPath(path.clone()));
      }
    }
  }
  Ok(())
}

struct MutationLock {
  _file: File,
}

impl MutationLock {
  fn acquire(config_path: &Path) -> Result<Self, ProfileError> {
    #[cfg(not(unix))]
    return Err(ProfileError::UnsupportedPlatform);
    #[cfg(unix)]
    {
      use fs2::FileExt as _;
      use std::os::unix::fs::PermissionsExt as _;

      let parent = config_path.parent().ok_or_else(|| ProfileError::NoParent(config_path.to_path_buf()))?;
      std::fs::create_dir_all(parent).map_err(|source| ProfileError::Write {
        kind: "profile mutation lock",
        path: parent.to_path_buf(),
        source,
      })?;
      std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(|source| ProfileError::Write {
        kind: "profile mutation lock",
        path: parent.to_path_buf(),
        source,
      })?;
      let lock_path = config_path.with_extension("lock");
      let file = rustix::fs::open(
        &lock_path,
        rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CREATE | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
      )
      .map(File::from)
      .map_err(|source| ProfileError::Write {
        kind: "profile mutation lock",
        path: lock_path.clone(),
        source: std::io::Error::from(source),
      })?;
      validate_unix_owner_and_mode(
        &lock_path,
        "profile mutation lock",
        &file.metadata().map_err(|source| ProfileError::Read {
          kind: "profile mutation lock",
          path: lock_path.clone(),
          source,
        })?,
      )?;
      file.lock_exclusive().map_err(|source| ProfileError::Write {
        kind: "profile mutation lock",
        path: lock_path,
        source,
      })?;
      Ok(Self { _file: file })
    }
  }
}

fn read_document_for_write<T>(path: &Path, kind: &'static str) -> Result<T, ProfileError>
where
  T: serde::de::DeserializeOwned + Default,
{
  match read_json(path, MAX_PROFILE_STORE_BYTES, kind) {
    Ok(document) => Ok(document),
    Err(ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
    Err(error) => Err(error),
  }
}

fn write_document<T: serde::Serialize>(path: &Path, document: &T, kind: &'static str) -> Result<(), ProfileError> {
  let bytes = serde_json::to_vec_pretty(document).map_err(|source| ProfileError::Encode { kind, source })?;
  if bytes.len() as u64 > MAX_PROFILE_STORE_BYTES {
    return Err(ProfileError::TooLarge {
      kind,
      path: path.to_path_buf(),
      limit: MAX_PROFILE_STORE_BYTES,
    });
  }
  let parent = path.parent().ok_or_else(|| ProfileError::NoParent(path.to_path_buf()))?;
  std::fs::create_dir_all(parent).map_err(|source| ProfileError::Write {
    kind,
    path: parent.to_path_buf(),
    source,
  })?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(|source| ProfileError::Write {
      kind,
      path: parent.to_path_buf(),
      source,
    })?;
  }
  #[cfg(not(unix))]
  return Err(ProfileError::UnsupportedPlatform);

  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt as _;
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().and_then(|value| value.to_str()).ok_or_else(|| ProfileError::NoParent(path.to_path_buf()))?;
    let temporary = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), sequence));
    let result = (|| {
      let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary).map_err(|source| ProfileError::Write {
          kind,
          path: temporary.clone(),
          source,
        })?;
      file.write_all(&bytes).and_then(|_| file.write_all(b"\n")).and_then(|_| file.sync_all()).map_err(|source| ProfileError::Write {
        kind,
        path: temporary.clone(),
        source,
      })?;
      std::fs::rename(&temporary, path).map_err(|source| ProfileError::Write {
        kind,
        path: path.to_path_buf(),
        source,
      })?;
      File::open(parent).and_then(|directory| directory.sync_all()).map_err(|source| ProfileError::Write {
        kind,
        path: parent.to_path_buf(),
        source,
      })
    })();
    if result.is_err() {
      let _ = std::fs::remove_file(&temporary);
    }
    result
  }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64, kind: &'static str) -> Result<T, ProfileError> {
  let bytes = read_secure(path, limit, kind)?;
  serde_json::from_slice(&bytes).map_err(|source| ProfileError::Decode {
    kind,
    path: path.to_path_buf(),
    source,
  })
}

fn read_secure(path: &Path, limit: u64, kind: &'static str) -> Result<Vec<u8>, ProfileError> {
  let mut file = open_no_follow(path, kind)?;
  let metadata = file.metadata().map_err(|source| ProfileError::Read {
    kind,
    path: path.to_path_buf(),
    source,
  })?;
  if !metadata.is_file() {
    return Err(ProfileError::NotRegular {
      kind,
      path: path.to_path_buf(),
    });
  }
  #[cfg(unix)]
  validate_unix_owner_and_mode(path, kind, &metadata)?;
  if metadata.len() > limit {
    return Err(ProfileError::TooLarge {
      kind,
      path: path.to_path_buf(),
      limit,
    });
  }
  let mut bytes = Vec::with_capacity(metadata.len() as usize);
  std::io::Read::by_ref(&mut file).take(limit + 1).read_to_end(&mut bytes).map_err(|source| ProfileError::Read {
    kind,
    path: path.to_path_buf(),
    source,
  })?;
  if bytes.len() as u64 > limit {
    return Err(ProfileError::TooLarge {
      kind,
      path: path.to_path_buf(),
      limit,
    });
  }
  Ok(bytes)
}

#[cfg(unix)]
fn open_no_follow(path: &Path, kind: &'static str) -> Result<File, ProfileError> {
  use rustix::fs::{Mode, OFlags};

  rustix::fs::open(path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW, Mode::empty()).map(File::from).map_err(|source| {
    ProfileError::Open {
      kind,
      path: path.to_path_buf(),
      source: std::io::Error::from(source),
    }
  })
}

#[cfg(not(unix))]
fn open_no_follow(_path: &Path, _kind: &'static str) -> Result<File, ProfileError> {
  // TODO(windows-profile-acl): validate the current user's ownership and ACL
  // before enabling paired profiles on Windows. Fail closed until then; Unix
  // uses an atomic O_NOFOLLOW open plus uid/mode checks in this slice.
  Err(ProfileError::UnsupportedPlatform)
}

#[cfg(unix)]
fn validate_unix_owner_and_mode(path: &Path, kind: &'static str, metadata: &std::fs::Metadata) -> Result<(), ProfileError> {
  use std::os::unix::fs::MetadataExt as _;

  if metadata.uid() != rustix::process::geteuid().as_raw() {
    return Err(ProfileError::WrongOwner {
      kind,
      path: path.to_path_buf(),
    });
  }
  if metadata.mode() & 0o022 != 0 {
    return Err(ProfileError::InsecurePermissions {
      kind,
      path: path.to_path_buf(),
    });
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::fs::OpenOptions;
  use std::io::Write as _;

  #[cfg(unix)]
  use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _, symlink};

  use super::{CredentialProfileInput, DeviceProfileInput, ProfileError, ProfileStore};
  use crate::AuvContext;

  fn write_private(path: &std::path::Path, contents: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).expect("create owner-only fixture");
    file.write_all(contents).expect("write fixture");
  }

  #[test]
  fn named_device_and_credential_profiles_resolve_without_embedding_secrets_in_context() {
    let directory = tempfile::tempdir().unwrap();
    let ca = directory.path().join("ca.pem");
    let certificate = directory.path().join("client.pem");
    let key = directory.path().join("client-key.pem");
    write_private(&ca, b"ca");
    write_private(&certificate, b"certificate");
    write_private(&key, b"private-key");
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(
      &config,
      format!(
        r#"{{"profiles":{{"studio":{{"device_id":"device_studio","device_name":"Studio Mac","endpoint":"https://studio.example:9847","server_name":"studio.example","credential_profile":"paired-studio"}}}}}}"#
      )
      .as_bytes(),
    );
    write_private(
      &credentials,
      serde_json::to_string(&serde_json::json!({
        "profiles": {
          "paired-studio": {
            "server_ca_certificate": ca,
            "client_certificate": certificate,
            "client_private_key": key,
          }
        }
      }))
      .unwrap()
      .as_bytes(),
    );

    let store = ProfileStore::from_paths(config.clone(), credentials.clone());
    let listed = store.list_devices().expect("list non-secret configured Devices");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].config_profile(), "studio");
    assert_eq!(listed[0].credential_profile(), "paired-studio");
    assert_eq!(listed[0].device_id(), "device_studio");
    assert_eq!(listed[0].device_name(), "Studio Mac");
    assert_eq!(listed[0].endpoint().to_string(), "https://studio.example:9847/");
    assert_eq!(listed[0].server_name(), "studio.example");

    let context = AuvContext {
      config_profile: Some("studio".to_string()),
      ..Default::default()
    };
    let resolved = store.resolve(&context).expect("resolve paired profile");
    assert_eq!(resolved.config_profile(), "studio");
    assert_eq!(resolved.credential_profile(), "paired-studio");
    assert_eq!(resolved.device_id(), "device_studio");
    assert_eq!(resolved.device_name(), "Studio Mac");
    assert_eq!(resolved.endpoint().to_string(), "https://studio.example:9847/");
    assert_eq!(resolved.server_name(), "studio.example");
    assert_eq!(resolved.server_ca_certificate_pem(), b"ca");
    assert_eq!(resolved.client_certificate_pem(), b"certificate");
    assert_eq!(resolved.client_private_key_pem(), b"private-key");
    assert!(!serde_json::to_string(&context).unwrap().contains("private-key"));

    let error = ProfileStore::from_paths(directory.path().join("profiles.json"), directory.path().join("credentials.json"))
      .resolve(&AuvContext {
        config_profile: Some("studio".to_string()),
        credential_profile: Some("different-credential".to_string()),
        ..Default::default()
      })
      .expect_err("context must not override a Device profile's credential binding");
    assert!(matches!(
      error,
      ProfileError::ContextConflict {
        field: "credential_profile",
        ..
      }
    ));
  }

  #[cfg(unix)]
  #[test]
  fn writable_profile_store_and_symlinked_credentials_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(&config, br#"{"profiles":{}}"#);
    write_private(&credentials, br#"{"profiles":{}}"#);
    fs::set_permissions(&config, fs::Permissions::from_mode(0o620)).unwrap();
    let error = ProfileStore::from_paths(&config, &credentials)
      .resolve(&AuvContext {
        config_profile: Some("missing".to_string()),
        ..Default::default()
      })
      .unwrap_err();
    assert!(matches!(error, ProfileError::InsecurePermissions { .. }));

    fs::remove_file(&config).unwrap();
    fs::remove_file(&credentials).unwrap();
    let target = directory.path().join("target.pem");
    let linked = directory.path().join("linked.pem");
    write_private(&target, b"secret");
    symlink(&target, &linked).unwrap();
    write_private(
      &config,
      br#"{"profiles":{"studio":{"device_id":"device_studio","device_name":"Studio","endpoint":"https://studio.example:9847","server_name":"studio.example","credential_profile":"paired"}}}"#,
    );
    write_private(
      &credentials,
      serde_json::to_string(&serde_json::json!({
        "profiles": {"paired": {
          "server_ca_certificate": linked,
          "client_certificate": target,
          "client_private_key": directory.path().join("target.pem")
        }}
      }))
      .unwrap()
      .as_bytes(),
    );
    let error = ProfileStore::from_paths(config, credentials)
      .resolve(&AuvContext {
        config_profile: Some("studio".to_string()),
        ..Default::default()
      })
      .unwrap_err();
    assert!(matches!(error, ProfileError::Open { .. }), "unexpected error: {error}");
  }

  #[test]
  fn credential_material_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let oversized = directory.path().join("oversized.pem");
    write_private(&oversized, &vec![b'x'; 1024 * 1024 + 1]);
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(
      &config,
      br#"{"profiles":{"studio":{"device_id":"device_studio","device_name":"Studio","endpoint":"https://studio.example:9847","server_name":"studio.example","credential_profile":"paired"}}}"#,
    );
    write_private(
      &credentials,
      serde_json::to_string(&serde_json::json!({
        "profiles": {"paired": {
          "server_ca_certificate": oversized,
          "client_certificate": directory.path().join("oversized.pem"),
          "client_private_key": directory.path().join("oversized.pem")
        }}
      }))
      .unwrap()
      .as_bytes(),
    );
    let error = ProfileStore::from_paths(config, credentials)
      .resolve(&AuvContext {
        config_profile: Some("studio".to_string()),
        ..Default::default()
      })
      .unwrap_err();
    assert!(matches!(
      error,
      ProfileError::TooLarge {
        limit: 1_048_576,
        ..
      }
    ));
  }

  #[test]
  fn duplicate_device_names_report_canonical_candidate_ids() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(
      &config,
      br#"{"profiles":{"a":{"device_id":"device_a","device_name":"Studio","endpoint":"https://a.example:9847","server_name":"a.example","credential_profile":"a"},"b":{"device_id":"device_b","device_name":"Studio","endpoint":"https://b.example:9847","server_name":"b.example","credential_profile":"b"}}}"#,
    );
    write_private(&credentials, br#"{"profiles":{}}"#);
    let error = ProfileStore::from_paths(config, credentials)
      .resolve(&AuvContext {
        device_name: Some("Studio".to_string()),
        ..Default::default()
      })
      .unwrap_err();
    assert!(matches!(error, ProfileError::AmbiguousDevice(ids) if ids == "device_a, device_b"));
  }

  #[test]
  fn duplicate_canonical_device_ids_are_rejected_even_when_a_profile_name_is_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(
      &config,
      br#"{"profiles":{"a":{"device_id":"device_same","device_name":"A","endpoint":"https://a.example:9847","server_name":"a.example","credential_profile":"a"},"b":{"device_id":"device_same","device_name":"B","endpoint":"https://b.example:9847","server_name":"b.example","credential_profile":"b"}}}"#,
    );
    write_private(&credentials, br#"{"profiles":{}}"#);
    let error = ProfileStore::from_paths(config, credentials)
      .resolve(&AuvContext {
        config_profile: Some("a".to_string()),
        ..Default::default()
      })
      .expect_err("canonical Device IDs must be unique across named profiles");
    assert!(matches!(error, ProfileError::DuplicateDeviceId { device_id, profiles } if device_id == "device_same" && profiles == "a, b"));
  }

  #[cfg(unix)]
  #[test]
  fn profile_mutations_are_atomic_owner_only_and_do_not_embed_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config/profiles.json");
    let credentials = directory.path().join("config/credentials.json");
    let ca = directory.path().join("ca.pem");
    let certificate = directory.path().join("client.pem");
    let key = directory.path().join("key.pem");
    for path in [&ca, &certificate, &key] {
      write_private(path, b"secret material");
    }
    let store = ProfileStore::from_paths(&config, &credentials);
    store
      .create(
        "studio",
        DeviceProfileInput {
          device_id: "device_studio".into(),
          device_name: "Studio".into(),
          endpoint: "https://studio.example:9847".into(),
          server_name: "studio.example".into(),
          credential_profile: "paired-studio".into(),
        },
        Some(CredentialProfileInput {
          server_ca_certificate: ca,
          client_certificate: certificate,
          client_private_key: key,
        }),
      )
      .expect("create profile and credential binding");
    assert_eq!(store.get_device("studio").unwrap().device_id(), "device_studio");
    assert_eq!(fs::metadata(&config).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&credentials).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(!fs::read_to_string(&config).unwrap().contains("secret material"));
    assert!(!fs::read_to_string(&credentials).unwrap().contains("secret material"));

    store
      .update(
        "studio",
        DeviceProfileInput {
          device_id: "device_studio".into(),
          device_name: "Studio Updated".into(),
          endpoint: "https://studio.example:9848".into(),
          server_name: "studio.example".into(),
          credential_profile: "paired-studio".into(),
        },
        None,
      )
      .unwrap();
    assert_eq!(store.get_device("studio").unwrap().device_name(), "Studio Updated");
    store.delete("studio").unwrap();
    assert!(matches!(store.get_device("studio"), Err(ProfileError::UnknownConfigProfile(_))));
  }

  #[cfg(unix)]
  #[test]
  fn mutation_refuses_to_replace_a_damaged_existing_store() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(&config, b"not-json");
    write_private(&credentials, br#"{"profiles":{}}"#);
    let original = fs::read(&config).unwrap();
    let error = ProfileStore::from_paths(&config, &credentials)
      .create(
        "studio",
        DeviceProfileInput {
          device_id: "device_studio".into(),
          device_name: "Studio".into(),
          endpoint: "https://studio.example:9847".into(),
          server_name: "studio.example".into(),
          credential_profile: "paired".into(),
        },
        None,
      )
      .unwrap_err();
    assert!(matches!(error, ProfileError::Decode { .. }));
    assert_eq!(fs::read(&config).unwrap(), original);
  }

  #[cfg(unix)]
  #[test]
  fn device_profile_cannot_reference_a_missing_credential_binding() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("profiles.json");
    let credentials = directory.path().join("credentials.json");
    write_private(&credentials, br#"{"profiles":{}}"#);
    let error = ProfileStore::from_paths(&config, &credentials)
      .create(
        "studio",
        DeviceProfileInput {
          device_id: "device_studio".into(),
          device_name: "Studio".into(),
          endpoint: "https://studio.example:9847".into(),
          server_name: "studio.example".into(),
          credential_profile: "missing".into(),
        },
        None,
      )
      .unwrap_err();
    assert!(matches!(error, ProfileError::UnknownCredentialProfile(name) if name == "missing"));
    assert!(!config.exists());
  }
}
