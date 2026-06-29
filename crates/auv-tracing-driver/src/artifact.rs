use std::path::PathBuf;

use crate::trace::{ArtifactId, EventId, RunId, SpanId};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
  pub run_id: RunId,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub captured_event_id: Option<EventId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducedArtifact {
  pub kind: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ArtifactFileSource {
  pub role: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub summary: Option<String>,
}

/// In-memory artifact payload for [`crate::store::LocalStore::stage_artifact_bytes`].
#[derive(Clone, Debug)]
pub struct ArtifactBytesSource {
  pub role: String,
  pub bytes: Vec<u8>,
  pub preferred_name: String,
  pub summary: Option<String>,
}
