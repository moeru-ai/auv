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

use observation::{conservative_merge_observations, should_merge_adjacent_observations};

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::model::{AuvResult, now_millis};
use crate::run_read::{emit_scan_coverage, emit_scroll_scan};
use auv_driver::WindowInput as _;
use auv_scan::{CoverageEntry, CoverageView, NegativeEvidence};
use image::RgbaImage;
use serde::{Deserialize, Serialize};

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
  Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
  NoProgressLimit,
  ReachedBoundary,
  MaxPages,
  MaxScrolls,
  MatchFound,
  Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanRect {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CollectionObservation {
  pub observation_id: String,
  pub page_index: usize,
  pub raw_text: String,
  pub normalized_text_key: String,
  pub bounds: ScanRect,
  pub section_context: Option<String>,
  pub provider_score: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ObservationCluster {
  pub cluster_id: String,
  pub observation_ids: Vec<String>,
  pub representative_text: String,
  pub merge_reason: String,
  pub confidence: f64,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanPageRecord {
  pub page_index: usize,
  pub observation_count: usize,
  pub new_observation_count: usize,
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
  pub match_found: bool,
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
  pub clusters: Vec<ObservationCluster>,
  pub scroll_boundary_candidates: Vec<ScrollBoundaryCandidate>,
  pub stop_evidence: StopEvidence,
  pub completeness_claim: CompletenessClaim,
  pub warnings: Vec<String>,
}

pub const SCROLL_SCAN_PURPOSE: &str = "auv.runtime.scroll_scan";
/// Scroll-scan JSON is inspectable structured evidence, not bulk telemetry.
/// Eight MiB accommodates thousands of row observations while bounding the
/// producer and every reader.
pub const SCROLL_SCAN_JSON_BYTE_LIMIT: u64 = 8 * 1024 * 1024;
pub const SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE: &str = "auv.runtime.scroll_scan.payload_too_large";

pub(crate) fn validate_scroll_scan_artifact(artifact: &ScrollScanArtifact) -> AuvResult<()> {
  validate_scan_id(&artifact.scan_id)?;
  validate_scan_region(&artifact.target.region)?;
  if max_pages_for_policy(&artifact.stop_policy) == 0 {
    return Err("scroll-scan max_pages must be greater than zero".to_string());
  }
  if artifact.pages.len() > max_pages_for_policy(&artifact.stop_policy) {
    return Err(format!(
      "page count {} exceeds stop policy max_pages={}",
      artifact.pages.len(),
      max_pages_for_policy(&artifact.stop_policy)
    ));
  }

  let mut observation_ids = BTreeSet::new();
  let mut observation_counts = vec![0usize; artifact.pages.len()];
  let mut new_observation_counts = vec![0usize; artifact.pages.len()];
  let mut known_observation_signatures = BTreeSet::new();
  let mut previous_page_index = None;
  for observation in &artifact.observations {
    if observation.observation_id.trim().is_empty() {
      return Err("observation_id must not be empty".to_string());
    }
    if !observation_ids.insert(observation.observation_id.as_str()) {
      return Err(format!("duplicate observation_id {:?}", observation.observation_id));
    }
    if observation.page_index >= artifact.pages.len() {
      return Err(format!("observation page_index {} has no matching page", observation.page_index));
    }
    if previous_page_index.is_some_and(|previous| previous > observation.page_index) {
      return Err("observations must be ordered by nondecreasing page_index".to_string());
    }
    if observation.provider_score.is_some_and(|score| !score.is_finite() || !(0.0..=1.0).contains(&score)) {
      return Err(format!("observation {:?} provider_score must be inside 0..=1", observation.observation_id));
    }
    previous_page_index = Some(observation.page_index);
    observation_counts[observation.page_index] += 1;
    if known_observation_signatures.insert(observation_signature(observation)) {
      new_observation_counts[observation.page_index] += 1;
    }
  }

  for (expected_page_index, page) in artifact.pages.iter().enumerate() {
    if page.page_index != expected_page_index {
      return Err(format!(
        "page indices must be contiguous and ordered from zero: found {} at position {expected_page_index}",
        page.page_index
      ));
    }
    if page.observation_count != observation_counts[expected_page_index] {
      return Err(format!(
        "page {expected_page_index} observation_count {} does not match {} observations",
        page.observation_count, observation_counts[expected_page_index]
      ));
    }
    if page.new_observation_count != new_observation_counts[expected_page_index] {
      return Err(format!(
        "page {expected_page_index} new_observation_count {} does not match {} newly observed signatures",
        page.new_observation_count, new_observation_counts[expected_page_index]
      ));
    }
  }

  validate_scroll_scan_clusters(artifact, &observation_ids)?;
  validate_scroll_scan_page_references(artifact)?;
  validate_scroll_scan_stop_status(artifact)?;
  Ok(())
}

fn validate_scan_id(scan_id: &str) -> AuvResult<()> {
  if scan_id.is_empty() || !scan_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')) {
    return Err("scan_id must be non-empty and contain only ASCII letters, digits, '.', '-', or '_'".to_string());
  }
  Ok(())
}

fn validate_scan_region(region: &ScanRegion) -> AuvResult<()> {
  let ratios = [
    region.left_ratio,
    region.top_ratio,
    region.right_ratio,
    region.bottom_ratio,
  ];
  if ratios.iter().any(|ratio| !ratio.is_finite())
    || !(0.0..1.0).contains(&region.left_ratio)
    || !(0.0..1.0).contains(&region.top_ratio)
    || !(0.0..=1.0).contains(&region.right_ratio)
    || !(0.0..=1.0).contains(&region.bottom_ratio)
    || region.left_ratio >= region.right_ratio
    || region.top_ratio >= region.bottom_ratio
  {
    return Err("scroll-scan region ratios must define a non-empty rectangle inside 0..=1".to_string());
  }
  Ok(())
}

fn validate_scroll_scan_clusters(artifact: &ScrollScanArtifact, observation_ids: &BTreeSet<&str>) -> AuvResult<()> {
  let mut cluster_ids = BTreeSet::new();
  let mut clustered_observation_ids = BTreeSet::new();
  for cluster in &artifact.clusters {
    if cluster.cluster_id.trim().is_empty() {
      return Err("cluster_id must not be empty".to_string());
    }
    if !cluster_ids.insert(cluster.cluster_id.as_str()) {
      return Err(format!("duplicate cluster_id {:?}", cluster.cluster_id));
    }
    if cluster.observation_ids.is_empty() {
      return Err(format!("cluster {:?} must reference at least one observation", cluster.cluster_id));
    }
    if !cluster.confidence.is_finite() || !(0.0..=1.0).contains(&cluster.confidence) {
      return Err(format!("cluster {:?} confidence must be inside 0..=1", cluster.cluster_id));
    }
    for observation_id in &cluster.observation_ids {
      if !observation_ids.contains(observation_id.as_str()) {
        return Err(format!("cluster observation reference {observation_id:?} does not match an artifact observation"));
      }
      if !clustered_observation_ids.insert(observation_id.as_str()) {
        return Err(format!("cluster observation reference {observation_id:?} appears more than once"));
      }
    }
  }
  if clustered_observation_ids != *observation_ids {
    return Err("clusters must reference every observation exactly once".to_string());
  }
  Ok(())
}

fn validate_scroll_scan_page_references(artifact: &ScrollScanArtifact) -> AuvResult<()> {
  let page_count = artifact.pages.len();
  for candidate in &artifact.scroll_boundary_candidates {
    if candidate.page_index >= page_count {
      return Err(format!("scroll boundary page_index {} has no matching page", candidate.page_index));
    }
  }
  Ok(())
}

fn validate_scroll_scan_stop_status(artifact: &ScrollScanArtifact) -> AuvResult<()> {
  if artifact.stop_evidence.message.trim().is_empty() {
    return Err("stop_evidence message must not be empty".to_string());
  }
  let last_page_index = artifact.pages.len().checked_sub(1);
  let valid_stop_index = match artifact.stop_evidence.reason {
    StopReason::Error => match last_page_index {
      Some(last) => artifact.stop_evidence.page_index == last || artifact.stop_evidence.page_index == artifact.pages.len(),
      None => artifact.stop_evidence.page_index == 0,
    },
    _ => last_page_index == Some(artifact.stop_evidence.page_index),
  };
  if !valid_stop_index {
    return Err(format!(
      "stop_evidence page_index {} is inconsistent with {} observed page(s)",
      artifact.stop_evidence.page_index,
      artifact.pages.len()
    ));
  }
  let status_matches = matches!(
    (artifact.stop_evidence.reason, artifact.completeness_claim),
    (
      StopReason::NoProgressLimit,
      CompletenessClaim::CompleteByNoVisualProgressDown
        | CompletenessClaim::CompleteByNoVisualProgressUp
        | CompletenessClaim::CompleteByNoVisualProgress
    ) | (StopReason::ReachedBoundary, CompletenessClaim::CompleteByReachedBoundary)
      | (StopReason::MaxPages, CompletenessClaim::PartialMaxPages)
      | (StopReason::MaxScrolls | StopReason::MatchFound | StopReason::Error, CompletenessClaim::Unknown)
  );
  if !status_matches {
    return Err(format!(
      "stop reason {:?} and completeness_claim {:?} are inconsistent",
      artifact.stop_evidence.reason, artifact.completeness_claim
    ));
  }
  Ok(())
}

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

// TODO(scroll-scan-typed-composition): recipe-backed scan hooks were removed
// with JSON recipe execution. Reintroduce callback composition only for an
// owner-approved typed consumer; `auv-tracing` may instrument that flow but
// does not own or execute it.
pub async fn scan_window_region(options: ScanWindowRegionOptions) -> AuvResult<ScrollScanArtifact> {
  let scan_id = format!("scan_{}", now_millis());
  let execution = match LocalScanWindowRegionSource::new(&options) {
    Ok(mut source) => execute_scan_window_region(&mut source, options, scan_id),
    Err(error) => failed_scan_execution(options, scan_id, error),
  };
  emit_scan_execution(&execution);
  match execution.error {
    Some(error) => Err(error),
    None => Ok(execution.artifact),
  }
}

fn emit_scan_execution(execution: &ScanExecution) {
  if !execution.artifact.pages.is_empty() {
    emit_scroll_scan(&execution.artifact);
  }
  // NOTICE: A source failure before the first observed page has no scroll-scan
  // artifact. Its coverage still records the domain failure.
  let coverage = scan_coverage_from_artifact(&execution.artifact);
  emit_scan_coverage(&coverage);
}

trait ScanWindowRegionSource {
  fn observe(&mut self, page_index: usize, options: &ScanWindowRegionOptions) -> AuvResult<ScanSourcePage>;
  fn scroll(&mut self, options: &ScanWindowRegionOptions) -> AuvResult<()>;
}

struct ScanSourcePage {
  observations: Vec<CollectionObservation>,
  screenshot: RgbaImage,
}

struct LocalScanWindowRegionSource {
  session: auv_driver::LocalDriverSession,
  window: auv_driver::Window,
}

impl LocalScanWindowRegionSource {
  fn new(options: &ScanWindowRegionOptions) -> AuvResult<Self> {
    validate_scan_options(options)?;
    let session = auv_driver::open_local().map_err(|error| format!("failed to open local driver for scroll scan: {error}"))?;
    let mut selector = auv_driver::WindowSelector {
      main_visible: true,
      ..auv_driver::WindowSelector::default()
    };
    if let Some(application_id) = options.target.application_id.as_deref().filter(|value| !value.trim().is_empty()) {
      selector = selector.owned_by(auv_driver::App::bundle_id(application_id));
    }
    if let Some(title) = options.target.window_title.as_deref().filter(|value| !value.trim().is_empty()) {
      selector = selector.title_contains(title);
    }
    let window = session.window().resolve(selector).map_err(|error| format!("failed to resolve scroll-scan window: {error}"))?;
    Ok(Self { session, window })
  }
}

impl ScanWindowRegionSource for LocalScanWindowRegionSource {
  fn observe(&mut self, page_index: usize, options: &ScanWindowRegionOptions) -> AuvResult<ScanSourcePage> {
    let capture = self.session.window().capture(&self.window).map_err(|error| format!("failed to capture scroll-scan page: {error}"))?;
    let region = scan_ratio_rect(&options.target.region);
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture(&capture, region)
      .map_err(|error| format!("failed to recognize scroll-scan page: {error}"))?;
    let limit = usize::try_from(options.max_observations).map_err(|error| format!("invalid max_observations: {error}"))?;
    let observations = recognition
      .regions
      .iter()
      .filter(|region| region.confidence.unwrap_or_default() as f64 >= options.min_confidence)
      .filter(|region| !region.text.trim().is_empty())
      .take(limit)
      .enumerate()
      .map(|(item_index, region)| CollectionObservation {
        observation_id: format!("obs_{:04}_{:04}", page_index + 1, item_index + 1),
        page_index,
        raw_text: region.text.clone(),
        normalized_text_key: observation::normalize_observation_text(&region.text),
        bounds: ScanRect {
          x: region.bounds.origin.x.round() as i64,
          y: region.bounds.origin.y.round() as i64,
          width: region.bounds.size.width.round() as i64,
          height: region.bounds.size.height.round() as i64,
        },
        section_context: None,
        provider_score: region.confidence,
      })
      .collect();
    Ok(ScanSourcePage {
      observations,
      screenshot: capture.image,
    })
  }

  fn scroll(&mut self, options: &ScanWindowRegionOptions) -> AuvResult<()> {
    let point = scan_window_point(&self.window, &options.target.region);
    let scroll = scan_scroll_delta(&options.direction, options.scroll_amount)?;
    self
      .session
      .window()
      .scroll(
        &self.window,
        point,
        scroll,
        auv_driver::ScrollOptions {
          settle: Duration::from_millis(options.settle_ms),
          ..auv_driver::ScrollOptions::default()
        },
      )
      .map(|_| ())
      .map_err(|error| format!("failed to scroll window region: {error}"))
  }
}

#[derive(Default)]
struct ScanWindowRegionState {
  pages: Vec<ScanPageRecord>,
  observations: Vec<CollectionObservation>,
  known_observation_signatures: BTreeSet<String>,
  scroll_boundary_candidates: Vec<ScrollBoundaryCandidate>,
  warnings: Vec<String>,
  previous_screenshot: Option<RgbaImage>,
}

struct ScanExecution {
  artifact: ScrollScanArtifact,
  error: Option<String>,
}

fn execute_scan_window_region<S: ScanWindowRegionSource>(
  source: &mut S,
  options: ScanWindowRegionOptions,
  scan_id: String,
) -> ScanExecution {
  let mut state = ScanWindowRegionState::default();
  let mut consecutive_no_progress = 0;
  let mut final_decision = None;
  let mut scan_error = None;

  for (scroll_count, page_index) in (0..max_pages_for_policy(&options.stop_policy)).enumerate() {
    let page = match source.observe(page_index, &options) {
      Ok(page) => page,
      Err(error) => {
        scan_error = Some(error);
        final_decision = Some(error_stop_decision(page_index));
        break;
      }
    };
    let page_observations = page.observations;
    let new_observation_count = count_new_observations(&page_observations, &mut state.known_observation_signatures);
    if new_observation_count == 0 {
      consecutive_no_progress += 1;
    } else {
      consecutive_no_progress = 0;
    }
    let observation_count = page_observations.len();
    state.observations.extend(page_observations.clone());
    state.pages.push(ScanPageRecord {
      page_index,
      observation_count,
      new_observation_count,
    });
    let screenshot_diff_stability = state
      .previous_screenshot
      .as_ref()
      .map(|previous| screenshot_diff_stability_rgba(previous, &page.screenshot))
      .transpose()
      .unwrap_or_else(|error| {
        state.warnings.push(format!("failed to compare adjacent page screenshots for boundary evidence: {error}"));
        None
      });
    state.previous_screenshot = Some(page.screenshot);
    let scroll_boundary_candidate = scroll_boundary_candidate_for_progress(
      &options.direction,
      page_index,
      scroll_count,
      consecutive_no_progress,
      new_observation_count,
      &state.observations,
      screenshot_diff_stability.as_ref(),
    );
    if let Some(candidate) = scroll_boundary_candidate.clone() {
      state.scroll_boundary_candidates.push(candidate);
    }
    let progress = ScanProgress {
      page_index,
      scroll_count,
      consecutive_no_progress,
      new_observation_count,
      match_found: match_found_on_current_page(&options.stop_policy, &page_observations),
      scroll_boundary_candidate,
    };
    if let Some(decision) = evaluate_stop_policy(&options.stop_policy, &progress, &options.direction) {
      final_decision = Some(decision);
      break;
    }
    if let Err(error) = source.scroll(&options) {
      scan_error = Some(error);
      final_decision = Some(error_stop_decision(page_index));
      break;
    }
  }

  let final_decision = final_decision.unwrap_or_else(|| {
    stop_decision(
      StopReason::MaxPages,
      format!("reached max_pages={}", max_pages_for_policy(&options.stop_policy)),
      state.pages.last().map(|page| page.page_index).unwrap_or(0),
      CompletenessClaim::PartialMaxPages,
    )
  });
  if scan_error.is_some() {
    state.warnings.push("scan ended with an error; coverage is partial".to_string());
  }
  let artifact = state.into_artifact(scan_id, options.target, options.stop_policy, final_decision);
  ScanExecution {
    artifact,
    error: scan_error,
  }
}

impl ScanWindowRegionState {
  fn into_artifact(self, scan_id: String, target: ScanTarget, stop_policy: StopPolicy, final_decision: StopDecision) -> ScrollScanArtifact {
    let clusters = conservative_merge_observations(&self.observations);
    ScrollScanArtifact {
      scan_id,
      target,
      stop_policy,
      pages: self.pages,
      observations: self.observations,
      clusters,
      scroll_boundary_candidates: self.scroll_boundary_candidates,
      stop_evidence: final_decision.stop_evidence,
      completeness_claim: final_decision.completeness_claim,
      warnings: self.warnings,
    }
  }
}

fn failed_scan_execution(options: ScanWindowRegionOptions, scan_id: String, error: String) -> ScanExecution {
  ScanExecution {
    artifact: ScrollScanArtifact {
      scan_id,
      target: options.target,
      stop_policy: options.stop_policy,
      pages: Vec::new(),
      observations: Vec::new(),
      clusters: Vec::new(),
      scroll_boundary_candidates: Vec::new(),
      stop_evidence: StopEvidence {
        reason: StopReason::Error,
        message: error.clone(),
        page_index: 0,
      },
      completeness_claim: CompletenessClaim::Unknown,
      warnings: vec!["scan source failed before the first page; coverage is partial".to_string()],
    },
    error: Some(error),
  }
}

fn scan_coverage_from_artifact(artifact: &ScrollScanArtifact) -> CoverageView {
  let observation_pages = artifact
    .observations
    .iter()
    .map(|observation| (observation.observation_id.as_str(), observation.page_index))
    .collect::<BTreeMap<_, _>>();
  let entries = artifact
    .clusters
    .iter()
    .map(|cluster| {
      let last_page = cluster.observation_ids.iter().filter_map(|id| observation_pages.get(id.as_str())).copied().max().unwrap_or(0);
      CoverageEntry {
        track_id: cluster.cluster_id.clone(),
        last_seen_frame_id: scan_page_frame_id(&artifact.scan_id, last_page),
        observation_count: u32::try_from(cluster.observation_ids.len()).unwrap_or(u32::MAX),
      }
    })
    .collect();
  let open_uncertainty_codes = match artifact.completeness_claim {
    CompletenessClaim::PartialMaxPages => vec!["max_pages_reached".to_string()],
    CompletenessClaim::Unknown if artifact.stop_evidence.reason == StopReason::Error => vec!["scan_source_error".to_string()],
    CompletenessClaim::Unknown => vec!["scan_completeness_unknown".to_string()],
    _ => Vec::new(),
  };
  let negative_evidence = artifact
    .pages
    .iter()
    .filter(|page| page.page_index > 0 && page.observation_count == 0)
    .map(|page| NegativeEvidence {
      code: "no_new_observation".to_string(),
      after_frame_id: scan_page_frame_id(&artifact.scan_id, page.page_index),
    })
    .collect::<Vec<_>>();
  let claims_complete = matches!(
    artifact.completeness_claim,
    CompletenessClaim::CompleteByNoVisualProgressDown
      | CompletenessClaim::CompleteByNoVisualProgressUp
      | CompletenessClaim::CompleteByNoVisualProgress
      | CompletenessClaim::CompleteByReachedBoundary
  );
  if claims_complete && open_uncertainty_codes.is_empty() && negative_evidence.is_empty() {
    CoverageView::complete(entries)
  } else {
    let reason = if !open_uncertainty_codes.is_empty() || !negative_evidence.is_empty() {
      "open uncertainties or negative evidence remain".to_string()
    } else {
      artifact.stop_evidence.message.clone()
    };
    CoverageView::incomplete(entries, reason, open_uncertainty_codes, negative_evidence)
  }
}

fn scan_page_frame_id(scan_id: &str, page_index: usize) -> String {
  format!("{scan_id}:page:{:04}", page_index + 1)
}

fn max_pages_for_policy(policy: &StopPolicy) -> usize {
  match policy {
    StopPolicy::UntilEnd { max_pages, .. } | StopPolicy::UntilMatch { max_pages, .. } | StopPolicy::Bounded { max_pages, .. } => *max_pages,
  }
}

fn count_new_observations(observations: &[CollectionObservation], known_observation_signatures: &mut BTreeSet<String>) -> usize {
  observations.iter().filter(|observation| known_observation_signatures.insert(observation_signature(observation))).count()
}

fn observation_signature(observation: &CollectionObservation) -> String {
  if !observation.normalized_text_key.is_empty() {
    observation.normalized_text_key.clone()
  } else {
    format!("visual|x={}|w={}|h={}", observation.bounds.x, observation.bounds.width, observation.bounds.height)
  }
}

fn match_found_on_current_page(policy: &StopPolicy, observations: &[CollectionObservation]) -> bool {
  let StopPolicy::UntilMatch { query, .. } = policy else {
    return false;
  };
  let normalized_query = observation::normalize_observation_text(query);
  !normalized_query.is_empty() && observations.iter().any(|observation| observation.normalized_text_key.contains(&normalized_query))
}

fn validate_scan_options(options: &ScanWindowRegionOptions) -> AuvResult<()> {
  validate_scan_region(&options.target.region)?;
  if max_pages_for_policy(&options.stop_policy) == 0 {
    return Err("scroll-scan max_pages must be greater than zero".to_string());
  }
  if options.max_observations <= 0 {
    return Err("scroll-scan max_observations must be greater than zero".to_string());
  }
  if !options.min_confidence.is_finite() || !(0.0..=1.0).contains(&options.min_confidence) {
    return Err("scroll-scan min_confidence must be inside 0..=1".to_string());
  }
  let _ = scan_scroll_delta(&options.direction, options.scroll_amount)?;
  Ok(())
}

fn scan_ratio_rect(region: &ScanRegion) -> auv_driver::RatioRect {
  auv_driver::RatioRect::new(
    region.left_ratio,
    region.top_ratio,
    region.right_ratio - region.left_ratio,
    region.bottom_ratio - region.top_ratio,
  )
}

fn scan_window_point(window: &auv_driver::Window, region: &ScanRegion) -> auv_driver::WindowPoint {
  auv_driver::WindowPoint::new(
    window.frame.size.width * (region.left_ratio + region.right_ratio) / 2.0,
    window.frame.size.height * (region.top_ratio + region.bottom_ratio) / 2.0,
  )
}

fn scan_scroll_delta(direction: &str, amount: f64) -> AuvResult<auv_driver::Scroll> {
  if !amount.is_finite() || amount <= 0.0 {
    return Err("scroll-scan scroll_amount must be finite and greater than zero".to_string());
  }
  match direction.trim().to_ascii_lowercase().as_str() {
    "up" => Ok(auv_driver::Scroll::new(0.0, amount)),
    "down" => Ok(auv_driver::Scroll::new(0.0, -amount)),
    "left" => Ok(auv_driver::Scroll::new(amount, 0.0)),
    "right" => Ok(auv_driver::Scroll::new(-amount, 0.0)),
    _ => Err(format!("unsupported scroll-scan direction {direction:?}; expected up, down, left, or right")),
  }
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
  if progress.match_found {
    return Some(stop_decision(StopReason::MatchFound, "target match found", progress.page_index, CompletenessClaim::Unknown));
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
    StopPolicy::UntilMatch {
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

fn screenshot_diff_stability_rgba(previous: &RgbaImage, current: &RgbaImage) -> AuvResult<ScreenshotDiffStability> {
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
