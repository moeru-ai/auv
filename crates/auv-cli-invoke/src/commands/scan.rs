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
#[path = "scan_test.rs"]
mod tests;
