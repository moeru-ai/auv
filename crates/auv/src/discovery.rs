//! Local daemon discovery shared by all client frontends.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use auv_api_client::{ConnectEndpoint, EndpointParseError};

/// Durable JSON record published after a local daemon has bound its listener.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Descriptor {
  endpoint: String,
  instance_id: String,
  /// Absolute store root of the publishing daemon. Target-local commands use
  /// it to find that daemon's local service without a `--store-root` flag.
  /// NOTICE(discovery-store-root): descriptors written before 2026-10-11 omit
  /// it; readers then fall back to their default store.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  store_root: Option<PathBuf>,
}

impl Descriptor {
  pub fn for_current_process(endpoint: String, instance_id: String, store_root: PathBuf) -> Self {
    Self {
      endpoint,
      instance_id,
      store_root: Some(store_root),
    }
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub fn instance_id(&self) -> &str {
    &self.instance_id
  }

  pub fn store_root(&self) -> Option<&Path> {
    self.store_root.as_deref()
  }
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
  #[error("could not resolve the current user's AUV state directory")]
  StateDirectoryUnavailable,
  #[error("failed to read daemon descriptor {path}: {source}")]
  Read {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("invalid daemon descriptor {path}: {source}")]
  Decode {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("AUV_ENDPOINT is not valid Unicode: {0}")]
  InvalidEnvironment(std::env::VarError),
  #[error("invalid AUV API endpoint {endpoint:?}: {source}")]
  InvalidEndpoint {
    endpoint: String,
    #[source]
    source: EndpointParseError,
  },
}

pub fn default_path() -> Result<PathBuf, DiscoveryError> {
  if let Some(path) = std::env::var_os("AUV_DISCOVERY_FILE") {
    return Ok(PathBuf::from(path));
  }
  let directories = directories::ProjectDirs::from("ai", "moeru", "auv").ok_or(DiscoveryError::StateDirectoryUnavailable)?;
  Ok(directories.state_dir().unwrap_or_else(|| directories.data_local_dir()).join("api-server.json"))
}

/// Reads a descriptor without treating a missing daemon as an error.
pub fn read_descriptor(path: &Path) -> Result<Option<Descriptor>, DiscoveryError> {
  let bytes = match fs::read(path) {
    Ok(bytes) => bytes,
    Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
    Err(source) => {
      return Err(DiscoveryError::Read {
        path: path.to_path_buf(),
        source,
      });
    }
  };
  let descriptor = serde_json::from_slice::<Descriptor>(&bytes).map_err(|source| DiscoveryError::Decode {
    path: path.to_path_buf(),
    source,
  })?;
  Ok(Some(descriptor))
}

/// Store root published by the current user's discovered daemon, if one is
/// registered and its descriptor records it.
pub fn discovered_store_root() -> Result<Option<PathBuf>, DiscoveryError> {
  Ok(read_descriptor(&default_path()?)?.and_then(|descriptor| descriptor.store_root))
}

/// Selects an API endpoint using explicit argument, `AUV_ENDPOINT`, then the
/// current user's discovery descriptor. Missing discovery returns `None`.
pub fn resolve(explicit: Option<&str>) -> Result<Option<ConnectEndpoint>, DiscoveryError> {
  let selected = match explicit {
    Some(endpoint) => Some(endpoint.to_string()),
    None => match std::env::var("AUV_ENDPOINT") {
      Ok(endpoint) => Some(endpoint),
      Err(std::env::VarError::NotPresent) => read_descriptor(&default_path()?)?.map(|descriptor| descriptor.endpoint),
      Err(error) => return Err(DiscoveryError::InvalidEnvironment(error)),
    },
  };
  selected.map(|endpoint| endpoint.parse().map_err(|source| DiscoveryError::InvalidEndpoint { endpoint, source })).transpose()
}

#[cfg(test)]
#[path = "discovery_test.rs"]
mod tests;
