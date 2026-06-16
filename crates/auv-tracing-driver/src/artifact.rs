use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
  // TODO(auv-tracing-driver-task2): replace string IDs with trace ID newtypes
  // after `trace` moves into this crate; Task 1 must build without placeholder
  // future modules.
  pub run_id: String,
  pub artifact_id: String,
  pub span_id: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub captured_event_id: Option<String>,
}

#[derive(Clone, Debug)]
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
