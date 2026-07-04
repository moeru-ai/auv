use std::fs;
use std::path::{Path, PathBuf};

use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, arg::SCAN_FRAME_ARGS,
  invoke_command,
};
use auv_scan::{artifact::frame_artifact_file_name, produce_frame_from_fixture_dir};
use auv_tracing_driver::{ProducedArtifact, now_millis};
use tempfile::TempDir;

pub fn group() -> CommandGroup {
  CommandGroup::new("scan", "SCAN").command(frame_invoke_command())
}

#[invoke_command(
  id = "scan.frame",
  group = "scan",
  summary = "Produce a single scan-frame-v0 artifact bundle from a hermetic fixture directory and stage it into the run.",
  args = SCAN_FRAME_ARGS,
)]
fn frame(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  frame_impl(input)
}

fn frame_impl(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("scan.frame dry-run");
    output.verification = Some("dry-run; no artifacts produced".to_string());
    output
      .known_limits
      .push("scan.frame dry-run does not write scan artifacts.".to_string());
    return Ok(output);
  }

  let fixture_dir = required_input(&input, "fixture-dir")?;
  let fixture_path = Path::new(fixture_dir);
  if !fixture_path.is_dir() {
    return Err(format!(
      "scan.frame fixture directory does not exist: {fixture_dir}"
    ));
  }

  // NOTICE(s7-temp-artifact-lifetime): producer temp dir may drop after copy; staging sources persist.
  let producer_out = TempDir::new()
    .map_err(|error| format!("scan.frame failed to create producer output directory: {error}"))?;
  let produced = produce_frame_from_fixture_dir(fixture_path, producer_out.path())
    .map_err(|error| format!("scan.frame producer failed: {error}"))?;

  let json_source = invoke_artifact_path(input.command_id, "scan-frame-v0", "json");
  let image_source = invoke_artifact_path(input.command_id, "scan-frame-image", "png");
  fs::copy(&produced.json_path, &json_source)
    .map_err(|error| format!("scan.frame failed to stage JSON artifact source: {error}"))?;
  fs::copy(&produced.image_path, &image_source)
    .map_err(|error| format!("scan.frame failed to stage PNG artifact source: {error}"))?;

  let json_preferred_name = produced
    .json_path
    .file_name()
    .and_then(|name| name.to_str())
    .map(str::to_string)
    .unwrap_or_else(|| frame_artifact_file_name(produced.frame.sequence_index));
  let image_preferred_name = produced.frame.image.file_name.clone();

  let mut output =
    InvokeCommandOutput::new(format!("scan frame produced from fixture {}", fixture_dir));
  output.backend = Some("auv-scan.produce_frame_from_fixture_dir".to_string());
  output.verification = Some("capture-only; no semantic success claim".to_string());
  output.known_limits.push(
    "scan.frame records a single scan-frame-v0 bundle only; multi-frame invoke is deferred."
      .to_string(),
  );
  output.artifacts.push(ProducedArtifact {
    kind: "scan-frame-v0".to_string(),
    source_path: json_source,
    preferred_name: json_preferred_name,
    note: Some("Scan frame JSON produced by auv-scan fixture producer.".to_string()),
  });
  output.artifacts.push(ProducedArtifact {
    kind: "scan-frame-image".to_string(),
    source_path: image_source,
    preferred_name: image_preferred_name,
    note: Some("Scan frame PNG sibling produced by auv-scan fixture producer.".to_string()),
  });
  Ok(output)
}

fn invoke_artifact_path(command_id: &str, label: &str, extension: &str) -> PathBuf {
  std::env::temp_dir().join(format!(
    "auv-invoke-{}-{label}-{}-{}.{}",
    command_id.replace('.', "-"),
    std::process::id(),
    now_millis(),
    extension
  ))
}

fn required_input<'a>(input: &'a InvokeCommandInput<'_>, name: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(name)
    .map(String::as_str)
    .ok_or_else(|| format!("scan.frame missing required flag --{name}"))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::sync::Arc;

  use auv_scan::read_frame_artifact;
  use auv_tracing_driver::{LocalStore, MemoryRunRecorder, RunRecordingBackend, TraceStatusCode};

  use crate::{
    ExecutionTarget, InvokeNamespace, InvokeRequest, RunStatus, default_registry,
    recorded::invoke_recorded,
  };

  use super::{frame_impl, frame_invoke_command};

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn temp_store_root(label: &str) -> PathBuf {
    env::temp_dir().join(format!(
      "auv-cli-invoke-scan-{label}-{}-{}",
      std::process::id(),
      auv_tracing_driver::now_millis()
    ))
  }

  fn recording(label: &str) -> (RunRecordingBackend, PathBuf) {
    let store_root = temp_store_root(label);
    let _ = fs::remove_dir_all(&store_root);
    let backend = RunRecordingBackend::new(
      LocalStore::new(store_root.clone()).expect("store should create"),
      Arc::new(MemoryRunRecorder::new()),
    );
    (backend, store_root)
  }

  #[test]
  fn scan_frame_command_uses_scan_namespace() {
    let command = frame_invoke_command();
    assert_eq!(command.id, "scan.frame");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_frame_is_registered_in_default_registry() {
    let registry = default_registry();
    let command = registry
      .resolve("scan.frame")
      .expect("scan.frame should be registered");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_frame_requires_fixture_dir() {
    let err = frame_impl(crate::InvokeCommandInput {
      command_id: "scan.frame",
      target_application_id: None,
      inputs: &BTreeMap::new(),
      dry_run: false,
    })
    .expect_err("missing fixture-dir should fail");

    assert!(err.contains("fixture-dir"));
  }

  #[test]
  fn scan_frame_dry_run_produces_no_artifacts() {
    let output = frame_impl(crate::InvokeCommandInput {
      command_id: "scan.frame",
      target_application_id: None,
      inputs: &BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
    })
    .expect("dry-run should succeed");

    assert!(output.artifacts.is_empty());
    assert!(
      output
        .verification
        .as_deref()
        .is_some_and(|claim| claim.contains("dry-run"))
    );
  }

  #[test]
  fn scan_frame_from_fixture_dir_stages_artifacts() {
    let fixture_dir = single_frame_fixture_dir();
    let (recording, store_root) = recording("scan-frame-artifacts");
    let registry = default_registry();

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      fixture_dir.to_string_lossy().into_owned(),
    );

    let result = invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: "scan.frame".to_string(),
        target: ExecutionTarget::default(),
        inputs,
        dry_run: false,
      },
    )
    .expect("invoke should succeed");

    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(result.artifacts.len(), 2);

    result
      .artifacts
      .iter()
      .find(|artifact| artifact.role == "scan-frame-v0")
      .expect("scan-frame-v0 artifact record");
    result
      .artifacts
      .iter()
      .find(|artifact| artifact.role == "scan-frame-image")
      .expect("scan-frame-image artifact record");

    let json_staged = result
      .artifact_paths
      .iter()
      .find(|path| path.extension().is_some_and(|ext| ext == "json"))
      .expect("staged json path");
    let png_staged = result
      .artifact_paths
      .iter()
      .find(|path| path.extension().is_some_and(|ext| ext == "png"))
      .expect("staged png path");

    assert!(json_staged.is_file());
    assert!(png_staged.is_file());
    assert!(
      json_staged
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("artifact_"))
    );

    let frame = read_frame_artifact(json_staged).expect("read staged json");
    frame.validate_wire().expect("wire should validate");

    let canonical = recording
      .read_run(result.run_id.as_str())
      .expect("run should persist");
    assert_eq!(canonical.run.status_code, TraceStatusCode::Ok);

    let _ = fs::remove_dir_all(store_root);
  }
}
