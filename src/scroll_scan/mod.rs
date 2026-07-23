// File: src/scroll_scan/mod.rs
//! Scroll-scan orchestration for window/region list-like content.
//!
//! `scroll_scan` produces *bounded observation evidence* (pages, row-like
//! observations, stop reasons, and corroborating artifacts). It is not a proof
//! of full UI coverage: completeness is inferred heuristically (overlap across
//! adjacent pages + screenshot stability), and callers must treat outputs as
//! inspectable evidence rather than a guarantee.
//!
//! This module owns the scan loop + artifact shaping. Low-level capture/OCR/AX
//! and action semantics live in drivers + commands.

mod observation;

use observation::{observation_from_recognized_item, observation_from_row, should_merge_adjacent_observations};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::contract::{ObservationSnapshot, RecognitionResult, SurfaceNode};
use crate::model::AuvResult;
use crate::runtime::Runtime;
use auv_cli_invoke::{ArtifactInstrumentationReceipt, ArtifactPublication};
use auv_tracing::RunId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScanRegion {
  pub left_ratio: f64,
  pub top_ratio: f64,
  pub right_ratio: f64,
  pub bottom_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScanTarget {
  pub application_id: Option<String>,
  pub window_title: Option<String>,
  pub region: ScanRegion,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopPolicy {
  UntilEnd {
    max_pages: usize,
    max_scrolls: usize,
    no_progress_limit: usize,
  },
  UntilNextSection {
    max_pages: usize,
    max_scrolls: usize,
  },
  UntilMatch {
    query: String,
    max_pages: usize,
    max_scrolls: usize,
  },
  Bounded {
    max_pages: usize,
    max_scrolls: usize,
  },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessClaim {
  /// Scan stopped after consecutive pages with no new observations while
  /// scrolling downward. Suggests the bottom of the list was reached, but no
  /// AX or scrollbar evidence corroborates the boundary.
  CompleteByNoVisualProgressDown,
  /// Scan stopped after consecutive pages with no new observations while
  /// scrolling upward. Suggests the top of the list was reached, but no
  /// AX or scrollbar evidence corroborates the boundary.
  CompleteByNoVisualProgressUp,
  /// Scan stopped due to no visual progress but the scroll direction is
  /// lateral or unknown. Kept as a fallback for directions that do not map
  /// to a top/bottom claim.
  CompleteByNoVisualProgress,
  CompleteByReachedBoundary,
  PartialMaxPages,
  PartialMaxDuration,
  PartialUnstableContent,
  PartialNextSectionCandidate,
  Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
  NoProgressLimit,
  ReachedBoundary,
  MaxPages,
  MaxScrolls,
  HookRequestedStop,
  MatchFound,
  NextSectionCandidate,
  Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanRect {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionObservation {
  pub observation_id: String,
  pub page_index: usize,
  pub raw_text: String,
  pub normalized_text_key: String,
  pub bounds: ScanRect,
  pub section_context: Option<String>,
  pub source_artifacts: Vec<PathBuf>,
  pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ObservationCluster {
  pub cluster_id: String,
  pub observation_ids: Vec<String>,
  pub representative_text: String,
  pub merge_reason: String,
  pub confidence: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SectionCandidate {
  pub section_id: String,
  pub page_index: usize,
  pub text: String,
  pub bounds: ScanRect,
  pub confidence: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollBoundary {
  Top,
  Bottom,
  Left,
  Right,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScrollBoundaryCandidate {
  pub page_index: usize,
  pub scroll_count: usize,
  pub direction: String,
  pub boundary: ScrollBoundary,
  pub basis: String,
  pub confidence: String,
  pub consecutive_no_progress: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HookDecisionRecord {
  pub hook_name: String,
  pub page_index: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub item_index: Option<usize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub row_candidate_index: Option<usize>,
  pub action: HookAction,
  pub reason: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub annotations: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub adjusted_region: Option<ScanRegion>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub adjusted_scroll: Option<ScanHookAdjustedScroll>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub retry_policy: Option<ScanHookRetryPolicy>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookAction {
  Continue,
  Stop,
  RetryObserve,
  AdjustRegion,
  AdjustScroll,
  Annotate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScanHookAdjustedScroll {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub direction: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub amount: Option<f64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settle_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScanHookRetryPolicy {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub max_attempts: Option<usize>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settle_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanPageRecord {
  pub page_index: usize,
  pub observe_run_id: Option<String>,
  pub screenshot_artifact: Option<PathBuf>,
  pub observation_count: usize,
  pub new_observation_count: usize,
  pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StopEvidence {
  pub reason: StopReason,
  pub message: String,
  pub page_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanProgress {
  pub page_index: usize,
  pub scroll_count: usize,
  pub consecutive_no_progress: usize,
  pub new_observation_count: usize,
  pub hook_stop_requested: bool,
  pub match_found: bool,
  pub next_section_candidate: bool,
  pub scroll_boundary_candidate: Option<ScrollBoundaryCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopDecision {
  pub stop_evidence: StopEvidence,
  pub completeness_claim: CompletenessClaim,
}

// REVIEW: These thresholds are intentionally conservative. They only try to
// identify "scroll likely had no visible effect" across adjacent screenshots,
// not general scene similarity. Revisit after collecting more real scan traces
// from window lists with animation, sticky headers, and partially occluded
// content.
const SCREENSHOT_STABILITY_SAMPLE_GRID: u32 = 24;
const SCREENSHOT_STABILITY_MAX_MEAN_ABS_DIFF: f64 = 0.02;
const SCREENSHOT_STABILITY_MAX_CHANGED_SAMPLE_RATIO: f64 = 0.08;
const SCREENSHOT_STABILITY_CHANGED_SAMPLE_DELTA: f64 = 0.04;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScreenshotDiffStability {
  mean_abs_diff: f64,
  changed_sample_ratio: f64,
}

impl ScreenshotDiffStability {
  fn is_stable(&self) -> bool {
    self.mean_abs_diff <= SCREENSHOT_STABILITY_MAX_MEAN_ABS_DIFF
      && self.changed_sample_ratio <= SCREENSHOT_STABILITY_MAX_CHANGED_SAMPLE_RATIO
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScrollScanArtifact {
  pub scan_id: String,
  pub target: ScanTarget,
  pub stop_policy: StopPolicy,
  pub pages: Vec<ScanPageRecord>,
  pub observations: Vec<CollectionObservation>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub nodes: Vec<SurfaceNode>,
  /// Per-page projection of the scan into the v0 `ObservationSnapshot` shape.
  /// Each page's observations are grouped into one snapshot record so future
  /// consumers can read evidence through the unified observed-UI-layer
  /// contract without knowing scroll_scan internals. Empty if a partial scan
  /// failed before producing any pages.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub snapshots: Vec<ObservationSnapshot>,
  pub clusters: Vec<ObservationCluster>,
  pub section_candidates: Vec<SectionCandidate>,
  pub scroll_boundary_candidates: Vec<ScrollBoundaryCandidate>,
  pub hook_decisions: Vec<HookDecisionRecord>,
  pub stop_evidence: StopEvidence,
  pub completeness_claim: CompletenessClaim,
  pub warnings: Vec<String>,
}

pub const SCROLL_SCAN_PURPOSE: &str = "auv.runtime.scroll_scan";
/// Scroll-scan JSON is inspectable structured evidence, not bulk telemetry.
/// Eight MiB accommodates thousands of row observations plus their node and
/// snapshot projections while bounding the producer and every reader.
pub const SCROLL_SCAN_JSON_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
pub const SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE: &str = "auv.runtime.scroll_scan.payload_too_large";

#[derive(Clone, Debug)]
pub struct ScanWindowRegionOptions {
  pub target: ScanTarget,
  pub stop_policy: StopPolicy,
  pub direction: String,
  pub scroll_amount: f64,
  pub settle_ms: u64,
  pub min_confidence: f64,
  pub max_observations: i64,
}

// TODO(tracing-interaction-hooks): recipe-backed scan hooks were removed with
// JSON recipe execution. Reintroduce hook composition only as typed Rust
// interaction hooks once `auv-tracing-interaction` owns macro-operation
// recording.
pub async fn scan_window_region(runtime: &Runtime, options: ScanWindowRegionOptions) -> ArtifactPublication<AuvResult<RunId>> {
  let _project_root = runtime.project_root();
  let error = "scroll scan requires a typed window region observation API".to_string();
  let artifact = ScrollScanArtifact {
    scan_id: format!("scan_{}", auv_tracing::Context::current().run_id().map_or_else(|| "disabled".to_string(), |id| id.to_string())),
    target: options.target,
    stop_policy: options.stop_policy,
    pages: Vec::new(),
    observations: Vec::new(),
    nodes: Vec::new(),
    snapshots: Vec::new(),
    clusters: Vec::new(),
    section_candidates: Vec::new(),
    scroll_boundary_candidates: Vec::new(),
    hook_decisions: Vec::new(),
    stop_evidence: StopEvidence {
      reason: StopReason::Error,
      message: error.clone(),
      page_index: 0,
    },
    completeness_claim: CompletenessClaim::Unknown,
    warnings: vec!["scan ended with an error; artifact is partial".to_string()],
  };
  let mut instrumentation = ArtifactInstrumentationReceipt::default();
  instrumentation
    .publish_json_bounded(SCROLL_SCAN_PURPOSE, &artifact, SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE)
    .await;
  ArtifactPublication::new(Err(error), instrumentation)
}

pub fn observations_from_observe_json(page_index: usize, raw: &str, source_artifact: PathBuf) -> AuvResult<Vec<CollectionObservation>> {
  let value: Value = serde_json::from_str(raw).map_err(|error| format!("malformed observe JSON: {error}"))?;
  if let Some(recognition) = recognition_result_from_value(&value)? {
    return Ok(observations_from_recognition_result(page_index, &recognition, &source_artifact));
  }
  let rows = value
    .get("item_candidates")
    .and_then(Value::as_array)
    .filter(|candidates| !candidates.is_empty())
    .or_else(|| value.get("rows").and_then(Value::as_array))
    .ok_or_else(|| "malformed observe JSON: missing rows array".to_string())?;

  rows.iter().enumerate().map(|(row_index, row)| observation_from_row(page_index, row_index, row, &source_artifact)).collect()
}

/// Cheap structural check: does this JSON value carry the markers of a `RecognitionResult`?
/// Used to discriminate observe JSON formats without paying for a full deserialization.
fn has_recognition_result_shape(value: &Value) -> bool {
  value.get("recognition_id").is_some() && value.get("source").is_some()
}

/// Returns `Ok(None)` if the value clearly is not a `RecognitionResult` (no discriminator fields),
/// `Ok(Some(_))` on a clean parse, or `Err` if the value looks like one but fails to deserialize —
/// so the caller can surface the underlying serde error instead of silently falling back to the
/// legacy `rows` path with a misleading "missing rows array" message.
fn recognition_result_from_value(value: &Value) -> AuvResult<Option<RecognitionResult>> {
  if !has_recognition_result_shape(value) {
    return Ok(None);
  }
  serde_json::from_value(value.clone()).map(Some).map_err(|error| format!("recognition result JSON failed to deserialize: {error}"))
}

fn observations_from_recognition_result(
  page_index: usize,
  recognition: &RecognitionResult,
  source_artifact: &Path,
) -> Vec<CollectionObservation> {
  let items = if recognition.filtered.is_empty() {
    &recognition.all
  } else {
    &recognition.filtered
  };
  items
    .iter()
    .enumerate()
    .map(|(item_index, item)| observation_from_recognized_item(page_index, item_index, item, recognition, source_artifact))
    .collect()
}

/// Returns the direction-aware `CompleteByNoVisualProgress*` claim for the
/// given scan direction. Downward → `CompleteByNoVisualProgressDown`, upward →
/// `CompleteByNoVisualProgressUp`, anything else → the generic fallback.
fn direction_aware_no_progress_claim(direction: &str) -> CompletenessClaim {
  match direction.trim().to_ascii_lowercase().as_str() {
    "down" => CompletenessClaim::CompleteByNoVisualProgressDown,
    "up" => CompletenessClaim::CompleteByNoVisualProgressUp,
    _ => CompletenessClaim::CompleteByNoVisualProgress,
  }
}

pub fn evaluate_stop_policy(policy: &StopPolicy, progress: &ScanProgress, direction: &str) -> Option<StopDecision> {
  if progress.hook_stop_requested {
    return Some(stop_decision(StopReason::HookRequestedStop, "scan hook requested stop", progress.page_index, CompletenessClaim::Unknown));
  }
  if progress.match_found {
    return Some(stop_decision(StopReason::MatchFound, "target match found", progress.page_index, CompletenessClaim::Unknown));
  }
  if progress.next_section_candidate && matches!(policy, StopPolicy::UntilNextSection { .. }) {
    return Some(stop_decision(
      StopReason::NextSectionCandidate,
      "next section candidate observed",
      progress.page_index,
      CompletenessClaim::PartialNextSectionCandidate,
    ));
  }
  if let Some(boundary_candidate) = &progress.scroll_boundary_candidate
    && !matches!(policy, StopPolicy::Bounded { .. })
  {
    return Some(stop_decision(
      StopReason::ReachedBoundary,
      format!(
        "directional {} boundary candidate observed after {} scroll(s): {}",
        scroll_boundary_name(boundary_candidate.boundary),
        boundary_candidate.scroll_count,
        boundary_candidate.basis
      ),
      progress.page_index,
      CompletenessClaim::CompleteByReachedBoundary,
    ));
  }

  match policy {
    StopPolicy::UntilEnd {
      max_pages,
      max_scrolls,
      no_progress_limit,
    } => bounded_or_no_progress_stop(*max_pages, *max_scrolls, *no_progress_limit, direction, progress),
    StopPolicy::UntilNextSection {
      max_pages,
      max_scrolls,
    }
    | StopPolicy::UntilMatch {
      max_pages,
      max_scrolls,
      ..
    }
    | StopPolicy::Bounded {
      max_pages,
      max_scrolls,
    } => bounded_stop(*max_pages, *max_scrolls, progress),
  }
}

fn bounded_or_no_progress_stop(
  max_pages: usize,
  max_scrolls: usize,
  no_progress_limit: usize,
  direction: &str,
  progress: &ScanProgress,
) -> Option<StopDecision> {
  if progress.consecutive_no_progress >= no_progress_limit {
    // Emit a direction-aware completeness claim so callers can distinguish
    // "reached the bottom" from "reached the top" without inspecting the
    // scan direction separately. The claim is still heuristic (no AX scroll
    // position or scrollbar-thumb evidence backs it) but is more precise than
    // the generic CompleteByNoVisualProgress fallback.
    // A future layer should corroborate further with AX scroll values or
    // provider-reported scroll state (see TODO 1385).
    let claim = direction_aware_no_progress_claim(direction);
    return Some(stop_decision(
      StopReason::NoProgressLimit,
      format!(
        "reached no_progress_limit={no_progress_limit} scrolling {direction} \
         ({} consecutive page(s) with no new observations)",
        progress.consecutive_no_progress,
      ),
      progress.page_index,
      claim,
    ));
  }
  bounded_stop(max_pages, max_scrolls, progress)
}

fn bounded_stop(max_pages: usize, max_scrolls: usize, progress: &ScanProgress) -> Option<StopDecision> {
  if progress.page_index + 1 >= max_pages {
    return Some(stop_decision(
      StopReason::MaxPages,
      format!("reached max_pages={max_pages}"),
      progress.page_index,
      CompletenessClaim::PartialMaxPages,
    ));
  }
  if progress.scroll_count >= max_scrolls {
    return Some(stop_decision(
      StopReason::MaxScrolls,
      format!("reached max_scrolls={max_scrolls}"),
      progress.page_index,
      CompletenessClaim::Unknown,
    ));
  }
  None
}

fn stop_decision(reason: StopReason, message: impl Into<String>, page_index: usize, completeness_claim: CompletenessClaim) -> StopDecision {
  StopDecision {
    stop_evidence: StopEvidence {
      reason,
      message: message.into(),
      page_index,
    },
    completeness_claim,
  }
}

fn error_stop_decision(page_index: usize) -> StopDecision {
  stop_decision(StopReason::Error, "scan stopped because an orchestration step failed", page_index, CompletenessClaim::Unknown)
}

fn scroll_boundary_candidate_for_progress(
  direction: &str,
  page_index: usize,
  scroll_count: usize,
  consecutive_no_progress: usize,
  new_observation_count: usize,
  observations: &[CollectionObservation],
  screenshot_diff_stability: Option<&ScreenshotDiffStability>,
) -> Option<ScrollBoundaryCandidate> {
  if page_index == 0 || scroll_count == 0 || new_observation_count > 0 {
    return None;
  }
  let normalized_direction = direction.trim().to_ascii_lowercase();
  let boundary = scroll_boundary_for_direction(&normalized_direction)?;
  let repeated_overlap_count = repeated_row_band_overlap_count(page_index, observations);
  let screenshot_stable = screenshot_diff_stability.is_some_and(|stability| stability.is_stable());
  let (basis, confidence) = match (repeated_overlap_count >= 2, screenshot_stable) {
    (true, true) => ("repeated_row_band_overlap+screenshot_diff_stability", "corroborated"),
    (true, false) => ("repeated_row_band_overlap", "corroborated"),
    (false, true) => ("screenshot_diff_stability", "corroborated"),
    (false, false) => ("no_new_observations_after_scroll", "heuristic"),
  };
  Some(ScrollBoundaryCandidate {
    page_index,
    scroll_count,
    direction: normalized_direction,
    boundary,
    basis: basis.to_string(),
    confidence: confidence.to_string(),
    consecutive_no_progress,
  })
}

fn screenshot_diff_stability_for_pages(page_index: usize, pages: &[ScanPageRecord]) -> AuvResult<Option<ScreenshotDiffStability>> {
  if page_index == 0 {
    return Ok(None);
  }
  let previous_screenshot = pages.iter().find(|page| page.page_index == page_index - 1).and_then(|page| page.screenshot_artifact.as_deref());
  let current_screenshot = pages.iter().find(|page| page.page_index == page_index).and_then(|page| page.screenshot_artifact.as_deref());
  let (Some(previous_screenshot), Some(current_screenshot)) = (previous_screenshot, current_screenshot) else {
    return Ok(None);
  };
  screenshot_diff_stability(previous_screenshot, current_screenshot).map(Some)
}

fn screenshot_diff_stability(previous_screenshot: &Path, current_screenshot: &Path) -> AuvResult<ScreenshotDiffStability> {
  let previous = image::open(previous_screenshot)
    .map_err(|error| format!("failed to open previous screenshot {}: {error}", previous_screenshot.display()))?;
  let current = image::open(current_screenshot)
    .map_err(|error| format!("failed to open current screenshot {}: {error}", current_screenshot.display()))?;

  let previous = previous.to_rgba8();
  let current = current.to_rgba8();
  let width = previous.width();
  let height = previous.height();
  if width == 0 || height == 0 {
    return Err("cannot compare zero-sized screenshots for boundary evidence".to_string());
  }
  if width != current.width() || height != current.height() {
    return Ok(ScreenshotDiffStability {
      mean_abs_diff: 1.0,
      changed_sample_ratio: 1.0,
    });
  }

  let sample_grid_x = SCREENSHOT_STABILITY_SAMPLE_GRID.min(width);
  let sample_grid_y = SCREENSHOT_STABILITY_SAMPLE_GRID.min(height);
  let mut total_diff = 0.0;
  let mut changed_samples = 0usize;
  let mut sample_count = 0usize;

  for sample_y in 0..sample_grid_y {
    let y = if sample_grid_y == 1 {
      0
    } else {
      sample_y * (height - 1) / (sample_grid_y - 1)
    };
    for sample_x in 0..sample_grid_x {
      let x = if sample_grid_x == 1 {
        0
      } else {
        sample_x * (width - 1) / (sample_grid_x - 1)
      };
      let previous_pixel = previous.get_pixel(x, y).0;
      let current_pixel = current.get_pixel(x, y).0;
      let pixel_diff = (f64::from(previous_pixel[0].abs_diff(current_pixel[0]))
        + f64::from(previous_pixel[1].abs_diff(current_pixel[1]))
        + f64::from(previous_pixel[2].abs_diff(current_pixel[2])))
        / (255.0 * 3.0);
      total_diff += pixel_diff;
      if pixel_diff >= SCREENSHOT_STABILITY_CHANGED_SAMPLE_DELTA {
        changed_samples += 1;
      }
      sample_count += 1;
    }
  }

  if sample_count == 0 {
    return Err("no screenshot samples available for boundary evidence".to_string());
  }

  Ok(ScreenshotDiffStability {
    mean_abs_diff: total_diff / sample_count as f64,
    changed_sample_ratio: changed_samples as f64 / sample_count as f64,
  })
}

fn repeated_row_band_overlap_count(page_index: usize, observations: &[CollectionObservation]) -> usize {
  if page_index == 0 {
    return 0;
  }
  let previous_page = page_index - 1;
  let previous = observations.iter().filter(|observation| observation.page_index == previous_page).collect::<Vec<_>>();
  let current = observations.iter().filter(|observation| observation.page_index == page_index).collect::<Vec<_>>();
  let mut matched_previous = BTreeSet::new();
  let mut overlap_count = 0;

  for observation in current {
    if let Some((previous_index, _)) = previous
      .iter()
      .enumerate()
      .find(|(previous_index, candidate)| !matched_previous.contains(previous_index) && repeated_row_band_overlap(candidate, observation))
    {
      matched_previous.insert(previous_index);
      overlap_count += 1;
    }
  }

  overlap_count
}

fn repeated_row_band_overlap(left: &CollectionObservation, right: &CollectionObservation) -> bool {
  if !should_merge_adjacent_observations(left, right) {
    return false;
  }
  rect_overlap_ratio(left.bounds.x, left.bounds.width, right.bounds.x, right.bounds.width) >= 0.5
    && rect_overlap_ratio(left.bounds.y, left.bounds.height, right.bounds.y, right.bounds.height) >= 0.6
}

fn rect_overlap_ratio(start_a: i64, size_a: i64, start_b: i64, size_b: i64) -> f64 {
  if size_a <= 0 || size_b <= 0 {
    return 0.0;
  }
  let end_a = start_a + size_a;
  let end_b = start_b + size_b;
  let overlap = (end_a.min(end_b) - start_a.max(start_b)).max(0);
  overlap as f64 / size_a.min(size_b) as f64
}

fn scroll_boundary_for_direction(direction: &str) -> Option<ScrollBoundary> {
  match direction.trim().to_ascii_lowercase().as_str() {
    "up" => Some(ScrollBoundary::Top),
    "down" => Some(ScrollBoundary::Bottom),
    "left" => Some(ScrollBoundary::Left),
    "right" => Some(ScrollBoundary::Right),
    _ => None,
  }
}

fn scroll_boundary_name(boundary: ScrollBoundary) -> &'static str {
  match boundary {
    ScrollBoundary::Top => "top",
    ScrollBoundary::Bottom => "bottom",
    ScrollBoundary::Left => "left",
    ScrollBoundary::Right => "right",
  }
}
