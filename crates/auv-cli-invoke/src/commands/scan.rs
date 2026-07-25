use std::path::PathBuf;

use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportValue,
  arg::{SCAN_COVERAGE_ARGS, SCAN_FRAME_ARGS},
  artifact::{emit_bytes_with_receipt, emit_prepared},
  invoke_command,
};
use auv_scan::{build_coverage_fixture, load_frame_fixture};
use auv_tracing::{ArtifactPurpose, ArtifactUri, Attributes, ByteLength, NewArtifact};
use futures_util::io::Cursor as AsyncCursor;
use serde::Serialize;

const SCAN_COVERAGE_PURPOSE: &str = "auv.runtime.scan_coverage";
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

pub fn group() -> CommandGroup {
  CommandGroup::new("scan", "SCAN").command(frame_invoke_command()).command(coverage_invoke_command())
}

#[invoke_command(
  id = "scan.frame",
  group = "scan",
  description = "Produce a single scan-frame-v0 artifact bundle from a hermetic fixture directory and stage it into the run.",
  args = SCAN_FRAME_ARGS,
)]
async fn frame(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  let frame = produce_scan_frame(PathBuf::from(&fixture_dir)).await?;

  scan_frame_output(&frame)
}

pub async fn produce_scan_frame(fixture_dir: PathBuf) -> Result<auv_scan::ScanFrame, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.frame fixture directory does not exist: {}", fixture_dir.display()));
  }
  let loaded = load_frame_fixture(&fixture_dir).map_err(|error| format!("scan.frame fixture decode failed: {error}"))?;
  let (frame, image_bytes) = loaded.into_parts();
  if let Some(image) = emit_bytes_with_receipt("auv.scan.frame_image", "image/png", image_bytes).await {
    emit_prepared("auv.scan.frame", scan_frame_artifact(&frame, image.uri()));
  }
  Ok(frame)
}

#[derive(Serialize)]
struct ScanFrameArtifact<'a> {
  frame_id: &'a str,
  sequence_index: u32,
  captured_at_millis: u64,
  window_bounds: &'a auv_scan::ScanBounds,
  #[serde(skip_serializing_if = "Option::is_none")]
  viewport_bounds: Option<&'a auv_scan::ScanBounds>,
  image: ScanFrameImageArtifact<'a>,
}

#[derive(Serialize)]
struct ScanFrameImageArtifact<'a> {
  artifact_uri: &'a ArtifactUri,
  width: u32,
  height: u32,
}

fn scan_frame_artifact(frame: &auv_scan::ScanFrame, image_uri: &ArtifactUri) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  NewArtifact::from_json(
    ArtifactPurpose::parse("auv.scan.frame").map_err(|error| format!("invalid auv.scan.frame purpose: {error}"))?,
    Attributes::empty(),
    ByteLength::new(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static scan JSON limit is valid"),
    &ScanFrameArtifact {
      frame_id: &frame.frame_id,
      sequence_index: frame.sequence_index,
      captured_at_millis: frame.captured_at_millis,
      window_bounds: &frame.window_bounds,
      viewport_bounds: frame.viewport_bounds.as_ref(),
      image: ScanFrameImageArtifact {
        artifact_uri: image_uri,
        width: frame.image_dimensions.width,
        height: frame.image_dimensions.height,
      },
    },
  )
  .map_err(|error| format!("failed to construct auv.scan.frame artifact: {error}"))
}

fn scan_frame_output(frame: &auv_scan::ScanFrame) -> InvokeCommandResult {
  let mut fields = vec![
    InvokeReportField::new("Frame ID", &frame.frame_id),
    InvokeReportField::new("Sequence", frame.sequence_index.to_string()),
    InvokeReportField::new("Captured At", format!("{} ms", frame.captured_at_millis)),
    InvokeReportField::new("Image", format!("{}x{}", frame.image_dimensions.width, frame.image_dimensions.height)),
    InvokeReportField::new("Window Bounds", frame.window_bounds.report_value()),
  ];
  if let Some(viewport) = &frame.viewport_bounds {
    fields.push(InvokeReportField::new("Viewport Bounds", viewport.report_value()));
  }
  let mut output = InvokeCommandOutput::from_result(frame)?;
  output.report = Some(InvokeReport::new(fields, Vec::new()));
  Ok(output)
}

#[invoke_command(
  id = "scan.coverage",
  group = "scan",
  description = "Evaluate typed scan coverage from a fixture and record it in the active run.",
  args = SCAN_COVERAGE_ARGS,
)]
async fn coverage(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  let coverage = produce_scan_coverage(PathBuf::from(&fixture_dir)).await?;

  scan_coverage_output(&coverage)
}

pub async fn produce_scan_coverage(fixture_dir: PathBuf) -> Result<auv_scan::CoverageView, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.coverage fixture directory does not exist: {}", fixture_dir.display()));
  }
  let coverage = build_coverage_fixture(&fixture_dir).map_err(|error| format!("scan.coverage fixture build failed: {error}"))?;
  emit_scan_coverage(&coverage);
  Ok(coverage)
}

fn emit_scan_coverage(value: &auv_scan::CoverageView) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  emit_prepared(SCAN_COVERAGE_PURPOSE, scan_coverage_artifact(value));
}

fn scan_coverage_artifact(value: &auv_scan::CoverageView) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  let artifact = auv_scan::ScanCoverageArtifact::new(value.clone());
  NewArtifact::from_json(
    ArtifactPurpose::parse(SCAN_COVERAGE_PURPOSE).map_err(|error| format!("invalid {SCAN_COVERAGE_PURPOSE} purpose: {error}"))?,
    Attributes::empty(),
    ByteLength::new(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static scan JSON limit is valid"),
    &artifact,
  )
  .map_err(|error| format!("failed to construct {SCAN_COVERAGE_PURPOSE} artifact: {error}"))
}

fn scan_coverage_output(coverage: &auv_scan::CoverageView) -> InvokeCommandResult {
  let completeness = match coverage.status() {
    auv_scan::CoverageStatus::Complete => "complete".to_string(),
    auv_scan::CoverageStatus::Incomplete { reason, .. } => format!("incomplete: {reason}"),
  };
  let mut output = InvokeCommandOutput::from_result(coverage)?;
  output.report = Some(InvokeReport::new(
    vec![
      InvokeReportField::new("Entries", coverage.entries.len().to_string()),
      InvokeReportField::new("Open Uncertainties", coverage.open_uncertainty_codes().len().to_string()),
      InvokeReportField::new("Negative Evidence", coverage.negative_evidence().len().to_string()),
      InvokeReportField::new("Completeness", completeness),
    ],
    Vec::new(),
  ));
  Ok(output)
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactMetadata, ArtifactRequest, BoxFuture, Context, ErrorCode, MemoryTracingStore, RunId, StoreError, TraceRecord,
    TracingStore, configure, dispatcher,
  };

  use crate::{
    InvokeCommand, InvokeCommandInput, InvokeCommandOutput, InvokeNamespace, arg::SCAN_COVERAGE_ARGS, default_registry, render_command_help,
  };

  use super::{
    ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, coverage, coverage_invoke_command, emit_scan_coverage, frame, frame_invoke_command,
    produce_scan_coverage, produce_scan_frame, scan_coverage_artifact,
  };

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn coverage_stable_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/coverage/coverage_stable_v0")
  }

  struct RejectArtifactStore {
    writes: AtomicUsize,
  }

  impl RejectArtifactStore {
    fn new() -> Self {
      Self {
        writes: AtomicUsize::new(0),
      }
    }
  }

  impl TracingStore for RejectArtifactStore {
    fn write(&self, _record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
      Box::pin(async { Ok(()) })
    }

    fn write_artifact(&self, _request: ArtifactRequest, _body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
      self.writes.fetch_add(1, Ordering::SeqCst);
      Box::pin(async { Err(StoreError::new(ErrorCode::parse("auv.test.scan_coverage_rejected").expect("test error code"))) })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
      Box::pin(async { Ok(()) })
    }
  }

  async fn invoke_traced(command: InvokeCommand, input: InvokeCommandInput) -> (InvokeCommandOutput, Arc<MemoryTracingStore>) {
    let store = Arc::new(MemoryTracingStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch should build");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| command.invoke(input));
    let output = root.instrument(future).await.expect("invoke should succeed");
    dispatch.flush().await.expect("tracing should flush");
    (output, store)
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
    let frame_json = serde_json::to_value(&frame).expect("frame JSON");
    assert!(!frame_json.to_string().contains("file_name"));
    assert!(!frame_json.to_string().contains("media_type"));
    assert_eq!(coverage.status(), &auv_scan::CoverageStatus::Complete);
  }

  #[tokio::test]
  async fn scan_coverage_typed_call_is_unchanged_by_publication_failure() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| produce_scan_coverage(coverage_stable_fixture_dir()));

    let coverage = root.instrument(future).await.expect("domain coverage remains successful");

    assert_eq!(coverage.status(), &auv_scan::CoverageStatus::Complete);
    assert!(dispatch.flush().await.is_err(), "recording failure remains on the dispatch");
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn scan_frame_typed_call_does_not_publish_a_dangling_frame_when_image_publication_fails() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().tracing_store(store.clone()).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| produce_scan_frame(single_frame_fixture_dir()));

    let frame = root.instrument(future).await.expect("domain frame remains successful");

    assert_eq!(frame.schema_version, auv_scan::SCAN_FRAME_SCHEMA_VERSION);
    assert!(dispatch.flush().await.is_err(), "recording failure remains on the dispatch");
    assert_eq!(store.writes.load(Ordering::SeqCst), 1, "frame payload must not be attempted without a committed image URI");
  }

  #[tokio::test]
  async fn scan_coverage_publication_short_circuits_without_run_context() {
    let coverage = auv_scan::CoverageView::incomplete(
      Vec::new(),
      "oversized detached fixture",
      vec!["x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)],
      Vec::new(),
    );

    emit_scan_coverage(&coverage);
  }

  #[test]
  fn scan_coverage_artifact_enforces_four_mibibyte_bound() {
    let oversized = auv_scan::CoverageView::incomplete(
      Vec::new(),
      "oversized fixture",
      vec!["x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)],
      Vec::new(),
    );
    let size_error = scan_coverage_artifact(&oversized).err().expect("oversized coverage must fail");
    assert!(size_error.contains("4194304-byte limit"));
  }

  #[test]
  fn scan_frame_requires_fixture_dir() {
    let err = futures_executor::block_on(frame(crate::InvokeCommandInput {
      command_id: "scan.frame".to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
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
      cancellation: crate::InvokeCancellation::new(),
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
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect("dry-run should succeed");

    assert!(output.report.is_none());
  }

  #[test]
  fn scan_coverage_dry_run_produces_no_artifacts() {
    let output = futures_executor::block_on(coverage(crate::InvokeCommandInput {
      command_id: "scan.coverage".to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect("dry-run should succeed");

    assert!(output.report.is_none());
  }

  #[test]
  fn scan_frame_from_fixture_dir_emits_owned_artifacts() {
    let fixture_dir = single_frame_fixture_dir();
    let (output, store) = futures_executor::block_on(invoke_traced(
      frame_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.frame".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
    ));
    let result = output.result().expect("scan.frame typed result");
    assert_eq!(result["schema_version"], auv_scan::SCAN_FRAME_SCHEMA_VERSION);
    assert_eq!(result["frame_id"], "frame-0001");
    assert_eq!(result["image_dimensions"]["width"], 8);
    let report = output.report.as_ref().expect("scan.frame direct value report");
    assert_eq!(
      report.fields.iter().map(|field| (field.label.as_str(), field.value.as_str())).collect::<Vec<_>>(),
      vec![
        ("Frame ID", "frame-0001"),
        ("Sequence", "0"),
        ("Captured At", "1700000000000 ms"),
        ("Image", "8x8"),
        ("Window Bounds", "0,0 800x600"),
      ]
    );

    let records = store.records();
    let artifacts = records
      .iter()
      .filter_map(|record| match record {
        TraceRecord::Artifact { metadata, .. } => Some(metadata),
        _ => None,
      })
      .collect::<Vec<_>>();
    let purposes = artifacts.iter().map(|metadata| metadata.purpose().as_str()).collect::<Vec<_>>();
    assert_eq!(purposes.len(), 2);
    assert!(purposes.contains(&"auv.scan.frame"));
    assert!(purposes.contains(&"auv.scan.frame_image"));

    let frame_metadata =
      artifacts.iter().find(|metadata| metadata.purpose().as_str() == "auv.scan.frame").expect("frame payload artifact").to_owned();
    let image_uri = artifacts
      .iter()
      .find(|metadata| metadata.purpose().as_str() == "auv.scan.frame_image")
      .expect("frame image artifact")
      .uri()
      .to_string();
    let frame_payload = serde_json::from_slice::<serde_json::Value>(&store.artifact(frame_metadata.uri()).expect("frame payload body"))
      .expect("frame payload JSON");

    assert_eq!(frame_payload.pointer("/image/artifact_uri").and_then(serde_json::Value::as_str), Some(image_uri.as_str()));
    assert!(frame_payload.pointer("/image/file_name").is_none());
    assert!(frame_payload.pointer("/image/media_type").is_none());
  }

  #[test]
  fn scan_coverage_from_fixture_dir_emits_owned_artifact() {
    let fixture_dir = coverage_stable_fixture_dir();
    let expected = futures_executor::block_on(produce_scan_coverage(fixture_dir.clone())).expect("direct typed coverage");
    let (output, store) = futures_executor::block_on(invoke_traced(
      coverage_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.coverage".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
    ));
    assert_eq!(output.result(), Some(&serde_json::to_value(&expected).expect("expected coverage JSON")));
    let report = output.report.as_ref().expect("scan.coverage direct value report");
    assert_eq!(
      report.fields.iter().map(|field| (field.label.as_str(), field.value.as_str())).collect::<Vec<_>>(),
      vec![
        ("Entries", "1"),
        ("Open Uncertainties", "0"),
        ("Negative Evidence", "0"),
        ("Completeness", "complete"),
      ]
    );

    let records = store.records();
    let artifacts = records
      .iter()
      .filter_map(|record| match record {
        TraceRecord::Artifact { metadata, .. } => Some(metadata),
        _ => None,
      })
      .collect::<Vec<_>>();
    let metadata = artifacts.first().expect("coverage artifact");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(metadata.purpose().as_str(), "auv.runtime.scan_coverage");
    assert_eq!(metadata.content_type().to_string(), "application/json");
    let actual = serde_json::from_slice::<auv_scan::ScanCoverageArtifact>(&store.artifact(metadata.uri()).expect("coverage artifact body"))
      .expect("typed coverage artifact")
      .into_coverage();
    assert_eq!(actual, expected);
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
