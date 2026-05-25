// File: src/driver/macos/support/ocr_commands.rs
use std::path::PathBuf;

use super::super::*;
use crate::driver::macos::capture::types::CaptureContract;

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct TextMatchCommandReport {
  pub(crate) scope: String,
  pub(crate) capture_source: String,
  pub(crate) query: String,
  pub(crate) match_count: usize,
  pub(crate) filtered_match_count: usize,
  pub(crate) region: Option<ObservedRect>,
  pub(crate) best_match_bounds: Option<ObservedRect>,
  pub(crate) screenshot_point: Option<(f64, f64)>,
  pub(crate) logical_point: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CapturedObservation {
  pub(crate) scope: String,
  pub(crate) capture_source: String,
  pub(crate) screenshot_path: PathBuf,
  pub(crate) capture_contract: CaptureContract,
  pub(crate) dimensions: ScreenshotDimensions,
}

pub(crate) fn render_text_match_command_json(report: &TextMatchCommandReport) -> AuvResult<String> {
  serde_json::to_string_pretty(report)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode text match command report JSON: {error}"))
}

pub(crate) fn screenshot_artifact(
  capture: &CapturedObservation,
  label: &str,
  note_suffix: &str,
) -> ProducedArtifact {
  ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: capture.screenshot_path.clone(),
    preferred_name: format!("{}.png", sanitize_file_component(label)),
    note: Some(format!("Screenshot captured for {note_suffix}.")),
  }
}

pub(crate) fn run_text_match_on_capture(
  call: &DriverCall,
  capture: &CapturedObservation,
  query: &str,
) -> AuvResult<(
  OcrTextSnapshot,
  Vec<OcrTextMatch>,
  String,
  Option<TextMatchCommandReport>,
)> {
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region =
    parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;
  let ocr_capture = crate::driver::macos::native::ocr::find_text(
    capture.screenshot_path.as_path(),
    query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  )?;
  let ocr_report = crate::driver::macos::native::ocr::render_ocr_text_report(&ocr_capture);
  let snapshot = ocr_capture.snapshot;
  let filtered = filter_ocr_matches(&snapshot.matches, min_confidence, region.as_ref())
    .into_iter()
    .cloned()
    .collect::<Vec<_>>();
  let report = filtered.first().map(|best| {
    let (sx, sy) = ocr_match_center(best);
    let logical_point =
      crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
        &capture.capture_contract,
        sx,
        sy,
      )
      .ok();
    TextMatchCommandReport {
      scope: capture.scope.clone(),
      capture_source: capture.capture_source.clone(),
      query: query.to_string(),
      match_count: snapshot.matches.len(),
      filtered_match_count: filtered.len(),
      region: region.clone(),
      best_match_bounds: Some(best.bounds.clone()),
      screenshot_point: Some((sx, sy)),
      logical_point,
    }
  });
  Ok((snapshot, filtered, ocr_report, report))
}
