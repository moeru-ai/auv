use std::path::PathBuf;

use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportValue,
  artifact::{emit_bytes_with_receipt, emit_prepared_with_receipt},
  invoke_command,
};
use auv_scan::{build_coverage_fixture, load_frame_fixture};
use auv_tracing::{ArtifactUri, Attributes, ByteLength, EmitBytesOptions, NewArtifact};
use clap::Args;
use futures_util::io::Cursor as AsyncCursor;
use serde::Serialize;

const SCAN_COVERAGE_PURPOSE: &str = "auv.runtime.scan_coverage";
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

pub fn group() -> CommandGroup {
  CommandGroup::new("scan", "SCAN").command(frame_invoke_command()).command(coverage_invoke_command())
}

#[derive(Clone, Debug, Args, Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke scan.frame --fixture-dir fixtures/frame --label baseline")]
struct ScanFrameArgs {
  /// Directory containing a single-frame scan fixture.
  #[arg(long, value_name = "PATH")]
  #[serde(rename = "fixture-dir")]
  fixture_dir: PathBuf,
  /// Human-readable label for this scan fixture invocation.
  #[arg(long)]
  label: Option<String>,
}

#[invoke_command(
  id = "scan.frame",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = NoRoute,
  role = Fixture,
  group = "scan",
  description = "Produce a single scan-frame-v0 artifact bundle from a hermetic fixture directory and stage it into the run.",
  input = ScanFrameArgs,
)]
async fn frame(input: InvokeCommandInput, args: ScanFrameArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  let (frame, artifacts) = produce_scan_frame_recorded(args.fixture_dir).await?;
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
  Ok(InvokeCommandOutput::from_result(&frame)?.with_report(InvokeReport::new(fields, Vec::new())).with_artifacts(artifacts))
}

pub async fn produce_scan_frame(fixture_dir: PathBuf) -> Result<auv_scan::ScanFrame, String> {
  produce_scan_frame_recorded(fixture_dir).await.map(|(frame, _)| frame)
}

async fn produce_scan_frame_recorded(fixture_dir: PathBuf) -> Result<(auv_scan::ScanFrame, Vec<auv_tracing::ArtifactMetadata>), String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.frame fixture directory does not exist: {}", fixture_dir.display()));
  }
  let loaded = load_frame_fixture(&fixture_dir).map_err(|error| format!("scan.frame fixture decode failed: {error}"))?;
  let (frame, image_bytes) = loaded.into_parts();
  let mut artifacts = Vec::new();
  let image_options = EmitBytesOptions::new().with_purpose("auv.scan.frame_image").with_content_type("image/png").with_file_extension("png");
  if let Some(image) = emit_bytes_with_receipt(image_options, image_bytes).await {
    let frame_artifact = emit_prepared_with_receipt("auv.scan.frame", scan_frame_artifact(&frame, image.uri())).await;
    artifacts.push(image);
    artifacts.extend(frame_artifact);
  }
  Ok((frame, artifacts))
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
    "auv.scan.frame",
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

#[derive(Clone, Debug, Args, Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke scan.coverage --fixture-dir fixtures/coverage --json")]
struct ScanCoverageArgs {
  /// Directory containing a coverage scenario manifest; frame PNGs are resolved through its frame_fixture cross-reference.
  #[arg(long, value_name = "PATH")]
  #[serde(rename = "fixture-dir")]
  fixture_dir: PathBuf,
}

#[invoke_command(
  id = "scan.coverage",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = NoRoute,
  role = Fixture,
  group = "scan",
  description = "Evaluate typed scan coverage from a fixture and record it in the active run.",
  input = ScanCoverageArgs,
)]
async fn coverage(input: InvokeCommandInput, args: ScanCoverageArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }

  let (coverage, artifact) = produce_scan_coverage_recorded(args.fixture_dir).await?;
  let completeness = match coverage.status() {
    auv_scan::CoverageStatus::Complete => "complete".to_string(),
    auv_scan::CoverageStatus::Incomplete { reason, .. } => format!("incomplete: {reason}"),
  };
  Ok(
    InvokeCommandOutput::from_result(&coverage)?
      .with_report(InvokeReport::new(
        vec![
          InvokeReportField::new("Entries", coverage.entries.len().to_string()),
          InvokeReportField::new("Open Uncertainties", coverage.open_uncertainty_codes().len().to_string()),
          InvokeReportField::new("Negative Evidence", coverage.negative_evidence().len().to_string()),
          InvokeReportField::new("Completeness", completeness),
        ],
        Vec::new(),
      ))
      .with_artifacts(artifact),
  )
}

pub async fn produce_scan_coverage(fixture_dir: PathBuf) -> Result<auv_scan::CoverageView, String> {
  produce_scan_coverage_recorded(fixture_dir).await.map(|(coverage, _)| coverage)
}

async fn produce_scan_coverage_recorded(
  fixture_dir: PathBuf,
) -> Result<(auv_scan::CoverageView, Option<auv_tracing::ArtifactMetadata>), String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.coverage fixture directory does not exist: {}", fixture_dir.display()));
  }
  let coverage = build_coverage_fixture(&fixture_dir).map_err(|error| format!("scan.coverage fixture build failed: {error}"))?;
  let artifact = emit_prepared_with_receipt(SCAN_COVERAGE_PURPOSE, scan_coverage_artifact(&coverage)).await;
  Ok((coverage, artifact))
}

fn scan_coverage_artifact(value: &auv_scan::CoverageView) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  let artifact = auv_scan::ScanCoverageArtifact::new(value.clone());
  NewArtifact::from_json(
    SCAN_COVERAGE_PURPOSE,
    Attributes::empty(),
    ByteLength::new(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static scan JSON limit is valid"),
    &artifact,
  )
  .map_err(|error| format!("failed to construct {SCAN_COVERAGE_PURPOSE} artifact: {error}"))
}

#[cfg(test)]
#[path = "scan_test.rs"]
mod tests;
