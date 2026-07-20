use std::fmt;
use std::str::FromStr;

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

use crate::{ArtifactId, RunId, ValidationError};

/// The canonical transport-independent identity of one run artifact.
// TODO(inspect-artifact-resolution-v1): Enforce the 256-URI request bound on
// the resolver DTO when that later Inspect slice introduces the batch surface.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactUri(Url);

impl ArtifactUri {
  /// Constructs the sole V1 URI form from validated identifiers.
  pub fn from_ids(run_id: RunId, artifact_id: ArtifactId) -> Self {
    format!("auv://runs/{run_id}/artifacts/{artifact_id}").parse().expect("validated IDs always produce a canonical artifact URI")
  }

  /// Returns the owning run identifier.
  pub fn run_id(&self) -> RunId {
    self.path_ids().0
  }

  /// Returns the artifact identifier.
  pub fn artifact_id(&self) -> ArtifactId {
    self.path_ids().1
  }

  fn path_ids(&self) -> (RunId, ArtifactId) {
    let segments = self.0.path_segments().expect("canonical artifact URI has path segments").collect::<Vec<_>>();
    (
      segments[0].parse().expect("canonical artifact URI has a run ID"),
      segments[2].parse().expect("canonical artifact URI has an artifact ID"),
    )
  }
}

impl fmt::Display for ArtifactUri {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.0.as_str())
  }
}

impl FromStr for ArtifactUri {
  type Err = ValidationError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let parsed = Url::parse(value).map_err(|_| ValidationError::new("artifact URI is not a valid URL"))?;
    if parsed.scheme() != "auv"
      || parsed.host_str() != Some("runs")
      || !parsed.username().is_empty()
      || parsed.password().is_some()
      || parsed.port().is_some()
      || parsed.query().is_some()
      || parsed.fragment().is_some()
    {
      return Err(ValidationError::new("artifact URI must use the canonical AUV authority"));
    }

    let segments = parsed.path_segments().ok_or_else(|| ValidationError::new("artifact URI path is invalid"))?.collect::<Vec<_>>();
    if segments.len() != 3 || segments[1] != "artifacts" {
      return Err(ValidationError::new("artifact URI must identify exactly one run artifact"));
    }
    let run_id = segments[0].parse::<RunId>().map_err(|_| ValidationError::new("artifact URI run ID is invalid"))?;
    let artifact_id = segments[2].parse::<ArtifactId>().map_err(|_| ValidationError::new("artifact URI artifact ID is invalid"))?;
    let canonical = format!("auv://runs/{run_id}/artifacts/{artifact_id}");
    if value != canonical || parsed.as_str() != canonical {
      return Err(ValidationError::new("artifact URI is not canonical"));
    }
    Ok(Self(parsed))
  }
}

impl Serialize for ArtifactUri {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for ArtifactUri {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
  }
}
