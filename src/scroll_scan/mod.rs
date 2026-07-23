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

use observation::{
  build_page_observation_snapshot, conservative_merge_observations, observation_from_recognized_item, observation_from_row,
  should_merge_adjacent_observations, surface_nodes_from_observations,
};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::contract::{ObservationSnapshot, RecognitionResult, SurfaceNode};
use crate::model::{AuvResult, now_millis};
use crate::run_read::publish_scan_coverage;
use crate::runtime::Runtime;
use auv_scan::{CompletenessWire, CoverageEntryWire, NegativeEvidenceWire, SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire};
use auv_tracing::{Context, RunId, SpanId};
use image::RgbaImage;
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
pub async fn scan_window_region(runtime: &Runtime, options: ScanWindowRegionOptions) -> AuvResult<ScrollScanArtifact> {
  let _project_root = runtime.project_root();
  let context = Context::current();
  let scan_id = context.run_id().map_or_else(|| format!("scan_{}", now_millis()), |run_id| format!("scan_{run_id}"));
  let trace_ids = context.run_id().copied().zip(context.span_id().copied());
  let execution = match LocalScanWindowRegionSource::new(&options) {
    Ok(mut source) => execute_scan_window_region(&mut source, options, scan_id, trace_ids),
    Err(error) => failed_scan_execution(options, scan_id, error),
  };
  let coverage = scan_coverage_from_artifact(&execution.artifact);
  let _ = publish_scan_coverage(Some(&context), &coverage).await;
  match execution.error {
    Some(error) => Err(error),
    None => Ok(execution.artifact),
  }
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
      .map(|(item_index, region)| {
        let mut attributes = BTreeMap::from([
          ("item_index".to_string(), item_index.to_string()),
          ("recognition_source".to_string(), "ocr_text".to_string()),
          ("recognition_surface".to_string(), "region".to_string()),
          ("source".to_string(), "auv-driver.vision.ocr".to_string()),
        ]);
        if let Some(confidence) = region.confidence {
          attributes.insert("provider_score".to_string(), confidence.to_string());
        }
        CollectionObservation {
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
          source_artifacts: Vec::new(),
          attributes,
        }
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
  snapshots: Vec<ObservationSnapshot>,
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
  trace_ids: Option<(RunId, SpanId)>,
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
      observe_run_id: trace_ids.map(|(run_id, _)| run_id.to_string()),
      screenshot_artifact: None,
      observation_count,
      new_observation_count,
      summary: format!("observed {observation_count} OCR region(s); {new_observation_count} new scan signature(s)"),
    });
    if let Some((run_id, span_id)) = trace_ids {
      state.snapshots.push(build_page_observation_snapshot(
        run_id,
        span_id,
        page_index,
        &options.target,
        &page_observations,
        new_observation_count,
      ));
    }

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
      hook_stop_requested: false,
      match_found: match_found_on_current_page(&options.stop_policy, &page_observations),
      next_section_candidate: false,
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
  let artifact = state.into_artifact(scan_id, options.target, options.stop_policy, final_decision, trace_ids);
  ScanExecution {
    artifact,
    error: scan_error,
  }
}

impl ScanWindowRegionState {
  fn into_artifact(
    self,
    scan_id: String,
    target: ScanTarget,
    stop_policy: StopPolicy,
    final_decision: StopDecision,
    trace_ids: Option<(RunId, SpanId)>,
  ) -> ScrollScanArtifact {
    let clusters = conservative_merge_observations(&self.observations);
    let nodes = trace_ids.map(|(run_id, span_id)| surface_nodes_from_observations(run_id, span_id, &self.observations)).unwrap_or_default();
    ScrollScanArtifact {
      scan_id,
      target,
      stop_policy,
      pages: self.pages,
      observations: self.observations,
      nodes,
      snapshots: self.snapshots,
      clusters,
      section_candidates: Vec::new(),
      scroll_boundary_candidates: self.scroll_boundary_candidates,
      hook_decisions: Vec::new(),
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
      warnings: vec!["scan source failed before the first page; coverage is partial".to_string()],
    },
    error: Some(error),
  }
}

fn scan_coverage_from_artifact(artifact: &ScrollScanArtifact) -> ScanCoverageWire {
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
      CoverageEntryWire {
        track_id: cluster.cluster_id.clone(),
        last_seen_frame_id: scan_page_frame_id(&artifact.scan_id, last_page),
        observation_count: u32::try_from(cluster.observation_ids.len()).unwrap_or(u32::MAX),
      }
    })
    .collect();
  let open_uncertainty_codes = match artifact.completeness_claim {
    CompletenessClaim::PartialMaxPages => vec!["max_pages_reached".to_string()],
    CompletenessClaim::PartialMaxDuration => vec!["max_duration_reached".to_string()],
    CompletenessClaim::PartialUnstableContent => vec!["unstable_content".to_string()],
    CompletenessClaim::PartialNextSectionCandidate => vec!["next_section_candidate".to_string()],
    CompletenessClaim::Unknown if artifact.stop_evidence.reason == StopReason::Error => vec!["scan_source_error".to_string()],
    CompletenessClaim::Unknown => vec!["scan_completeness_unknown".to_string()],
    _ => Vec::new(),
  };
  let negative_evidence = artifact
    .pages
    .iter()
    .filter(|page| page.page_index > 0 && page.observation_count == 0)
    .map(|page| NegativeEvidenceWire {
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
  let completeness = if claims_complete && open_uncertainty_codes.is_empty() && negative_evidence.is_empty() {
    CompletenessWire::Complete
  } else {
    CompletenessWire::Incomplete {
      reason: if !open_uncertainty_codes.is_empty() || !negative_evidence.is_empty() {
        "open uncertainties or negative evidence remain".to_string()
      } else {
        artifact.stop_evidence.message.clone()
      },
    }
  };
  ScanCoverageWire {
    schema_version: SCAN_COVERAGE_SCHEMA_VERSION.to_string(),
    entries,
    open_uncertainty_codes,
    negative_evidence,
    completeness,
  }
}

fn scan_page_frame_id(scan_id: &str, page_index: usize) -> String {
  format!("{scan_id}:page:{:04}", page_index + 1)
}

fn max_pages_for_policy(policy: &StopPolicy) -> usize {
  match policy {
    StopPolicy::UntilEnd { max_pages, .. }
    | StopPolicy::UntilNextSection { max_pages, .. }
    | StopPolicy::UntilMatch { max_pages, .. }
    | StopPolicy::Bounded { max_pages, .. } => *max_pages,
  }
}

fn count_new_observations(observations: &[CollectionObservation], known_observation_signatures: &mut BTreeSet<String>) -> usize {
  observations.iter().filter(|observation| known_observation_signatures.insert(observation_signature(observation))).count()
}

fn observation_signature(observation: &CollectionObservation) -> String {
  if !observation.normalized_text_key.is_empty() {
    observation.normalized_text_key.clone()
  } else {
    let source = observation.attributes.get("source").map(String::as_str).unwrap_or("unknown");
    format!("visual:{source}|x={}|w={}|h={}", observation.bounds.x, observation.bounds.width, observation.bounds.height)
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
  let region = &options.target.region;
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

#[cfg(test)]
mod tests {
  use std::collections::VecDeque;

  use image::Rgba;

  use super::*;

  struct ScanSource {
    pages: VecDeque<AuvResult<ScanSourcePage>>,
    scroll_calls: usize,
    scroll_error: Option<String>,
  }

  impl ScanSource {
    fn with_pages(pages: Vec<AuvResult<ScanSourcePage>>) -> Self {
      Self {
        pages: pages.into(),
        scroll_calls: 0,
        scroll_error: None,
      }
    }
  }

  impl ScanWindowRegionSource for ScanSource {
    fn observe(&mut self, _page_index: usize, _options: &ScanWindowRegionOptions) -> AuvResult<ScanSourcePage> {
      self.pages.pop_front().unwrap_or_else(|| Err("fixture exhausted".to_string()))
    }

    fn scroll(&mut self, _options: &ScanWindowRegionOptions) -> AuvResult<()> {
      self.scroll_calls += 1;
      self.scroll_error.take().map_or(Ok(()), Err)
    }
  }

  fn options(stop_policy: StopPolicy) -> ScanWindowRegionOptions {
    ScanWindowRegionOptions {
      target: ScanTarget {
        application_id: Some("com.example.fixture".to_string()),
        window_title: Some("Fixture".to_string()),
        region: ScanRegion {
          left_ratio: 0.0,
          top_ratio: 0.0,
          right_ratio: 1.0,
          bottom_ratio: 1.0,
        },
      },
      stop_policy,
      direction: "down".to_string(),
      scroll_amount: 40.0,
      settle_ms: 0,
      min_confidence: 0.0,
      max_observations: 16,
    }
  }

  fn page(page_index: usize, text: &str, color: [u8; 4]) -> ScanSourcePage {
    ScanSourcePage {
      observations: vec![CollectionObservation {
        observation_id: format!("obs_{:04}_0001", page_index + 1),
        page_index,
        raw_text: text.to_string(),
        normalized_text_key: observation::normalize_observation_text(text),
        bounds: ScanRect {
          x: 10,
          y: 20,
          width: 100,
          height: 24,
        },
        section_context: None,
        source_artifacts: Vec::new(),
        attributes: BTreeMap::from([("source".to_string(), "auv-driver.vision.ocr".to_string())]),
      }],
      screenshot: RgbaImage::from_pixel(8, 8, Rgba(color)),
    }
  }

  fn empty_page(color: [u8; 4]) -> ScanSourcePage {
    ScanSourcePage {
      observations: Vec::new(),
      screenshot: RgbaImage::from_pixel(8, 8, Rgba(color)),
    }
  }

  #[test]
  fn scan_execution_produces_domain_pages_clusters_and_coverage() {
    let mut source = ScanSource::with_pages(vec![
      Ok(page(0, "Alpha", [0, 0, 0, 255])),
      Ok(page(1, "Beta", [255, 255, 255, 255])),
    ]);
    let execution = execute_scan_window_region(
      &mut source,
      options(StopPolicy::Bounded {
        max_pages: 2,
        max_scrolls: 5,
      }),
      "scan_fixture".to_string(),
      None,
    );

    assert!(execution.error.is_none());
    assert_eq!(source.scroll_calls, 1);
    assert_eq!(execution.artifact.pages.len(), 2);
    assert_eq!(execution.artifact.observations.len(), 2);
    assert_eq!(execution.artifact.clusters.len(), 2);
    assert_eq!(execution.artifact.stop_evidence.reason, StopReason::MaxPages);
    assert_eq!(execution.artifact.completeness_claim, CompletenessClaim::PartialMaxPages);

    let coverage = scan_coverage_from_artifact(&execution.artifact);
    assert_eq!(coverage.schema_version, SCAN_COVERAGE_SCHEMA_VERSION);
    assert_eq!(coverage.entries.len(), 2);
    assert_eq!(coverage.entries[1].last_seen_frame_id, "scan_fixture:page:0002");
    assert_eq!(coverage.open_uncertainty_codes, vec!["max_pages_reached"]);
    assert!(matches!(coverage.completeness, CompletenessWire::Incomplete { .. }));
  }

  #[test]
  fn scan_execution_preserves_partial_domain_evidence_on_source_error() {
    let mut source = ScanSource::with_pages(vec![
      Ok(page(0, "Alpha", [0, 0, 0, 255])),
      Err("fixture observation failed".to_string()),
    ]);
    let execution = execute_scan_window_region(
      &mut source,
      options(StopPolicy::Bounded {
        max_pages: 3,
        max_scrolls: 5,
      }),
      "scan_partial".to_string(),
      None,
    );

    assert_eq!(execution.error.as_deref(), Some("fixture observation failed"));
    assert_eq!(source.scroll_calls, 1);
    assert_eq!(execution.artifact.pages.len(), 1);
    assert_eq!(execution.artifact.observations.len(), 1);
    assert_eq!(execution.artifact.stop_evidence.reason, StopReason::Error);
    assert_eq!(execution.artifact.completeness_claim, CompletenessClaim::Unknown);

    let coverage = scan_coverage_from_artifact(&execution.artifact);
    assert_eq!(coverage.entries.len(), 1);
    assert_eq!(coverage.open_uncertainty_codes, vec!["scan_source_error"]);
    assert!(matches!(coverage.completeness, CompletenessWire::Incomplete { .. }));
  }

  #[test]
  fn scan_execution_keeps_repeated_observations_as_positive_coverage() {
    let mut source = ScanSource::with_pages(vec![
      Ok(page(0, "Repeat", [24, 48, 72, 255])),
      Ok(page(1, "Repeat", [24, 48, 72, 255])),
    ]);
    let execution = execute_scan_window_region(
      &mut source,
      options(StopPolicy::UntilEnd {
        max_pages: 5,
        max_scrolls: 5,
        no_progress_limit: 2,
      }),
      "scan_boundary".to_string(),
      None,
    );

    assert!(execution.error.is_none());
    assert_eq!(execution.artifact.stop_evidence.reason, StopReason::ReachedBoundary);
    assert_eq!(execution.artifact.completeness_claim, CompletenessClaim::CompleteByReachedBoundary);
    let coverage = scan_coverage_from_artifact(&execution.artifact);
    assert!(coverage.negative_evidence.is_empty());
    assert_eq!(coverage.entries[0].observation_count, 2);
    assert_eq!(coverage.completeness, CompletenessWire::Complete);
  }

  #[test]
  fn scan_execution_marks_empty_page_as_blocking_negative_evidence() {
    let mut source = ScanSource::with_pages(vec![
      Ok(page(0, "Alpha", [24, 48, 72, 255])),
      Ok(empty_page([24, 48, 72, 255])),
    ]);
    let execution = execute_scan_window_region(
      &mut source,
      options(StopPolicy::UntilEnd {
        max_pages: 5,
        max_scrolls: 5,
        no_progress_limit: 2,
      }),
      "scan_empty_page".to_string(),
      None,
    );

    assert!(execution.error.is_none());
    assert_eq!(execution.artifact.stop_evidence.reason, StopReason::ReachedBoundary);
    let coverage = scan_coverage_from_artifact(&execution.artifact);
    assert_eq!(coverage.negative_evidence.len(), 1);
    assert_eq!(coverage.negative_evidence[0].after_frame_id, "scan_empty_page:page:0002");
    assert!(matches!(coverage.completeness, CompletenessWire::Incomplete { .. }));
  }

  #[test]
  fn scan_execution_returns_scroll_interruption_after_observed_page() {
    let mut source = ScanSource::with_pages(vec![Ok(page(0, "Alpha", [0, 0, 0, 255]))]);
    source.scroll_error = Some("fixture scroll failed".to_string());
    let execution = execute_scan_window_region(
      &mut source,
      options(StopPolicy::Bounded {
        max_pages: 2,
        max_scrolls: 5,
      }),
      "scan_scroll_error".to_string(),
      None,
    );

    assert_eq!(execution.error.as_deref(), Some("fixture scroll failed"));
    assert_eq!(execution.artifact.pages.len(), 1);
    assert_eq!(execution.artifact.stop_evidence.reason, StopReason::Error);
  }

  #[tokio::test]
  async fn scan_window_region_reaches_the_direct_window_backend() {
    let project_root = std::env::temp_dir().join(format!("auv-scroll-scan-direct-backend-{}", std::process::id()));
    let runtime = Runtime::new(project_root);
    let result = scan_window_region(
      &runtime,
      ScanWindowRegionOptions {
        target: ScanTarget {
          application_id: Some("com.example.auv-task22-missing".to_string()),
          window_title: None,
          region: ScanRegion {
            left_ratio: 0.0,
            top_ratio: 0.0,
            right_ratio: 1.0,
            bottom_ratio: 1.0,
          },
        },
        stop_policy: StopPolicy::Bounded {
          max_pages: 1,
          max_scrolls: 0,
        },
        direction: "down".to_string(),
        scroll_amount: 40.0,
        settle_ms: 0,
        min_confidence: 0.0,
        max_observations: 16,
      },
    )
    .await;
    let error = result.expect_err("missing window must remain a source error");

    assert!(!error.contains("requires a typed window region observation API"), "synthetic scan error escaped: {error}");
  }
}
