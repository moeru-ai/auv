use std::path::PathBuf;

use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{SCAN_COVERAGE_ARGS, SCAN_FRAME_ARGS},
  artifact::{emission_enabled, file_artifact},
  invoke_command,
};
use auv_scan::{produce_coverage_from_fixture_dir, produce_frame_from_fixture_dir};
use tempfile::TempDir;

pub fn group() -> CommandGroup {
  CommandGroup::new("scan", "SCAN").command(frame_invoke_command()).command(coverage_invoke_command())
}

#[invoke_command(
  id = "scan.frame",
  group = "scan",
  summary = "Produce a single scan-frame-v0 artifact bundle from a hermetic fixture directory and stage it into the run.",
  args = SCAN_FRAME_ARGS,
)]
async fn frame(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("scan.frame dry-run");
    output.verification = Some("dry-run; no artifacts produced".to_string());
    output.known_limits.push("scan.frame dry-run does not write scan artifacts.".to_string());
    return Ok(output);
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  produce_scan_frame(PathBuf::from(&fixture_dir)).await?;

  let mut output = InvokeCommandOutput::new(format!("scan frame produced from fixture {}", fixture_dir));
  output.backend = Some("auv-scan.produce_frame_from_fixture_dir".to_string());
  output.verification = Some("capture-only; no semantic success claim".to_string());
  output.known_limits.push("scan.frame records a single scan-frame-v0 bundle only; multi-frame invoke is deferred.".to_string());
  Ok(output)
}

pub async fn produce_scan_frame(fixture_dir: PathBuf) -> Result<auv_scan::ScanFrame, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.frame fixture directory does not exist: {}", fixture_dir.display()));
  }
  // The producer directory remains alive until both owned artifact readers are admitted.
  let producer_out = TempDir::new().map_err(|error| format!("scan.frame failed to create producer output directory: {error}"))?;
  let produced =
    produce_frame_from_fixture_dir(&fixture_dir, producer_out.path()).map_err(|error| format!("scan.frame producer failed: {error}"))?;
  if emission_enabled() {
    if let Ok(artifact) = file_artifact("auv.scan.frame", "application/json", &produced.json_path, auv_tracing::Attributes::empty()) {
      let _ = auv_tracing::emit_artifact!(artifact).await;
    }
    if let Ok(artifact) = file_artifact("auv.scan.frame_image", "image/png", &produced.image_path, auv_tracing::Attributes::empty()) {
      let _ = auv_tracing::emit_artifact!(artifact).await;
    }
  }
  Ok(produced.frame)
}

#[invoke_command(
  id = "scan.coverage",
  group = "scan",
  summary = "Produce a scan-coverage-v0 artifact from a coverage scenario fixture and stage it into the run.",
  args = SCAN_COVERAGE_ARGS,
)]
async fn coverage(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("scan.coverage dry-run");
    output.verification = Some("dry-run; no artifacts produced".to_string());
    output.known_limits.push("scan.coverage dry-run does not write scan artifacts.".to_string());
    return Ok(output);
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  produce_scan_coverage(PathBuf::from(&fixture_dir)).await?;

  let mut output = InvokeCommandOutput::new(format!("scan coverage produced from fixture {fixture_dir}"));
  output.backend = Some("auv-scan.produce_coverage_from_fixture_dir".to_string());
  output.verification = Some("evaluator + projection; no semantic success claim".to_string());
  output.known_limits.push(
    "scan.coverage resolves frame PNGs via manifest frame_fixture cross-reference under .../scan/coverage/<scenario>/ layout only."
      .to_string(),
  );
  Ok(output)
}

pub async fn produce_scan_coverage(fixture_dir: PathBuf) -> Result<auv_scan::ScanCoverageWire, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.coverage fixture directory does not exist: {}", fixture_dir.display()));
  }
  // The producer directory remains alive until the owned artifact reader is admitted.
  let producer_out = TempDir::new().map_err(|error| format!("scan.coverage failed to create producer output directory: {error}"))?;
  let produced = produce_coverage_from_fixture_dir(&fixture_dir, producer_out.path())
    .map_err(|error| format!("scan.coverage producer failed: {error}"))?;
  if emission_enabled()
    && let Ok(artifact) =
      file_artifact("auv.runtime.scan_coverage", "application/json", &produced.json_path, auv_tracing::Attributes::empty())
  {
    let _ = auv_tracing::emit_artifact!(artifact).await;
  }
  Ok(produced.wire)
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::path::PathBuf;
  use std::sync::Arc;

  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use crate::{
    InvokeCommand, InvokeCommandInput, InvokeCommandOutput, InvokeNamespace, arg::SCAN_COVERAGE_ARGS, default_registry, render_command_help,
  };

  use super::{coverage, coverage_invoke_command, frame, frame_invoke_command, produce_scan_coverage, produce_scan_frame};

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn coverage_stable_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/coverage/coverage_stable_v0")
  }

  async fn invoke_traced(command: InvokeCommand, input: InvokeCommandInput) -> (InvokeCommandOutput, Arc<MemoryRunStore>, RunId) {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("dispatch should build");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future = root.in_scope(|| command.invoke(input));
    let output = root.instrument(future).await.expect("invoke should succeed");
    dispatch.flush().await.expect("tracing should flush");
    (output, store, run_id)
  }

  #[test]
  fn scan_frame_command_uses_scan_namespace() {
    let command = frame_invoke_command();
    assert_eq!(command.id, "scan.frame");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_command_uses_scan_namespace() {
    let command = coverage_invoke_command();
    assert_eq!(command.id, "scan.coverage");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_frame_is_registered_in_default_registry() {
    let registry = default_registry();
    let command = registry.resolve("scan.frame").expect("scan.frame should be registered");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_is_registered_in_default_registry() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("scan.coverage should be registered");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_args_use_coverage_fixture_help() {
    assert_eq!(SCAN_COVERAGE_ARGS.len(), 1);
    assert!(SCAN_COVERAGE_ARGS[0].help.contains("coverage scenario manifest"));
    assert!(SCAN_COVERAGE_ARGS[0].help.contains("frame_fixture cross-reference"));
  }

  #[test]
  fn typed_scan_calls_return_domain_values_without_cli_context() {
    let frame = futures_executor::block_on(produce_scan_frame(single_frame_fixture_dir())).expect("typed frame");
    let coverage = futures_executor::block_on(produce_scan_coverage(coverage_stable_fixture_dir())).expect("typed coverage");

    assert_eq!(frame.schema_version, auv_scan::SCAN_FRAME_SCHEMA_VERSION);
    assert_eq!(coverage.schema_version, auv_scan::SCAN_COVERAGE_SCHEMA_VERSION);
  }

  #[test]
  fn scan_frame_requires_fixture_dir() {
    let err = futures_executor::block_on(frame(crate::InvokeCommandInput {
      command_id: "scan.frame".to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
    }))
    .expect_err("missing fixture-dir should fail");

    assert!(err.contains("fixture-dir"));
  }

  #[test]
  fn scan_coverage_requires_fixture_dir() {
    let err = futures_executor::block_on(coverage(crate::InvokeCommandInput {
      command_id: "scan.coverage".to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
    }))
    .expect_err("missing fixture-dir should fail");

    assert!(err.contains("fixture-dir"));
  }

  #[test]
  fn scan_frame_dry_run_produces_no_artifacts() {
    let output = futures_executor::block_on(frame(crate::InvokeCommandInput {
      command_id: "scan.frame".to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
    }))
    .expect("dry-run should succeed");

    assert!(output.artifacts.is_empty());
    assert!(output.verification.as_deref().is_some_and(|claim| claim.contains("dry-run")));
  }

  #[test]
  fn scan_coverage_dry_run_produces_no_artifacts() {
    let output = futures_executor::block_on(coverage(crate::InvokeCommandInput {
      command_id: "scan.coverage".to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
    }))
    .expect("dry-run should succeed");

    assert!(output.artifacts.is_empty());
    assert!(output.verification.as_deref().is_some_and(|claim| claim.contains("dry-run")));
  }

  #[test]
  fn scan_frame_from_fixture_dir_emits_owned_artifacts() {
    let fixture_dir = single_frame_fixture_dir();
    let (output, store, run_id) = futures_executor::block_on(invoke_traced(
      frame_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.frame".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
      },
    ));

    assert!(output.artifacts.is_empty(), "binary artifact bytes are not CLI presentation state");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("recorded run");
    let purposes = snapshot.artifacts().values().map(|publication| publication.metadata().purpose().as_str()).collect::<Vec<_>>();
    assert_eq!(purposes.len(), 2);
    assert!(purposes.contains(&"auv.scan.frame"));
    assert!(purposes.contains(&"auv.scan.frame_image"));
  }

  #[test]
  fn scan_coverage_from_fixture_dir_emits_owned_artifact() {
    let fixture_dir = coverage_stable_fixture_dir();
    let (output, store, run_id) = futures_executor::block_on(invoke_traced(
      coverage_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.coverage".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
      },
    ));

    assert!(output.artifacts.is_empty(), "binary artifact bytes are not CLI presentation state");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("recorded run");
    let publication = snapshot.artifacts().values().next().expect("coverage artifact");
    assert_eq!(snapshot.artifacts().len(), 1);
    assert_eq!(publication.metadata().purpose().as_str(), "auv.runtime.scan_coverage");
    assert_eq!(publication.metadata().content_type().to_string(), "application/json");
  }

  #[test]
  fn help_lists_scan_coverage_with_coverage_fixture_help() {
    let command = coverage_invoke_command();
    let help = render_command_help(&command);
    assert!(help.contains("scan.coverage"));
    assert!(help.contains("coverage scenario manifest"));
    assert!(help.contains("frame_fixture cross-reference"));
  }
}
