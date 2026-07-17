use std::path::PathBuf;

use auv_tracing_driver::{ProducedArtifact, now_millis};

pub(crate) fn invoke_artifact_path(command_id: &str, label: &str, extension: &str) -> PathBuf {
  std::env::temp_dir().join(format!(
    "auv-invoke-{}-{label}-{}-{}.{}",
    command_id.replace('.', "-"),
    std::process::id(),
    now_millis(),
    extension
  ))
}

pub(crate) fn json_artifact<T: serde::Serialize>(
  kind: &str,
  label: &str,
  value: &T,
  note: impl Into<String>,
) -> Result<ProducedArtifact, String> {
  let source_path = invoke_artifact_path(kind, label, "json");
  let body = serde_json::to_vec_pretty(value).map_err(|error| format!("failed to serialize {kind} artifact: {error}"))?;
  std::fs::write(&source_path, body).map_err(|error| format!("failed to write {kind} artifact: {error}"))?;
  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{label}.json"),
    note: Some(note.into()),
  })
}
