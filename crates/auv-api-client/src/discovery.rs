//! Versioned local daemon discovery shared by all client frontends.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::{ConnectEndpoint, EndpointParseError};

const DESCRIPTOR_VERSION: u32 = 1;

/// Durable JSON record published after a local daemon has bound its listener.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Descriptor {
  version: u32,
  endpoint: String,
  process_id: u32,
  instance_id: String,
}

impl Descriptor {
  pub fn for_current_process(endpoint: String, instance_id: String) -> Self {
    Self {
      version: DESCRIPTOR_VERSION,
      endpoint,
      process_id: std::process::id(),
      instance_id,
    }
  }

  pub fn endpoint(&self) -> &str {
    &self.endpoint
  }

  pub fn instance_id(&self) -> &str {
    &self.instance_id
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
  #[error("unsupported daemon descriptor version {version} in {path}")]
  UnsupportedVersion { version: u32, path: PathBuf },
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
  if descriptor.version != DESCRIPTOR_VERSION {
    return Err(DiscoveryError::UnsupportedVersion {
      version: descriptor.version,
      path: path.to_path_buf(),
    });
  }
  Ok(Some(descriptor))
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
mod tests {
  use super::*;

  #[test]
  fn missing_descriptor_is_not_a_discovery_error() {
    let directory = tempfile::tempdir().unwrap();
    assert!(read_descriptor(&directory.path().join("missing.json")).unwrap().is_none());
  }

  #[test]
  fn descriptor_round_trips_and_validates_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("api-server.json");
    let descriptor = Descriptor::for_current_process("http://127.0.0.1:9847".to_string(), "instance".to_string());
    fs::write(&path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
    let decoded = read_descriptor(&path).unwrap().unwrap();
    assert_eq!(decoded.endpoint(), "http://127.0.0.1:9847");
    assert_eq!(decoded.instance_id(), "instance");

    fs::write(&path, br#"{"version":2,"endpoint":"http://127.0.0.1:9847","process_id":1,"instance_id":"new"}"#).unwrap();
    assert!(matches!(read_descriptor(&path), Err(DiscoveryError::UnsupportedVersion { version: 2, .. })));
  }

  #[test]
  fn explicit_endpoint_is_resolved_without_discovery() {
    let endpoint = resolve(Some("http://127.0.0.1:9847")).unwrap().expect("explicit endpoint");
    assert_eq!(endpoint.to_string(), "http://127.0.0.1:9847");
    assert!(resolve(Some("http://example.com:9847")).is_err());
  }
}
