use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{RecognitionResult, RecognitionSource, RecognitionSurface, RecognizedItem};
use crate::model::{
  AuvResult, DisturbanceClass, ExecutionTarget, InvokeRequest, InvokeResult, RunStatus,
};

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct StructuredHookDecisionSignal {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  hook_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  stage: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  page_index: Option<usize>,
  action: HookAction,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  reason: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  annotations: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  adjusted_region: Option<ScanRegion>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  adjusted_scroll: Option<ScanHookAdjustedScroll>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  retry_policy: Option<ScanHookRetryPolicy>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  evidence: Vec<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScrollScanArtifact {
  pub scan_id: String,
  pub target: ScanTarget,
  pub stop_policy: StopPolicy,
  pub pages: Vec<ScanPageRecord>,
  pub observations: Vec<CollectionObservation>,
  pub clusters: Vec<ObservationCluster>,
  pub section_candidates: Vec<SectionCandidate>,
  pub scroll_boundary_candidates: Vec<ScrollBoundaryCandidate>,
  pub hook_decisions: Vec<HookDecisionRecord>,
  pub stop_evidence: StopEvidence,
  pub completeness_claim: CompletenessClaim,
  pub warnings: Vec<String>,
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
  pub per_page_after_observe_recipe: Option<String>,
  pub per_list_item_candidate_recipe: Option<String>,
  pub on_stop_candidate_recipe: Option<String>,
}

pub fn scan_window_region(
  runtime: &crate::runtime::Runtime,
  options: ScanWindowRegionOptions,
) -> AuvResult<crate::trace::RunId> {
  let mut run = runtime.start_run(crate::recording::RunSpec::new(
    crate::trace::RunType::Execute,
    "auv.scan.window_region",
  ))?;
  let root = run.root_span();

  match scan_window_region_into_run(runtime, &mut run, &root, options) {
    Ok(summary) => runtime.finish_run(
      run,
      crate::recording::RunFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some(summary),
        failure: None,
      },
    ),
    Err(error) => {
      let finish_result = runtime.finish_run(
        run,
        crate::recording::RunFinish {
          status_code: crate::trace::TraceStatusCode::Error,
          summary: Some(format!("Window region scan failed: {error}")),
          failure: Some(error.clone()),
        },
      );
      match finish_result {
        Ok(_) => Err(error),
        Err(finish_error) => Err(format!(
          "{error}; additionally failed to persist failed scan run: {finish_error}"
        )),
      }
    }
  }
}

fn scan_window_region_into_run(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  options: ScanWindowRegionOptions,
) -> AuvResult<String> {
  let mut state = ScanWindowRegionState::default();
  let mut scroll_count = 0;
  let mut consecutive_no_progress = 0;
  let mut final_decision = None;
  let mut scan_error = None;

  for page_index in 0..max_pages_for_policy(&options.stop_policy) {
    let page_result = scan_window_region_page(runtime, run, root, page_index, &options, &mut state);
    let page_outcome = match page_result {
      Ok(page_outcome) => page_outcome,
      Err(error) => {
        scan_error = Some(error);
        final_decision = Some(error_stop_decision(page_index));
        break;
      }
    };
    let new_observation_count = page_outcome.new_observation_count;

    if new_observation_count == 0 {
      consecutive_no_progress += 1;
    } else {
      consecutive_no_progress = 0;
    }

    // Boundary evidence is still incomplete. We now distinguish raw
    // no-progress heuristics from adjacent-page repeated row-band overlap, but
    // downward scans still need stronger bottom detection and upward scans
    // still need stronger top detection. Future layers should add screenshot
    // diff stability, scrollbar/thumb geometry, AX scroll values, or explicit
    // driver-level scroll-effect evidence. Sectioned lists also need
    // middleware that can detect separators, sticky headers, or section
    // boundary regions during the scroll loop so a scan can stop at, enter, or
    // report section transitions deliberately.
    // MaaFW reference: Context::wait_freezes and ActionHelper compare image
    // stability around an action, while Pipeline roi/target references keep
    // image evidence tied to a node result; see:
    // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/source/MaaFramework/Task/Context.cpp
    // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/source/MaaFramework/Task/Component/ActionHelper.cpp
    let scroll_boundary_candidate = scroll_boundary_candidate_for_progress(
      &options.direction,
      page_index,
      scroll_count,
      consecutive_no_progress,
      new_observation_count,
      &state.observations,
    );
    if let Some(candidate) = scroll_boundary_candidate.clone() {
      state.scroll_boundary_candidates.push(candidate);
    }
    let progress = ScanProgress {
      page_index,
      scroll_count,
      consecutive_no_progress,
      new_observation_count,
      hook_stop_requested: state
        .hook_decisions
        .last()
        .is_some_and(|decision| decision.action == HookAction::Stop),
      match_found: match_found_on_current_page(&options.stop_policy, &page_outcome.observations),
      next_section_candidate: state
        .hook_decisions
        .last()
        .is_some_and(|decision| decision.action == HookAction::Stop)
        && matches!(options.stop_policy, StopPolicy::UntilNextSection { .. }),
      scroll_boundary_candidate,
    };

    if let Some(mut decision) = evaluate_stop_policy(&options.stop_policy, &progress) {
      let stop_hook_result = run_optional_scan_hook(
        runtime,
        run,
        root,
        options.on_stop_candidate_recipe.as_deref(),
        "on_stop_candidate",
        page_index,
        &options,
        Some(&decision.stop_evidence),
      );
      let stop_hook_decision = match stop_hook_result {
        Ok(decision) => decision,
        Err(error) => {
          scan_error = Some(error);
          final_decision = Some(error_stop_decision(page_index));
          break;
        }
      };

      if let Some(hook_decision) = stop_hook_decision {
        if let Err(error) = validate_scan_loop_hook_decision(&hook_decision) {
          state.hook_decisions.push(hook_decision);
          scan_error = Some(error);
          final_decision = Some(error_stop_decision(page_index));
          break;
        }
        if hook_decision.action == HookAction::Continue {
          if let Some(hard_cap_decision) =
            hard_cap_stop_decision_for_policy(&options.stop_policy, &progress)
          {
            state.warnings.push(format!(
              "stop candidate {:?} coincided with hard cap {:?}; ignoring hook continue request",
              decision.stop_evidence.reason, hard_cap_decision.stop_evidence.reason
            ));
            state.hook_decisions.push(hook_decision);
            final_decision = Some(hard_cap_decision);
            break;
          } else {
            state.warnings.push(format!(
              "stop candidate {:?} was inspected by hook and scan continued",
              decision.stop_evidence.reason
            ));
            state.hook_decisions.push(hook_decision);
          }
        } else {
          if hook_decision.action == HookAction::Stop {
            decision = stop_decision(
              StopReason::HookRequestedStop,
              hook_decision.reason.clone(),
              page_index,
              CompletenessClaim::Unknown,
            );
          }
          state.hook_decisions.push(hook_decision);
          final_decision = Some(decision);
          break;
        }
      } else {
        final_decision = Some(decision);
        break;
      }
    }

    match invoke_scan_command(
      runtime,
      run,
      root,
      scroll_request(&options),
      "scroll window region",
    ) {
      Ok(_) => {
        scroll_count += 1;
      }
      Err(error) => {
        scan_error = Some(error);
        final_decision = Some(error_stop_decision(page_index));
        break;
      }
    }
  }

  let final_decision = final_decision.unwrap_or_else(|| {
    stop_decision(
      StopReason::MaxPages,
      format!(
        "reached max_pages={}",
        max_pages_for_policy(&options.stop_policy)
      ),
      state.pages.last().map(|page| page.page_index).unwrap_or(0),
      CompletenessClaim::PartialMaxPages,
    )
  });
  if scan_error.is_some() {
    state
      .warnings
      .push("scan ended with an error; artifact is partial".to_string());
  }
  let artifact = state.into_artifact(
    run.id().to_string(),
    options.target,
    options.stop_policy,
    final_decision,
  );
  if let Err(stage_error) = stage_scan_artifact(runtime, run, root, &artifact) {
    if let Some(error) = scan_error {
      return Err(format!(
        "{error}; additionally failed to stage partial scroll-scan artifact: {stage_error}"
      ));
    }
    return Err(stage_error);
  }
  if let Some(error) = scan_error {
    return Err(error);
  }

  Ok(format!(
    "Scanned {} page(s), captured {} observation(s), formed {} cluster(s).",
    artifact.pages.len(),
    artifact.observations.len(),
    artifact.clusters.len()
  ))
}

pub fn observations_from_observe_json(
  page_index: usize,
  raw: &str,
  source_artifact: PathBuf,
) -> AuvResult<Vec<CollectionObservation>> {
  let value: Value =
    serde_json::from_str(raw).map_err(|error| format!("malformed observe JSON: {error}"))?;
  if let Some(recognition) = recognition_result_from_value(&value) {
    return Ok(observations_from_recognition_result(
      page_index,
      &recognition,
      &source_artifact,
    ));
  }
  let rows = value
    .get("item_candidates")
    .and_then(Value::as_array)
    .filter(|candidates| !candidates.is_empty())
    .or_else(|| value.get("rows").and_then(Value::as_array))
    .ok_or_else(|| "malformed observe JSON: missing rows array".to_string())?;

  rows
    .iter()
    .enumerate()
    .map(|(row_index, row)| observation_from_row(page_index, row_index, row, &source_artifact))
    .collect()
}

fn recognition_result_from_value(value: &Value) -> Option<RecognitionResult> {
  serde_json::from_value(value.clone()).ok()
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
    .map(|(item_index, item)| {
      observation_from_recognized_item(page_index, item_index, item, recognition, source_artifact)
    })
    .collect()
}

#[derive(Default)]
struct ScanWindowRegionState {
  pages: Vec<ScanPageRecord>,
  observations: Vec<CollectionObservation>,
  known_observation_signatures: BTreeSet<String>,
  scroll_boundary_candidates: Vec<ScrollBoundaryCandidate>,
  hook_decisions: Vec<HookDecisionRecord>,
  warnings: Vec<String>,
}

struct PageScanOutcome {
  new_observation_count: usize,
  observations: Vec<CollectionObservation>,
}

impl ScanWindowRegionState {
  fn into_artifact(
    self,
    scan_id: String,
    target: ScanTarget,
    stop_policy: StopPolicy,
    final_decision: StopDecision,
  ) -> ScrollScanArtifact {
    let clusters = conservative_merge_observations(&self.observations);
    ScrollScanArtifact {
      scan_id,
      target,
      stop_policy,
      pages: self.pages,
      observations: self.observations,
      clusters,
      section_candidates: Vec::new(),
      scroll_boundary_candidates: self.scroll_boundary_candidates,
      hook_decisions: self.hook_decisions,
      stop_evidence: final_decision.stop_evidence,
      completeness_claim: final_decision.completeness_claim,
      warnings: self.warnings,
    }
  }
}

fn scan_window_region_page(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  page_index: usize,
  options: &ScanWindowRegionOptions,
  state: &mut ScanWindowRegionState,
) -> AuvResult<PageScanOutcome> {
  let observe_result = invoke_scan_command(
    runtime,
    run,
    root,
    observe_request(options, page_index),
    "observe window region",
  )?;
  let screenshot_artifact = first_artifact_with_extension(&observe_result, "png");
  let source_artifact = screenshot_artifact
    .clone()
    .unwrap_or_else(|| first_artifact_with_extension(&observe_result, "json").unwrap_or_default());
  let mut page_observations =
    observations_from_first_json_artifact(page_index, &observe_result, source_artifact)?;
  enrich_list_item_observations_with_crops(
    runtime,
    run,
    root,
    page_index,
    screenshot_artifact.as_deref(),
    &options,
    &mut page_observations,
  )?;
  run_list_item_candidate_hooks(
    runtime,
    run,
    root,
    options.per_list_item_candidate_recipe.as_deref(),
    options,
    &page_observations,
    state,
  )?;
  let new_observation_count =
    count_new_observations(&page_observations, &mut state.known_observation_signatures);
  let observation_count = page_observations.len();
  state.observations.extend(page_observations.clone());

  state.pages.push(ScanPageRecord {
    page_index,
    observe_run_id: None,
    screenshot_artifact,
    observation_count,
    new_observation_count,
    summary: format!(
      "observed {observation_count} row(s); {new_observation_count} new page-local signature(s); observe command recorded inside the scan run"
    ),
  });

  if let Some(decision) = run_optional_scan_hook(
    runtime,
    run,
    root,
    options.per_page_after_observe_recipe.as_deref(),
    "per_page_after_observe",
    page_index,
    options,
    None,
  )? {
    let validation = validate_scan_loop_hook_decision(&decision);
    state.hook_decisions.push(decision);
    validation?;
  }

  Ok(PageScanOutcome {
    new_observation_count,
    observations: page_observations,
  })
}

pub fn evaluate_stop_policy(policy: &StopPolicy, progress: &ScanProgress) -> Option<StopDecision> {
  if progress.hook_stop_requested {
    return Some(stop_decision(
      StopReason::HookRequestedStop,
      "scan hook requested stop",
      progress.page_index,
      CompletenessClaim::Unknown,
    ));
  }
  if progress.match_found {
    return Some(stop_decision(
      StopReason::MatchFound,
      "target match found",
      progress.page_index,
      CompletenessClaim::Unknown,
    ));
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
    } => bounded_or_no_progress_stop(*max_pages, *max_scrolls, *no_progress_limit, progress),
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
  progress: &ScanProgress,
) -> Option<StopDecision> {
  if progress.consecutive_no_progress >= no_progress_limit {
    return Some(stop_decision(
      StopReason::NoProgressLimit,
      format!("reached no_progress_limit={no_progress_limit}"),
      progress.page_index,
      // TODO: Back this completeness claim with explicit boundary evidence.
      // "Complete by no visual progress" is only an evidence claim today, not
      // proof that the application has no hidden content. Split the future
      // evidence into direction-aware top/bottom claims so upward and downward
      // scans do not both collapse into generic no-progress.
      CompletenessClaim::CompleteByNoVisualProgress,
    ));
  }
  bounded_stop(max_pages, max_scrolls, progress)
}

fn bounded_stop(
  max_pages: usize,
  max_scrolls: usize,
  progress: &ScanProgress,
) -> Option<StopDecision> {
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

fn hard_cap_stop_decision_for_policy(
  policy: &StopPolicy,
  progress: &ScanProgress,
) -> Option<StopDecision> {
  match policy {
    StopPolicy::UntilEnd {
      max_pages,
      max_scrolls,
      ..
    }
    | StopPolicy::UntilNextSection {
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

fn stop_decision(
  reason: StopReason,
  message: impl Into<String>,
  page_index: usize,
  completeness_claim: CompletenessClaim,
) -> StopDecision {
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
  stop_decision(
    StopReason::Error,
    "scan stopped because an orchestration step failed",
    page_index,
    CompletenessClaim::Unknown,
  )
}

fn scroll_boundary_candidate_for_progress(
  direction: &str,
  page_index: usize,
  scroll_count: usize,
  consecutive_no_progress: usize,
  new_observation_count: usize,
  observations: &[CollectionObservation],
) -> Option<ScrollBoundaryCandidate> {
  if page_index == 0 || scroll_count == 0 || new_observation_count > 0 {
    return None;
  }
  let normalized_direction = direction.trim().to_ascii_lowercase();
  let boundary = scroll_boundary_for_direction(&normalized_direction)?;
  let repeated_overlap_count = repeated_row_band_overlap_count(page_index, observations);
  let (basis, confidence) = if repeated_overlap_count >= 2 {
    ("repeated_row_band_overlap", "corroborated")
  } else {
    ("no_new_observations_after_scroll", "heuristic")
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

fn repeated_row_band_overlap_count(
  page_index: usize,
  observations: &[CollectionObservation],
) -> usize {
  if page_index == 0 {
    return 0;
  }
  let previous_page = page_index - 1;
  let previous = observations
    .iter()
    .filter(|observation| observation.page_index == previous_page)
    .collect::<Vec<_>>();
  let current = observations
    .iter()
    .filter(|observation| observation.page_index == page_index)
    .collect::<Vec<_>>();
  let mut matched_previous = BTreeSet::new();
  let mut overlap_count = 0;

  for observation in current {
    if let Some((previous_index, _)) =
      previous
        .iter()
        .enumerate()
        .find(|(previous_index, candidate)| {
          !matched_previous.contains(previous_index)
            && repeated_row_band_overlap(candidate, observation)
        })
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
  rect_overlap_ratio(
    left.bounds.x,
    left.bounds.width,
    right.bounds.x,
    right.bounds.width,
  ) >= 0.5
    && rect_overlap_ratio(
      left.bounds.y,
      left.bounds.height,
      right.bounds.y,
      right.bounds.height,
    ) >= 0.6
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

const SCAN_HOOK_DECISION_SIGNAL: &str = "last.scan.hook.decision";
const SCAN_HOOK_ACTION_SIGNAL: &str = "last.scan.hook.action";
const SCAN_HOOK_REASON_SIGNAL: &str = "last.scan.hook.reason";

pub fn hook_decision_from_variables(
  hook_name: &str,
  page_index: usize,
  variables: &BTreeMap<String, String>,
) -> AuvResult<Option<HookDecisionRecord>> {
  if let Some(raw) = variables.get(SCAN_HOOK_DECISION_SIGNAL) {
    return parse_structured_hook_decision_signal(hook_name, page_index, raw).map(Some);
  }

  let Some(action) = variables.get(SCAN_HOOK_ACTION_SIGNAL) else {
    return Ok(None);
  };
  let action = parse_hook_action(action)?;
  let reason = variables
    .get(SCAN_HOOK_REASON_SIGNAL)
    .cloned()
    .unwrap_or_else(|| "hook did not provide a reason".to_string());
  Ok(Some(base_hook_decision_record(
    hook_name, page_index, action, reason,
  )))
}

fn parse_structured_hook_decision_signal(
  hook_name: &str,
  page_index: usize,
  raw: &str,
) -> AuvResult<HookDecisionRecord> {
  let signal: StructuredHookDecisionSignal = serde_json::from_str(raw).map_err(|error| {
    format!(
      "invalid structured scan hook decision in {}: {error}",
      SCAN_HOOK_DECISION_SIGNAL
    )
  })?;
  if let Some(signal_hook_name) = signal.hook_name.as_deref()
    && signal_hook_name != hook_name
  {
    return Err(format!(
      "structured scan hook decision hook_name {:?} does not match expected {:?}",
      signal_hook_name, hook_name
    ));
  }
  if let Some(signal_stage) = signal.stage.as_deref()
    && signal_stage != hook_name
  {
    return Err(format!(
      "structured scan hook decision stage {:?} does not match expected {:?}",
      signal_stage, hook_name
    ));
  }
  if let Some(signal_page_index) = signal.page_index
    && signal_page_index != page_index
  {
    return Err(format!(
      "structured scan hook decision page_index {} does not match expected {}",
      signal_page_index, page_index
    ));
  }
  let mut decision = base_hook_decision_record(
    hook_name,
    page_index,
    signal.action,
    signal
      .reason
      .unwrap_or_else(|| "hook did not provide a reason".to_string()),
  );
  decision.annotations = signal.annotations;
  decision.adjusted_region = signal.adjusted_region;
  decision.adjusted_scroll = signal.adjusted_scroll;
  decision.retry_policy = signal.retry_policy;
  decision.evidence = signal.evidence;
  Ok(decision)
}

fn base_hook_decision_record(
  hook_name: &str,
  page_index: usize,
  action: HookAction,
  reason: String,
) -> HookDecisionRecord {
  HookDecisionRecord {
    hook_name: hook_name.to_string(),
    page_index,
    item_index: None,
    row_candidate_index: None,
    action,
    reason,
    annotations: Vec::new(),
    adjusted_region: None,
    adjusted_scroll: None,
    retry_policy: None,
    evidence: Vec::new(),
  }
}

fn validate_scan_loop_hook_decision(decision: &HookDecisionRecord) -> AuvResult<()> {
  match decision.action {
    HookAction::Continue | HookAction::Stop => Ok(()),
    HookAction::RetryObserve
    | HookAction::AdjustRegion
    | HookAction::AdjustScroll
    | HookAction::Annotate => Err(format!(
      "scan hook action {} is parsed but not implemented by scan_window_region yet",
      hook_action_name(decision.action)
    )),
  }
}

fn hook_action_name(action: HookAction) -> &'static str {
  match action {
    HookAction::Continue => "continue",
    HookAction::Stop => "stop",
    HookAction::RetryObserve => "retry_observe",
    HookAction::AdjustRegion => "adjust_region",
    HookAction::AdjustScroll => "adjust_scroll",
    HookAction::Annotate => "annotate",
  }
}

fn parse_hook_action(raw: &str) -> AuvResult<HookAction> {
  match raw.trim() {
    "continue" => Ok(HookAction::Continue),
    "stop" => Ok(HookAction::Stop),
    "retry_observe" => Ok(HookAction::RetryObserve),
    "adjust_region" => Ok(HookAction::AdjustRegion),
    "adjust_scroll" => Ok(HookAction::AdjustScroll),
    "annotate" => Ok(HookAction::Annotate),
    other => Err(format!(
      "invalid scan hook action {other:?}; expected continue, stop, retry_observe, adjust_region, adjust_scroll, or annotate"
    )),
  }
}

pub fn normalize_observation_text(raw: &str) -> String {
  raw
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .trim()
    .to_lowercase()
}

pub fn conservative_merge_observations(
  observations: &[CollectionObservation],
) -> Vec<ObservationCluster> {
  let mut clusters: Vec<ObservationCluster> = Vec::new();
  let mut assigned = vec![false; observations.len()];

  for (index, observation) in observations.iter().enumerate() {
    if assigned[index] {
      continue;
    }

    let mut ids = vec![observation.observation_id.clone()];
    assigned[index] = true;
    let mut merge_reason = "single_observation".to_string();
    let mut confidence = 1.0;

    for (candidate_index, candidate) in observations.iter().enumerate().skip(index + 1) {
      if assigned[candidate_index] {
        continue;
      }
      if should_merge_adjacent_observations(observation, candidate) {
        ids.push(candidate.observation_id.clone());
        assigned[candidate_index] = true;
        merge_reason = "same_text_adjacent_page_near_y".to_string();
        confidence = 0.72;
      }
    }

    clusters.push(ObservationCluster {
      cluster_id: format!("cluster_{:04}", clusters.len() + 1),
      observation_ids: ids,
      representative_text: observation.raw_text.clone(),
      merge_reason,
      confidence,
    });
  }

  clusters
}

// TODO: Revisit merge identity after scroll-boundary evidence and row-local
// image hashes exist. This first rule is intentionally conservative and only
// merges adjacent-page overlap with nearly identical y positions.
fn should_merge_adjacent_observations(
  left: &CollectionObservation,
  right: &CollectionObservation,
) -> bool {
  if left.normalized_text_key.is_empty() || left.normalized_text_key != right.normalized_text_key {
    return false;
  }
  if left.section_context != right.section_context {
    return false;
  }
  if left.page_index.abs_diff(right.page_index) != 1 {
    return false;
  }
  (left.bounds.y - right.bounds.y).abs() <= 8
}

fn observation_from_row(
  page_index: usize,
  row_index: usize,
  row: &Value,
  source_artifact: &Path,
) -> AuvResult<CollectionObservation> {
  let raw_text = row
    .get("text")
    .and_then(Value::as_str)
    .ok_or_else(|| format!("malformed observe JSON: row {row_index} missing text string"))?
    .to_string();
  let bounds = row
    .get("bounds")
    .ok_or_else(|| format!("malformed observe JSON: row {row_index} missing bounds object"))?;

  Ok(CollectionObservation {
    observation_id: format!("obs_{:04}_{:04}", page_index + 1, row_index + 1),
    page_index,
    raw_text: raw_text.clone(),
    normalized_text_key: normalize_observation_text(&raw_text),
    bounds: ScanRect {
      x: json_i64(bounds, "x", row_index)?,
      y: json_i64(bounds, "y", row_index)?,
      width: json_i64(bounds, "width", row_index)?,
      height: json_i64(bounds, "height", row_index)?,
    },
    section_context: None,
    source_artifacts: vec![source_artifact.to_path_buf()],
    attributes: observation_attributes_from_row(row),
  })
}

fn observation_from_recognized_item(
  page_index: usize,
  item_index: usize,
  item: &RecognizedItem,
  recognition: &RecognitionResult,
  source_artifact: &Path,
) -> CollectionObservation {
  let raw_text = recognized_item_text(item);
  CollectionObservation {
    observation_id: format!("obs_{:04}_{:04}", page_index + 1, item_index + 1),
    page_index,
    raw_text: raw_text.clone(),
    normalized_text_key: normalize_observation_text(&raw_text),
    bounds: ScanRect {
      x: item.box_.x,
      y: item.box_.y,
      width: item.box_.width,
      height: item.box_.height,
    },
    section_context: None,
    source_artifacts: vec![source_artifact.to_path_buf()],
    attributes: observation_attributes_from_recognized_item(item_index, item, recognition),
  }
}

fn observation_attributes_from_row(row: &Value) -> BTreeMap<String, String> {
  let mut attributes = BTreeMap::new();
  if let Some(source) = row.get("source").and_then(Value::as_str) {
    attributes.insert("source".to_string(), source.to_string());
  }
  if let Some(row_index) = row.get("row_index").and_then(Value::as_u64) {
    attributes.insert("row_index".to_string(), row_index.to_string());
  }
  if let Some(item_index) = row.get("item_index").and_then(Value::as_u64) {
    attributes.insert("item_index".to_string(), item_index.to_string());
  }
  if let Some(row_candidate_index) = row.get("row_candidate_index").and_then(Value::as_u64) {
    attributes.insert(
      "row_candidate_index".to_string(),
      row_candidate_index.to_string(),
    );
  }
  if let Some(role) = row.get("segmented_region_role").and_then(Value::as_str) {
    attributes.insert("segmented_region_role".to_string(), role.to_string());
  }
  if let Some(reason) = row.get("filter_reason").and_then(Value::as_str) {
    attributes.insert("filter_reason".to_string(), reason.to_string());
  }
  if let Some(fragments) = row.get("text_fragments").and_then(Value::as_array) {
    let text = fragments
      .iter()
      .filter_map(Value::as_str)
      .collect::<Vec<_>>()
      .join(" | ");
    if !text.is_empty() {
      attributes.insert("text_fragments".to_string(), text);
    }
  }
  attributes
}

fn observation_attributes_from_recognized_item(
  item_index: usize,
  item: &RecognizedItem,
  recognition: &RecognitionResult,
) -> BTreeMap<String, String> {
  let mut attributes = BTreeMap::new();
  attributes.insert("item_index".to_string(), item_index.to_string());
  attributes.insert(
    "recognition_id".to_string(),
    recognition.recognition_id.clone(),
  );
  attributes.insert("recognized_item_id".to_string(), item.item_id.clone());
  attributes.insert(
    "recognition_source".to_string(),
    recognition_source_name(recognition.source).to_string(),
  );
  attributes.insert(
    "recognition_surface".to_string(),
    recognition_surface_name(recognition.scope.surface).to_string(),
  );
  attributes.insert("recognized_item_kind".to_string(), item.kind.clone());
  if let Some(source) = item.detail.get("source").and_then(Value::as_str) {
    attributes.insert("source".to_string(), source.to_string());
  } else {
    attributes.insert(
      "source".to_string(),
      format!(
        "recognition:{}",
        recognition_source_name(recognition.source)
      ),
    );
  }
  if let Some(row_index) = item.detail.get("row_index").and_then(Value::as_u64) {
    attributes.insert("row_index".to_string(), row_index.to_string());
    attributes.insert("row_candidate_index".to_string(), row_index.to_string());
  }
  if let Some(text_fragments) = recognized_item_text_fragments(item) {
    if !text_fragments.is_empty() {
      attributes.insert("text_fragments".to_string(), text_fragments.join(" | "));
    }
  }
  attributes
}

fn recognized_item_text(item: &RecognizedItem) -> String {
  item
    .text
    .as_deref()
    .filter(|text| !text.trim().is_empty())
    .map(str::to_string)
    .or_else(|| recognized_item_text_fragments(item).map(|fragments| fragments.join(" | ")))
    .unwrap_or_default()
}

fn recognized_item_text_fragments(item: &RecognizedItem) -> Option<Vec<String>> {
  let fragments = item
    .detail
    .get("text_fragments")
    .and_then(Value::as_array)?
    .iter()
    .filter_map(Value::as_str)
    .map(str::trim)
    .filter(|fragment| !fragment.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>();
  if fragments.is_empty() {
    None
  } else {
    Some(fragments)
  }
}

fn recognition_source_name(source: RecognitionSource) -> &'static str {
  match source {
    RecognitionSource::OcrText => "ocr_text",
    RecognitionSource::OcrRow => "ocr_row",
    RecognitionSource::VisualRow => "visual_row",
    RecognitionSource::SegmentedRegion => "segmented_region",
    RecognitionSource::IconMatch => "icon_match",
    RecognitionSource::Custom => "custom",
  }
}

fn recognition_surface_name(surface: RecognitionSurface) -> &'static str {
  match surface {
    RecognitionSurface::Screen => "screen",
    RecognitionSurface::Display => "display",
    RecognitionSurface::Window => "window",
    RecognitionSurface::Region => "region",
  }
}

fn json_i64(bounds: &Value, key: &str, row_index: usize) -> AuvResult<i64> {
  bounds.get(key).and_then(Value::as_i64).ok_or_else(|| {
    format!("malformed observe JSON: row {row_index} bounds.{key} must be an integer")
  })
}

fn max_pages_for_policy(policy: &StopPolicy) -> usize {
  match policy {
    StopPolicy::UntilEnd { max_pages, .. }
    | StopPolicy::UntilNextSection { max_pages, .. }
    | StopPolicy::UntilMatch { max_pages, .. }
    | StopPolicy::Bounded { max_pages, .. } => *max_pages,
  }
}

fn observe_request(options: &ScanWindowRegionOptions, page_index: usize) -> InvokeRequest {
  let mut inputs = region_inputs(&options.target);
  inputs.insert(
    "label".to_string(),
    format!("scan-page-{:04}", page_index + 1),
  );
  inputs.insert(
    "min_confidence".to_string(),
    format!("{:.3}", options.min_confidence),
  );
  inputs.insert(
    "max_observations".to_string(),
    options.max_observations.to_string(),
  );
  InvokeRequest {
    command_id: "debug.observeWindowRegion".to_string(),
    target: ExecutionTarget {
      application_id: options.target.application_id.clone(),
    },
    inputs,
  }
}

fn scroll_request(options: &ScanWindowRegionOptions) -> InvokeRequest {
  let mut inputs = region_inputs(&options.target);
  // TODO: Preserve scroll-effect evidence in the scroll command result. The
  // scan loop needs to know whether an up/down scroll actually moved the
  // viewport, hit the top, hit the bottom, or crossed a section boundary; the
  // current request only sends direction/amount and treats command success as
  // movement.
  inputs.insert("direction".to_string(), options.direction.clone());
  inputs.insert(
    "amount".to_string(),
    format!("{:.3}", options.scroll_amount),
  );
  inputs.insert("settle_ms".to_string(), options.settle_ms.to_string());
  InvokeRequest {
    command_id: "debug.scrollWindowRegion".to_string(),
    target: ExecutionTarget {
      application_id: options.target.application_id.clone(),
    },
    inputs,
  }
}

fn region_inputs(target: &ScanTarget) -> BTreeMap<String, String> {
  let mut inputs = BTreeMap::from([
    (
      "region_left_ratio".to_string(),
      target.region.left_ratio.to_string(),
    ),
    (
      "region_top_ratio".to_string(),
      target.region.top_ratio.to_string(),
    ),
    (
      "region_right_ratio".to_string(),
      target.region.right_ratio.to_string(),
    ),
    (
      "region_bottom_ratio".to_string(),
      target.region.bottom_ratio.to_string(),
    ),
  ]);
  if let Some(title) = &target.window_title {
    inputs.insert("title".to_string(), title.clone());
  }
  inputs
}

fn invoke_scan_command(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  request: InvokeRequest,
  label: &str,
) -> AuvResult<InvokeResult> {
  let result = runtime.invoke_in_span(run, root, request)?;
  if result.status == RunStatus::Completed {
    Ok(result)
  } else {
    Err(
      result
        .failure_message
        .unwrap_or_else(|| format!("{label} command failed")),
    )
  }
}

fn observations_from_first_json_artifact(
  page_index: usize,
  result: &InvokeResult,
  source_artifact: PathBuf,
) -> AuvResult<Vec<CollectionObservation>> {
  let json_paths = result
    .artifact_paths
    .iter()
    .filter(|path| {
      path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
    })
    .cloned()
    .collect::<Vec<_>>();
  if json_paths.is_empty() {
    return Err("observe window region did not produce a JSON artifact".to_string());
  }
  observations_from_json_artifacts(page_index, &json_paths, &source_artifact)
}

fn observations_from_json_artifacts(
  page_index: usize,
  json_paths: &[PathBuf],
  source_artifact: &Path,
) -> AuvResult<Vec<CollectionObservation>> {
  let mut raw_json_artifacts = Vec::with_capacity(json_paths.len());
  for path in json_paths {
    let raw = fs::read_to_string(path)
      .map_err(|error| format!("failed to read observe JSON {}: {error}", path.display()))?;
    let value: Value =
      serde_json::from_str(&raw).map_err(|error| format!("malformed observe JSON: {error}"))?;
    if recognition_result_from_value(&value).is_some() {
      return observations_from_observe_json(page_index, &raw, source_artifact.to_path_buf());
    }
    raw_json_artifacts.push(raw);
  }

  let mut last_error = None;
  for raw in raw_json_artifacts {
    match observations_from_observe_json(page_index, &raw, source_artifact.to_path_buf()) {
      Ok(observations) => return Ok(observations),
      Err(error) => last_error = Some(error),
    }
  }

  Err(last_error.unwrap_or_else(|| {
    "observe window region did not produce a parseable recognition or rows JSON artifact"
      .to_string()
  }))
}

fn first_artifact_with_extension(result: &InvokeResult, extension: &str) -> Option<PathBuf> {
  result
    .artifact_paths
    .iter()
    .find(|path| {
      path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    })
    .cloned()
}

fn count_new_observations(
  observations: &[CollectionObservation],
  known_observation_signatures: &mut BTreeSet<String>,
) -> usize {
  observations
    .iter()
    .filter(|observation| known_observation_signatures.insert(observation_signature(observation)))
    .count()
}

fn observation_signature(observation: &CollectionObservation) -> String {
  let identity = if observation.normalized_text_key.is_empty() {
    // TODO: Replace visual-only geometry identity with row OCR, AX ids, or
    // local image hashes. Source + viewport geometry is only a bounded progress
    // signal and can misidentify virtualized or repeated rows.
    format!(
      "visual:{}:{}",
      observation
        .attributes
        .get("source")
        .map(String::as_str)
        .unwrap_or("unknown"),
      observation
        .attributes
        .get("row_index")
        .map(String::as_str)
        .unwrap_or("?")
    )
  } else {
    observation.normalized_text_key.clone()
  };
  format!(
    "{}|x={}|y={}|w={}|h={}",
    identity,
    observation.bounds.x,
    observation.bounds.y,
    observation.bounds.width,
    observation.bounds.height
  )
}

fn match_found_on_current_page(
  policy: &StopPolicy,
  current_page_observations: &[CollectionObservation],
) -> bool {
  let StopPolicy::UntilMatch { query, .. } = policy else {
    return false;
  };
  let normalized_query = normalize_observation_text(query);
  !normalized_query.is_empty()
    && current_page_observations
      .iter()
      .any(|observation| observation.normalized_text_key.contains(&normalized_query))
}

fn list_item_candidate_hook_overrides(
  options: &ScanWindowRegionOptions,
  item: &CollectionObservation,
) -> BTreeMap<String, String> {
  // TODO: Replace these scalar scan.item.* overrides with one typed hook
  // context object. We keep the scalar surface small for now because recipe
  // step args are still rendered as strings, but list-item parsing needs the
  // crop artifact, OCR fragments, bounds, and provenance as one coherent value.
  // MaaFW reference: custom_recognition_param/custom_action_param are JSON
  // values, and custom recognition returns a box plus JSON detail:
  // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/source/MaaFramework/Resource/PipelineTypes.h
  // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/source/MaaFramework/Task/Component/CustomRecognition.cpp
  // TODO: Include section-boundary and segmentation context here once region
  // segmentation can identify list bodies, sticky headers, separators, and
  // section-local item ranges. That middleware is the core difference between
  // "scroll until match" and "scroll within this section until match".
  let mut overrides = BTreeMap::from([
    (
      "scan.hook.name".to_string(),
      "per_list_item_candidate".to_string(),
    ),
    (
      "scan.hook.stage".to_string(),
      "per_list_item_candidate".to_string(),
    ),
    ("scan.page_index".to_string(), item.page_index.to_string()),
    ("scan.direction".to_string(), options.direction.clone()),
    (
      "scan.target.application_id".to_string(),
      options.target.application_id.clone().unwrap_or_default(),
    ),
    (
      "scan.item.index".to_string(),
      item
        .attributes
        .get("item_index")
        .cloned()
        .unwrap_or_else(|| "0".to_string()),
    ),
    (
      "scan.item.row_candidate_index".to_string(),
      item
        .attributes
        .get("row_candidate_index")
        .cloned()
        .unwrap_or_default(),
    ),
    ("scan.item.text".to_string(), item.raw_text.clone()),
    ("scan.item.bounds.x".to_string(), item.bounds.x.to_string()),
    ("scan.item.bounds.y".to_string(), item.bounds.y.to_string()),
    (
      "scan.item.bounds.width".to_string(),
      item.bounds.width.to_string(),
    ),
    (
      "scan.item.bounds.height".to_string(),
      item.bounds.height.to_string(),
    ),
  ]);
  for key in [
    "source",
    "filter_reason",
    "segmented_region_role",
    "text_fragments",
  ] {
    if let Some(value) = item.attributes.get(key) {
      overrides.insert(format!("scan.item.{key}"), value.clone());
    }
  }
  if let Some(source_artifact) = item.source_artifacts.first() {
    overrides.insert(
      "scan.item.source_artifact".to_string(),
      source_artifact.display().to_string(),
    );
  }
  if let Some(crop_artifact) = item.attributes.get("crop_artifact") {
    overrides.insert("scan.item.crop_artifact".to_string(), crop_artifact.clone());
  }
  if let Some(context_artifact) = item.attributes.get("context_artifact") {
    overrides.insert(
      "scan.item.context_artifact".to_string(),
      context_artifact.clone(),
    );
  }
  overrides
}

#[derive(Clone, Debug)]
struct ListItemCropOcrResult {
  crop_artifact: PathBuf,
  context_artifact: PathBuf,
  text_fragments: Vec<String>,
  strategy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ListItemCandidateContextArtifact {
  schema: String,
  observation_id: String,
  page_index: usize,
  raw_text: String,
  text_fragments: Vec<String>,
  bounds: ScanRect,
  attributes: BTreeMap<String, String>,
  source_artifacts: Vec<PathBuf>,
  crop_artifact: PathBuf,
  ocr_strategy: String,
}

impl ListItemCandidateContextArtifact {
  const SCHEMA: &'static str = "auv.scan.list_item_candidate_context.v1";
  const OCR_STRATEGY: &'static str = "crop_ocr";
}

fn enrich_list_item_observations_with_crops(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  page_index: usize,
  screenshot_artifact: Option<&Path>,
  options: &ScanWindowRegionOptions,
  observations: &mut [CollectionObservation],
) -> AuvResult<()> {
  let Some(screenshot_artifact) = screenshot_artifact else {
    return Ok(());
  };

  let mut crop_jobs = Vec::new();
  for (observation_index, observation) in observations.iter().enumerate() {
    let item_index = observation_item_index(observation);
    let crop_temp_path = crop_list_item_image(
      screenshot_artifact,
      &observation.bounds,
      &format!(
        "scan-page-{:04}-list-item-{:04}",
        page_index + 1,
        item_index + 1
      ),
    )?;
    crop_jobs.push((observation_index, item_index, crop_temp_path));
  }

  let text_results =
    ocr_list_item_crops_parallel(&crop_jobs, options.min_confidence, options.max_observations)?;

  for (observation_index, item_index, crop_temp_path) in crop_jobs {
    let observation = &mut observations[observation_index];
    let text_fragments = text_results
      .get(observation_index)
      .cloned()
      .unwrap_or_default();
    let crop_artifact = runtime.stage_artifact_file(
      run,
      root,
      "list-item-crop",
      &crop_temp_path,
      format!(
        "scan-page-{:04}-list-item-{:04}.png",
        page_index + 1,
        item_index + 1
      ),
      Some("Cropped screenshot for one list item candidate.".to_string()),
    )?;
    let context_temp_path =
      write_list_item_context_artifact(observation, &crop_artifact, &text_fragments)?;
    let context_artifact = runtime.stage_artifact_file(
      run,
      root,
      "list-item-context",
      &context_temp_path,
      format!(
        "scan-page-{:04}-list-item-{:04}-context.json",
        page_index + 1,
        item_index + 1
      ),
      Some("Typed context for one list item candidate.".to_string()),
    )?;
    let _ = fs::remove_file(&crop_temp_path);
    let _ = fs::remove_file(&context_temp_path);
    apply_list_item_crop_ocr_result(
      observation,
      ListItemCropOcrResult {
        crop_artifact,
        context_artifact,
        text_fragments,
        strategy: "crop_ocr".to_string(),
      },
    );
  }

  Ok(())
}

fn ocr_list_item_crops_parallel(
  crop_jobs: &[(usize, usize, PathBuf)],
  min_confidence: f64,
  max_observations: i64,
) -> AuvResult<Vec<Vec<String>>> {
  let mut handles = Vec::new();
  for (observation_index, _, crop_path) in crop_jobs {
    let crop_path = crop_path.clone();
    let observation_index = *observation_index;
    handles.push(std::thread::spawn(move || {
      crate::driver::ocr_text_fragments_in_image(&crop_path, min_confidence, max_observations)
        .map(|fragments| (observation_index, fragments))
    }));
  }

  let mut results = vec![Vec::new(); crop_jobs.len()];
  for handle in handles {
    let (observation_index, fragments) = handle
      .join()
      .map_err(|_| "list item crop OCR worker panicked".to_string())??;
    if let Some(slot) = results.get_mut(observation_index) {
      *slot = fragments;
    }
  }
  Ok(results)
}

fn observation_item_index(observation: &CollectionObservation) -> usize {
  observation
    .attributes
    .get("item_index")
    .and_then(|value| value.parse::<usize>().ok())
    .unwrap_or(0)
}

fn crop_list_item_image(source: &Path, bounds: &ScanRect, label: &str) -> AuvResult<PathBuf> {
  let image = image::open(source).map_err(|error| {
    format!(
      "failed to open list item source image {}: {error}",
      source.display()
    )
  })?;
  let image_width = image.width() as i64;
  let image_height = image.height() as i64;
  let crop = clamped_crop_rect(bounds, image_width, image_height)?;
  let cropped = image.crop_imm(
    crop.x as u32,
    crop.y as u32,
    crop.width as u32,
    crop.height as u32,
  );
  let path = std::env::temp_dir().join(format!(
    "auv-{}-{}-{}.png",
    sanitize_scan_artifact_component(label),
    std::process::id(),
    crate::model::now_millis()
  ));
  cropped
    .save(&path)
    .map_err(|error| format!("failed to write list item crop {}: {error}", path.display()))?;
  Ok(path)
}

fn clamped_crop_rect(
  bounds: &ScanRect,
  image_width: i64,
  image_height: i64,
) -> AuvResult<ScanRect> {
  if image_width <= 0 || image_height <= 0 {
    return Err(format!(
      "invalid source image dimensions {}x{} for list item crop",
      image_width, image_height
    ));
  }
  let x = bounds.x.clamp(0, image_width.saturating_sub(1));
  let y = bounds.y.clamp(0, image_height.saturating_sub(1));
  let max_x = (bounds.x + bounds.width).clamp(x + 1, image_width);
  let max_y = (bounds.y + bounds.height).clamp(y + 1, image_height);
  Ok(ScanRect {
    x,
    y,
    width: max_x - x,
    height: max_y - y,
  })
}

fn write_list_item_context_artifact(
  observation: &CollectionObservation,
  crop_artifact: &Path,
  text_fragments: &[String],
) -> AuvResult<PathBuf> {
  let path = std::env::temp_dir().join(format!(
    "auv-list-item-context-{}-{}-{}.json",
    sanitize_scan_artifact_component(&observation.observation_id),
    std::process::id(),
    crate::model::now_millis()
  ));
  let payload = build_list_item_context_payload(observation, crop_artifact, text_fragments);
  let rendered = serde_json::to_string_pretty(&payload)
    .map_err(|error| format!("failed to render list item context JSON: {error}"))?;
  fs::write(&path, format!("{rendered}\n")).map_err(|error| {
    format!(
      "failed to write list item context {}: {error}",
      path.display()
    )
  })?;
  Ok(path)
}

fn build_list_item_context_payload(
  observation: &CollectionObservation,
  crop_artifact: &Path,
  text_fragments: &[String],
) -> ListItemCandidateContextArtifact {
  let raw_text = joined_text_fragments(text_fragments);
  let mut attributes = observation.attributes.clone();
  if !raw_text.is_empty() {
    attributes.insert("text_fragments".to_string(), raw_text.clone());
  }
  attributes.insert(
    "ocr_strategy".to_string(),
    ListItemCandidateContextArtifact::OCR_STRATEGY.to_string(),
  );

  ListItemCandidateContextArtifact {
    schema: ListItemCandidateContextArtifact::SCHEMA.to_string(),
    observation_id: observation.observation_id.clone(),
    page_index: observation.page_index,
    raw_text,
    text_fragments: text_fragments.to_vec(),
    bounds: observation.bounds.clone(),
    attributes,
    source_artifacts: observation.source_artifacts.clone(),
    crop_artifact: crop_artifact.to_path_buf(),
    ocr_strategy: ListItemCandidateContextArtifact::OCR_STRATEGY.to_string(),
  }
}

fn apply_list_item_crop_ocr_result(
  observation: &mut CollectionObservation,
  result: ListItemCropOcrResult,
) {
  observation.attributes.insert(
    "crop_artifact".to_string(),
    result.crop_artifact.display().to_string(),
  );
  observation.attributes.insert(
    "context_artifact".to_string(),
    result.context_artifact.display().to_string(),
  );
  observation
    .attributes
    .insert("ocr_strategy".to_string(), result.strategy);
  if !result.text_fragments.is_empty() {
    let raw_text = joined_text_fragments(&result.text_fragments);
    observation.raw_text = raw_text.clone();
    observation.normalized_text_key = normalize_observation_text(&raw_text);
    observation
      .attributes
      .insert("text_fragments".to_string(), raw_text);
  }
}

fn joined_text_fragments(text_fragments: &[String]) -> String {
  text_fragments.join(" | ")
}

fn sanitize_scan_artifact_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
        character
      } else {
        '-'
      }
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();
  if sanitized.is_empty() {
    "item".to_string()
  } else {
    sanitized
  }
}

fn run_list_item_candidate_hooks(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  recipe: Option<&str>,
  options: &ScanWindowRegionOptions,
  items: &[CollectionObservation],
  state: &mut ScanWindowRegionState,
) -> AuvResult<()> {
  let Some(recipe) = recipe else {
    return Ok(());
  };
  let project_root = runtime.project_root();
  // TODO: Allow recipe manifests to declare inline hook/sub-recipe blocks.
  // This standalone recipe lookup exists because SkillCatalog currently resolves
  // only top-level recipe_id values from recipes/**/*.json, while scroll scan
  // needs a per-list-item hook that belongs conceptually to the parent workflow.
  // MaaFW reference: Context::run_task/run_recognition/run_action accept a JSON
  // pipeline_override, so a caller can synthesize or override nested pipeline
  // nodes without creating a separate resource file for every hook:
  // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/source/MaaFramework/Task/Context.cpp
  // /Users/neko/Git/github.com/MaaXYZ/MaaFramework/docs/zh_cn/3.1-任务流水线协议.md
  let catalog = crate::skill::SkillCatalog::discover(project_root)?;
  let entry = catalog.resolve(project_root, recipe)?;
  validate_scan_sub_recipe(&entry.manifest, "per_list_item_candidate")?;

  for item in items {
    let summary = crate::skill::run_skill_manifest_into_run(
      runtime,
      run,
      root,
      &entry.manifest,
      crate::skill::SkillRunOptions {
        dry_run: false,
        max_disturbance: Some(DisturbanceClass::None),
        overrides: list_item_candidate_hook_overrides(options, item),
        quiet: true,
      },
    )?;
    let Some(mut decision) = hook_decision_from_variables(
      "per_list_item_candidate",
      item.page_index,
      &summary.exported_variables,
    )?
    else {
      continue;
    };
    decision.item_index = item
      .attributes
      .get("item_index")
      .and_then(|value| value.parse::<usize>().ok());
    decision.row_candidate_index = item
      .attributes
      .get("row_candidate_index")
      .and_then(|value| value.parse::<usize>().ok());
    validate_list_item_candidate_hook_decision(&decision)?;
    let should_stop = decision.action == HookAction::Stop;
    state.hook_decisions.push(decision);
    if should_stop {
      break;
    }
  }

  Ok(())
}

fn validate_scan_sub_recipe(
  manifest: &crate::skill::SkillManifest,
  expected_stage: &str,
) -> AuvResult<()> {
  let invocation = &manifest.invocation;
  if invocation.kind != "sub_recipe"
    || invocation.host != "scroll_scan"
    || invocation.stage != expected_stage
  {
    return Err(format!(
      "recipe {} is not a scroll_scan sub_recipe for stage {expected_stage}",
      manifest.recipe_id
    ));
  }
  Ok(())
}

fn validate_list_item_candidate_hook_decision(decision: &HookDecisionRecord) -> AuvResult<()> {
  match decision.action {
    HookAction::Continue | HookAction::Stop | HookAction::Annotate => Ok(()),
    HookAction::RetryObserve | HookAction::AdjustRegion | HookAction::AdjustScroll => Err(format!(
      "list item hook action {} is parsed but not implemented by scan_window_region yet",
      hook_action_name(decision.action)
    )),
  }
}

fn run_optional_scan_hook(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  recipe: Option<&str>,
  hook_name: &str,
  page_index: usize,
  options: &ScanWindowRegionOptions,
  stop_evidence: Option<&StopEvidence>,
) -> AuvResult<Option<HookDecisionRecord>> {
  let Some(recipe) = recipe else {
    return Ok(None);
  };
  let project_root = runtime.project_root();
  let catalog = crate::skill::SkillCatalog::discover(project_root)?;
  let entry = catalog.resolve(project_root, recipe)?;
  let mut overrides = BTreeMap::from([
    ("scan.hook.name".to_string(), hook_name.to_string()),
    ("scan.page_index".to_string(), page_index.to_string()),
    ("scan.direction".to_string(), options.direction.clone()),
    (
      "scan.target.application_id".to_string(),
      options.target.application_id.clone().unwrap_or_default(),
    ),
  ]);
  if let Some(stop_evidence) = stop_evidence {
    overrides.insert(
      "scan.stop.reason".to_string(),
      format!("{:?}", stop_evidence.reason),
    );
    overrides.insert(
      "scan.stop.message".to_string(),
      stop_evidence.message.clone(),
    );
  }
  let summary = crate::skill::run_skill_manifest_into_run(
    runtime,
    run,
    root,
    &entry.manifest,
    crate::skill::SkillRunOptions {
      dry_run: false,
      max_disturbance: Some(DisturbanceClass::None),
      overrides,
      quiet: false,
    },
  )?;
  hook_decision_from_variables(hook_name, page_index, &summary.exported_variables)
}

fn stage_scan_artifact(
  runtime: &crate::runtime::Runtime,
  run: &mut crate::recording::RecordingRun,
  root: &crate::recording::SpanRef,
  artifact: &ScrollScanArtifact,
) -> AuvResult<PathBuf> {
  let temp_path = write_scan_artifact(artifact)?;
  let stage_result = runtime.stage_artifact_file(
    run,
    root,
    "scroll-scan",
    &temp_path,
    "scroll-scan.json",
    Some("Runtime window-region scroll scan artifact.".to_string()),
  );
  let cleanup_result = fs::remove_file(&temp_path);
  match (stage_result, cleanup_result) {
    (Ok(staged_path), Ok(())) => Ok(staged_path),
    (Ok(staged_path), Err(_)) => Ok(staged_path),
    (Err(error), Ok(())) | (Err(error), Err(_)) => Err(error),
  }
}

fn write_scan_artifact(artifact: &ScrollScanArtifact) -> AuvResult<PathBuf> {
  let path = std::env::temp_dir().join(format!(
    "auv-scroll-scan-{}-{}-{}.json",
    artifact.scan_id,
    std::process::id(),
    crate::model::now_millis()
  ));
  let rendered = serde_json::to_string_pretty(artifact)
    .map_err(|error| format!("failed to render scan artifact JSON: {error}"))?;
  fs::write(&path, format!("{rendered}\n"))
    .map_err(|error| format!("failed to write scan artifact {}: {error}", path.display()))?;
  Ok(path)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::sync::atomic::{AtomicU64, Ordering};

  static TEST_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);

  #[test]
  fn scan_artifact_serializes_completeness_and_observations() {
    let artifact = ScrollScanArtifact {
      scan_id: "scan_test".to_string(),
      target: ScanTarget {
        application_id: Some("com.example.App".to_string()),
        window_title: Some("Library".to_string()),
        region: ScanRegion {
          left_ratio: 0.1,
          top_ratio: 0.2,
          right_ratio: 0.9,
          bottom_ratio: 0.8,
        },
      },
      stop_policy: StopPolicy::Bounded {
        max_pages: 2,
        max_scrolls: 1,
      },
      pages: vec![ScanPageRecord {
        page_index: 0,
        observe_run_id: Some("run_observe".to_string()),
        screenshot_artifact: Some(PathBuf::from("artifacts/page.png")),
        observation_count: 1,
        new_observation_count: 1,
        summary: "observed 1 row".to_string(),
      }],
      observations: vec![CollectionObservation {
        observation_id: "obs_0001".to_string(),
        page_index: 0,
        raw_text: "Alpha".to_string(),
        normalized_text_key: "alpha".to_string(),
        bounds: ScanRect {
          x: 10,
          y: 20,
          width: 100,
          height: 30,
        },
        section_context: None,
        source_artifacts: vec![PathBuf::from("artifacts/page.png")],
        attributes: BTreeMap::new(),
      }],
      clusters: vec![ObservationCluster {
        cluster_id: "cluster_0001".to_string(),
        observation_ids: vec!["obs_0001".to_string()],
        representative_text: "Alpha".to_string(),
        merge_reason: "single_observation".to_string(),
        confidence: 1.0,
      }],
      section_candidates: Vec::new(),
      scroll_boundary_candidates: vec![ScrollBoundaryCandidate {
        page_index: 1,
        scroll_count: 1,
        direction: "down".to_string(),
        boundary: ScrollBoundary::Bottom,
        basis: "no_new_observations_after_scroll".to_string(),
        confidence: "heuristic".to_string(),
        consecutive_no_progress: 1,
      }],
      hook_decisions: Vec::new(),
      stop_evidence: StopEvidence {
        reason: StopReason::MaxPages,
        message: "reached max_pages=2".to_string(),
        page_index: 1,
      },
      completeness_claim: CompletenessClaim::PartialMaxPages,
      warnings: vec!["bounded scan".to_string()],
    };

    let rendered = serde_json::to_string_pretty(&artifact).expect("serialize");

    assert!(rendered.contains("\"completeness_claim\": \"partial_max_pages\""));
    assert!(rendered.contains("\"normalized_text_key\": \"alpha\""));
    assert!(rendered.contains("\"merge_reason\": \"single_observation\""));
    assert!(rendered.contains("\"scroll_boundary_candidates\""));
    assert!(rendered.contains("\"boundary\": \"bottom\""));
  }

  #[test]
  fn conservative_merge_keeps_same_text_on_same_page_separate() {
    let observations = vec![
      observation("obs_0001", 0, "Repeat", 10),
      observation("obs_0002", 0, "Repeat", 80),
    ];

    let clusters = conservative_merge_observations(&observations);

    assert_eq!(clusters.len(), 2);
    assert_eq!(clusters[0].merge_reason, "single_observation");
    assert_eq!(clusters[1].merge_reason, "single_observation");
  }

  #[test]
  fn conservative_merge_groups_same_text_on_adjacent_overlap_pages() {
    let observations = vec![
      observation("obs_0001", 0, "Repeat", 120),
      observation("obs_0002", 1, "Repeat", 118),
    ];

    let clusters = conservative_merge_observations(&observations);

    assert_eq!(clusters.len(), 1);
    assert_eq!(
      clusters[0].observation_ids,
      vec!["obs_0001".to_string(), "obs_0002".to_string()]
    );
    assert_eq!(clusters[0].merge_reason, "same_text_adjacent_page_near_y");
  }

  #[test]
  fn bounded_policy_stops_at_max_pages() {
    let decision = evaluate_stop_policy(
      &StopPolicy::Bounded {
        max_pages: 2,
        max_scrolls: 10,
      },
      &ScanProgress {
        page_index: 1,
        scroll_count: 1,
        consecutive_no_progress: 0,
        new_observation_count: 3,
        hook_stop_requested: false,
        match_found: false,
        next_section_candidate: false,
        scroll_boundary_candidate: None,
      },
    );

    assert_eq!(
      decision.expect("stop expected").completeness_claim,
      CompletenessClaim::PartialMaxPages
    );
  }

  #[test]
  fn until_end_policy_stops_after_no_progress_limit() {
    let decision = evaluate_stop_policy(
      &StopPolicy::UntilEnd {
        max_pages: 20,
        max_scrolls: 20,
        no_progress_limit: 2,
      },
      &ScanProgress {
        page_index: 3,
        scroll_count: 3,
        consecutive_no_progress: 2,
        new_observation_count: 0,
        hook_stop_requested: false,
        match_found: false,
        next_section_candidate: false,
        scroll_boundary_candidate: None,
      },
    );

    let decision = decision.expect("stop expected");
    assert_eq!(decision.stop_evidence.reason, StopReason::NoProgressLimit);
    assert_eq!(
      decision.completeness_claim,
      CompletenessClaim::CompleteByNoVisualProgress
    );
  }

  #[test]
  fn scroll_boundary_candidate_maps_direction_to_boundary() {
    let candidate =
      scroll_boundary_candidate_for_progress("up", 2, 2, 1, 0, &[]).expect("boundary candidate");

    assert_eq!(candidate.boundary, ScrollBoundary::Top);
    assert_eq!(candidate.direction, "up");
    assert_eq!(candidate.basis, "no_new_observations_after_scroll");
    assert_eq!(candidate.confidence, "heuristic");
  }

  #[test]
  fn scroll_boundary_candidate_requires_prior_scroll_and_no_new_observations() {
    assert!(scroll_boundary_candidate_for_progress("down", 0, 0, 0, 0, &[]).is_none());
    assert!(scroll_boundary_candidate_for_progress("down", 1, 0, 1, 0, &[]).is_none());
    assert!(scroll_boundary_candidate_for_progress("down", 1, 1, 0, 2, &[]).is_none());
  }

  #[test]
  fn scroll_boundary_candidate_uses_corroborated_basis_for_downward_repeated_row_overlap() {
    let observations = repeated_overlap_page_observations();

    let candidate = scroll_boundary_candidate_for_progress("down", 1, 1, 1, 0, &observations)
      .expect("boundary candidate");

    assert_eq!(candidate.boundary, ScrollBoundary::Bottom);
    assert_eq!(candidate.basis, "repeated_row_band_overlap");
    assert_eq!(candidate.confidence, "corroborated");
  }

  #[test]
  fn scroll_boundary_candidate_uses_corroborated_basis_for_upward_repeated_row_overlap() {
    let observations = repeated_overlap_page_observations();

    let candidate = scroll_boundary_candidate_for_progress("up", 1, 1, 1, 0, &observations)
      .expect("boundary candidate");

    assert_eq!(candidate.boundary, ScrollBoundary::Top);
    assert_eq!(candidate.basis, "repeated_row_band_overlap");
    assert_eq!(candidate.confidence, "corroborated");
  }

  #[test]
  fn scroll_boundary_candidate_keeps_heuristic_basis_for_single_repeated_row_overlap() {
    let observations = vec![
      observation("obs_0001", 0, "Repeat A", 120),
      observation("obs_0002", 1, "Repeat A", 118),
    ];

    let candidate = scroll_boundary_candidate_for_progress("down", 1, 1, 1, 0, &observations)
      .expect("boundary candidate");

    assert_eq!(candidate.basis, "no_new_observations_after_scroll");
    assert_eq!(candidate.confidence, "heuristic");
  }

  #[test]
  fn until_match_policy_stops_at_directional_boundary_candidate() {
    let decision = evaluate_stop_policy(
      &StopPolicy::UntilMatch {
        query: "needle".to_string(),
        max_pages: 20,
        max_scrolls: 20,
      },
      &ScanProgress {
        page_index: 2,
        scroll_count: 2,
        consecutive_no_progress: 1,
        new_observation_count: 0,
        hook_stop_requested: false,
        match_found: false,
        next_section_candidate: false,
        scroll_boundary_candidate: scroll_boundary_candidate_for_progress("down", 2, 2, 1, 0, &[]),
      },
    )
    .expect("boundary stop expected");

    assert_eq!(decision.stop_evidence.reason, StopReason::ReachedBoundary);
    assert_eq!(
      decision.completeness_claim,
      CompletenessClaim::CompleteByReachedBoundary
    );
    assert!(decision.stop_evidence.message.contains("bottom"));
    assert!(
      decision
        .stop_evidence
        .message
        .contains("no_new_observations_after_scroll")
    );
  }

  #[test]
  fn bounded_policy_ignores_directional_boundary_candidate() {
    let decision = evaluate_stop_policy(
      &StopPolicy::Bounded {
        max_pages: 20,
        max_scrolls: 20,
      },
      &ScanProgress {
        page_index: 2,
        scroll_count: 2,
        consecutive_no_progress: 1,
        new_observation_count: 0,
        hook_stop_requested: false,
        match_found: false,
        next_section_candidate: false,
        scroll_boundary_candidate: scroll_boundary_candidate_for_progress("down", 2, 2, 1, 0, &[]),
      },
    );

    assert!(decision.is_none());
  }

  #[test]
  fn hook_decision_parses_exported_recipe_variables() {
    let variables = BTreeMap::from([
      ("last.scan.hook.action".to_string(), "stop".to_string()),
      (
        "last.scan.hook.reason".to_string(),
        "next section".to_string(),
      ),
    ]);

    let decision = hook_decision_from_variables("per_page_after_observe", 3, &variables)
      .expect("decision should parse")
      .expect("decision should exist");

    assert_eq!(decision.action, HookAction::Stop);
    assert_eq!(decision.reason, "next section");
    assert_eq!(decision.page_index, 3);
    assert!(decision.annotations.is_empty());
    assert!(decision.evidence.is_empty());
  }

  #[test]
  fn hook_decision_rejects_unknown_action() {
    let variables = BTreeMap::from([("last.scan.hook.action".to_string(), "teleport".to_string())]);

    let error = hook_decision_from_variables("per_page_after_observe", 0, &variables)
      .expect_err("invalid action should fail");

    assert!(error.contains("invalid scan hook action"));
  }

  #[test]
  fn hook_decision_prefers_structured_signal_when_present() {
    let variables = BTreeMap::from([
      (
        "last.scan.hook.decision".to_string(),
        serde_json::json!({
          "hook_name": "per_page_after_observe",
          "page_index": 3,
          "action": "stop",
          "reason": "structured decision",
          "annotations": ["sticky header repeated"],
          "evidence": ["artifacts/page-0003-overlay.json"]
        })
        .to_string(),
      ),
      ("last.scan.hook.action".to_string(), "continue".to_string()),
      (
        "last.scan.hook.reason".to_string(),
        "scalar fallback should lose".to_string(),
      ),
    ]);

    let decision = hook_decision_from_variables("per_page_after_observe", 3, &variables)
      .expect("decision should parse")
      .expect("decision should exist");

    assert_eq!(decision.action, HookAction::Stop);
    assert_eq!(decision.reason, "structured decision");
    assert_eq!(
      decision.annotations,
      vec!["sticky header repeated".to_string()]
    );
    assert_eq!(
      decision.evidence,
      vec!["artifacts/page-0003-overlay.json".to_string()]
    );
  }

  #[test]
  fn hook_decision_rejects_mismatched_structured_page_index() {
    let variables = BTreeMap::from([(
      "last.scan.hook.decision".to_string(),
      serde_json::json!({
        "hook_name": "per_page_after_observe",
        "page_index": 4,
        "action": "stop",
        "reason": "wrong page"
      })
      .to_string(),
    )]);

    let error = hook_decision_from_variables("per_page_after_observe", 3, &variables)
      .expect_err("mismatched page index should fail");

    assert!(error.contains("page_index 4 does not match expected 3"));
  }

  #[test]
  fn count_new_observations_uses_position_signature_for_repeated_text() {
    let mut known_signatures = BTreeSet::new();
    let first = vec![observation("obs_0001", 0, "Repeat", 10)];
    let second = vec![observation("obs_0002", 1, "Repeat", 80)];

    assert_eq!(count_new_observations(&first, &mut known_signatures), 1);
    assert_eq!(count_new_observations(&second, &mut known_signatures), 1);
  }

  #[test]
  fn until_match_uses_current_page_observations_only() {
    let policy = StopPolicy::UntilMatch {
      query: "needle".to_string(),
      max_pages: 3,
      max_scrolls: 3,
    };
    let old_accumulated_observations = vec![observation("obs_0001", 0, "needle", 10)];
    let current_page_observations = vec![observation("obs_0002", 1, "other", 80)];

    assert!(
      old_accumulated_observations
        .iter()
        .any(|observation| observation.normalized_text_key.contains("needle"))
    );
    assert!(!match_found_on_current_page(
      &policy,
      &current_page_observations
    ));
  }

  #[test]
  fn scan_loop_rejects_unimplemented_hook_actions() {
    let decision = HookDecisionRecord {
      hook_name: "per_page_after_observe".to_string(),
      page_index: 0,
      item_index: None,
      row_candidate_index: None,
      action: HookAction::AdjustRegion,
      reason: "need a wider region".to_string(),
      annotations: Vec::new(),
      adjusted_region: None,
      adjusted_scroll: None,
      retry_policy: None,
      evidence: Vec::new(),
    };

    let error = validate_scan_loop_hook_decision(&decision).expect_err("action should fail");

    assert!(error.contains("parsed but not implemented by scan_window_region yet"));
    assert!(error.contains("adjust_region"));
  }

  #[test]
  fn stop_candidate_continue_hook_cannot_override_hard_caps() {
    let policy = StopPolicy::UntilMatch {
      query: "needle".to_string(),
      max_pages: 1,
      max_scrolls: 0,
    };
    let progress = ScanProgress {
      page_index: 0,
      scroll_count: 0,
      consecutive_no_progress: 0,
      new_observation_count: 1,
      hook_stop_requested: false,
      match_found: true,
      next_section_candidate: false,
      scroll_boundary_candidate: None,
    };

    let hard_cap =
      hard_cap_stop_decision_for_policy(&policy, &progress).expect("hard cap should win");

    assert_eq!(hard_cap.stop_evidence.reason, StopReason::MaxPages);
  }

  #[test]
  fn stop_candidate_continue_hook_allowed_before_hard_caps() {
    let policy = StopPolicy::UntilMatch {
      query: "needle".to_string(),
      max_pages: 3,
      max_scrolls: 2,
    };
    let progress = ScanProgress {
      page_index: 0,
      scroll_count: 0,
      consecutive_no_progress: 0,
      new_observation_count: 1,
      hook_stop_requested: false,
      match_found: true,
      next_section_candidate: false,
      scroll_boundary_candidate: None,
    };

    assert!(hard_cap_stop_decision_for_policy(&policy, &progress).is_none());
  }

  #[test]
  fn parse_observe_rows_json_returns_collection_observations() {
    let raw = r#"{
    "extractor": "ocr-row",
    "screenshot_path": "/tmp/page.png",
    "rows": [
      {
        "row_index": 0,
        "source": "visual-bands+ocr-text",
        "text": "Alpha",
        "text_fragments": ["Alpha"],
        "bounds": { "x": 1, "y": 2, "width": 30, "height": 10 },
        "peak_density": 0.42
      }
    ]
  }"#;

    let observations = observations_from_observe_json(0, raw, PathBuf::from("artifacts/page.png"))
      .expect("parse observations");

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].raw_text, "Alpha");
    assert_eq!(observations[0].normalized_text_key, "alpha");
    assert_eq!(
      observations[0].attributes.get("source").map(String::as_str),
      Some("visual-bands+ocr-text")
    );
    assert_eq!(
      observations[0]
        .attributes
        .get("text_fragments")
        .map(String::as_str),
      Some("Alpha")
    );
  }

  #[test]
  fn parse_observe_json_prefers_list_item_candidates_over_raw_rows() {
    let raw = r#"{
    "extractor": "ocr-row",
    "screenshot_path": "/tmp/page.png",
    "rows": [
      {
        "row_index": 0,
        "source": "visual-bands",
        "text": "",
        "text_fragments": [],
        "bounds": { "x": 10, "y": 20, "width": 400, "height": 160 }
      }
    ],
    "item_candidates": [
      {
        "item_index": 0,
        "row_candidate_index": 2,
        "source": "row_filter",
        "text": "Whisper of time",
        "text_fragments": ["Whisper of time"],
        "bounds": { "x": 100, "y": 220, "width": 600, "height": 84 }
      },
      {
        "item_index": 1,
        "row_candidate_index": 3,
        "source": "row_filter",
        "text": "万书隙",
        "text_fragments": ["万书隙"],
        "bounds": { "x": 100, "y": 348, "width": 600, "height": 86 }
      }
    ]
  }"#;

    let observations = observations_from_observe_json(0, raw, PathBuf::from("artifacts/page.png"))
      .expect("parse observations");

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].raw_text, "Whisper of time");
    assert_eq!(
      observations[0]
        .attributes
        .get("row_candidate_index")
        .map(String::as_str),
      Some("2")
    );
    assert_eq!(
      observations[0].attributes.get("source").map(String::as_str),
      Some("row_filter")
    );
  }

  #[test]
  fn parse_observe_json_prefers_recognition_result_filtered_items() {
    let raw = r#"{
    "recognition_id": "window_region_demo",
    "source": "visual_row",
    "scope": {
      "surface": "region",
      "display_ref": "display-1",
      "native_display_id": "69732928",
      "app_bundle_id": "com.tencent.QQMusicMac",
      "window_title": null,
      "window_number": 91,
      "region_hint": null,
      "capture_artifact": null,
      "capture_contract_artifact": null
    },
    "best": null,
    "filtered": [
      {
        "item_id": "row#1",
        "kind": "row",
        "box": { "x": 100, "y": 220, "width": 600, "height": 84 },
        "text": "Whisper of time",
        "provider_score": null,
        "detail": {
          "row_index": 2,
          "source": "row_filter",
          "text_fragments": ["Whisper of time"]
        }
      },
      {
        "item_id": "row#2",
        "kind": "row",
        "box": { "x": 100, "y": 348, "width": 600, "height": 86 },
        "text": "万书隙",
        "provider_score": null,
        "detail": {
          "row_index": 3,
          "source": "row_filter",
          "text_fragments": ["万书隙"]
        }
      }
    ],
    "all": [
      {
        "item_id": "row#0",
        "kind": "row",
        "box": { "x": 100, "y": 92, "width": 600, "height": 84 },
        "text": "Ignored",
        "provider_score": null,
        "detail": {
          "row_index": 1,
          "source": "visual-bands",
          "text_fragments": ["Ignored"]
        }
      }
    ],
    "detail": { "provider": "macos.row_detection" },
    "evidence": [],
    "known_limits": []
  }"#;

    let observations = observations_from_observe_json(0, raw, PathBuf::from("artifacts/page.png"))
      .expect("parse recognition observations");

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].raw_text, "Whisper of time");
    assert_eq!(
      observations[0]
        .attributes
        .get("recognition_id")
        .map(String::as_str),
      Some("window_region_demo")
    );
    assert_eq!(
      observations[0]
        .attributes
        .get("recognized_item_id")
        .map(String::as_str),
      Some("row#1")
    );
    assert_eq!(
      observations[0]
        .attributes
        .get("recognition_source")
        .map(String::as_str),
      Some("visual_row")
    );
    assert_eq!(
      observations[0]
        .attributes
        .get("recognition_surface")
        .map(String::as_str),
      Some("region")
    );
    assert_eq!(
      observations[0]
        .attributes
        .get("row_candidate_index")
        .map(String::as_str),
      Some("2")
    );
    assert_eq!(
      observations[0].attributes.get("source").map(String::as_str),
      Some("row_filter")
    );
  }

  #[test]
  fn observations_from_json_artifacts_prefers_recognition_result_over_legacy_rows() {
    let legacy_raw = r#"{
    "extractor": "ocr-row",
    "rows": [
      {
        "row_index": 0,
        "source": "visual-bands+ocr-text",
        "text": "Legacy Row",
        "text_fragments": ["Legacy Row"],
        "bounds": { "x": 1, "y": 2, "width": 30, "height": 10 }
      }
    ]
  }"#;
    let recognition_raw = r#"{
    "recognition_id": "window_region_demo",
    "source": "ocr_row",
    "scope": {
      "surface": "region",
      "display_ref": null,
      "native_display_id": null,
      "app_bundle_id": null,
      "window_title": null,
      "window_number": null,
      "region_hint": null,
      "capture_artifact": null,
      "capture_contract_artifact": null
    },
    "best": {
      "item_id": "row#1",
      "kind": "row",
      "box": { "x": 100, "y": 220, "width": 600, "height": 84 },
      "text": "Preferred Row",
      "provider_score": null,
      "detail": {
        "row_index": 2,
        "source": "row_filter",
        "text_fragments": ["Preferred Row"]
      }
    },
    "filtered": [
      {
        "item_id": "row#1",
        "kind": "row",
        "box": { "x": 100, "y": 220, "width": 600, "height": 84 },
        "text": "Preferred Row",
        "provider_score": null,
        "detail": {
          "row_index": 2,
          "source": "row_filter",
          "text_fragments": ["Preferred Row"]
        }
      }
    ],
    "all": [
      {
        "item_id": "row#1",
        "kind": "row",
        "box": { "x": 100, "y": 220, "width": 600, "height": 84 },
        "text": "Preferred Row",
        "provider_score": null,
        "detail": {
          "row_index": 2,
          "source": "row_filter",
          "text_fragments": ["Preferred Row"]
        }
      }
    ],
    "detail": { "provider": "macos.row_detection" },
    "evidence": [],
    "known_limits": []
  }"#;
    let legacy_path = write_temp_json_artifact("legacy", legacy_raw);
    let recognition_path = write_temp_json_artifact("recognition", recognition_raw);

    let observations = observations_from_json_artifacts(
      0,
      &[legacy_path.clone(), recognition_path.clone()],
      Path::new("artifacts/page.png"),
    )
    .expect("recognition should win over legacy rows");

    let _ = fs::remove_file(legacy_path);
    let _ = fs::remove_file(recognition_path);

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].raw_text, "Preferred Row");
    assert_eq!(
      observations[0]
        .attributes
        .get("recognized_item_id")
        .map(String::as_str),
      Some("row#1")
    );
  }

  #[test]
  fn count_new_observations_tracks_visual_only_rows_once() {
    let mut known = BTreeSet::new();
    let mut visual = observation("obs_0001_0001", 0, "", 120);
    visual
      .attributes
      .insert("source".to_string(), "visual-bands".to_string());
    visual
      .attributes
      .insert("row_index".to_string(), "0".to_string());

    assert_eq!(count_new_observations(&[visual.clone()], &mut known), 1);
    assert_eq!(count_new_observations(&[visual], &mut known), 0);
  }

  #[test]
  fn list_item_candidate_hook_overrides_include_outer_scan_context() {
    let mut item = observation("obs_0001_0001", 2, "Song A", 120);
    item
      .attributes
      .insert("item_index".to_string(), "4".to_string());
    item
      .attributes
      .insert("row_candidate_index".to_string(), "7".to_string());
    item
      .attributes
      .insert("source".to_string(), "row_filter".to_string());
    item.attributes.insert(
      "filter_reason".to_string(),
      "accepted_repeating_row_geometry".to_string(),
    );
    item
      .source_artifacts
      .push(PathBuf::from("artifacts/page.png"));
    let options = ScanWindowRegionOptions {
      target: ScanTarget {
        application_id: Some("com.example.App".to_string()),
        window_title: None,
        region: ScanRegion {
          left_ratio: 0.2,
          top_ratio: 0.3,
          right_ratio: 0.9,
          bottom_ratio: 0.8,
        },
      },
      stop_policy: StopPolicy::Bounded {
        max_pages: 1,
        max_scrolls: 0,
      },
      direction: "down".to_string(),
      scroll_amount: 40.0,
      settle_ms: 250,
      min_confidence: 0.0,
      max_observations: 128,
      per_page_after_observe_recipe: None,
      per_list_item_candidate_recipe: Some(
        "scan.fixture.list_item_candidate_continue.v0".to_string(),
      ),
      on_stop_candidate_recipe: None,
    };

    let overrides = list_item_candidate_hook_overrides(&options, &item);

    assert_eq!(
      overrides.get("scan.hook.stage").map(String::as_str),
      Some("per_list_item_candidate")
    );
    assert_eq!(
      overrides.get("scan.page_index").map(String::as_str),
      Some("2")
    );
    assert_eq!(
      overrides.get("scan.item.index").map(String::as_str),
      Some("4")
    );
    assert_eq!(
      overrides
        .get("scan.item.row_candidate_index")
        .map(String::as_str),
      Some("7")
    );
    assert_eq!(
      overrides.get("scan.item.bounds.y").map(String::as_str),
      Some("120")
    );
    assert_eq!(
      overrides
        .get("scan.item.source_artifact")
        .map(String::as_str),
      Some("artifacts/page.png")
    );
  }

  #[test]
  fn crop_ocr_enrichment_writes_text_and_artifact_context() {
    let mut observation = observation("obs_0001_0001", 0, "", 120);
    observation
      .attributes
      .insert("item_index".to_string(), "2".to_string());

    let enrichment = ListItemCropOcrResult {
      crop_artifact: PathBuf::from("artifacts/list-item-0003.png"),
      context_artifact: PathBuf::from("artifacts/list-item-0003-context.json"),
      text_fragments: vec!["Song A".to_string(), "Artist B".to_string()],
      strategy: "crop_ocr".to_string(),
    };

    apply_list_item_crop_ocr_result(&mut observation, enrichment);

    assert_eq!(observation.raw_text, "Song A | Artist B");
    assert_eq!(observation.normalized_text_key, "song a | artist b");
    assert_eq!(
      observation
        .attributes
        .get("crop_artifact")
        .map(String::as_str),
      Some("artifacts/list-item-0003.png")
    );
    assert_eq!(
      observation
        .attributes
        .get("context_artifact")
        .map(String::as_str),
      Some("artifacts/list-item-0003-context.json")
    );
    assert_eq!(
      observation
        .attributes
        .get("text_fragments")
        .map(String::as_str),
      Some("Song A | Artist B")
    );
  }

  #[test]
  fn list_item_context_payload_uses_crop_ocr_fragments_as_single_text_source() {
    let mut observation = observation("obs_0001_0001", 0, "old observe text", 120);
    observation
      .attributes
      .insert("text_fragments".to_string(), "old observe text".to_string());
    let fragments = vec!["Song A".to_string(), "Artist B".to_string()];

    let payload = build_list_item_context_payload(
      &observation,
      Path::new("artifacts/list-item-0001.png"),
      &fragments,
    );

    assert_eq!(payload.raw_text, "Song A | Artist B");
    assert_eq!(payload.text_fragments, fragments);
    assert_eq!(
      payload.attributes.get("text_fragments").map(String::as_str),
      Some("Song A | Artist B")
    );
  }

  fn observation(id: &str, page_index: usize, text: &str, y: i64) -> CollectionObservation {
    CollectionObservation {
      observation_id: id.to_string(),
      page_index,
      raw_text: text.to_string(),
      normalized_text_key: normalize_observation_text(text),
      bounds: ScanRect {
        x: 10,
        y,
        width: 100,
        height: 24,
      },
      section_context: None,
      source_artifacts: Vec::new(),
      attributes: BTreeMap::new(),
    }
  }

  fn repeated_overlap_page_observations() -> Vec<CollectionObservation> {
    vec![
      observation("obs_0001", 0, "Repeat A", 120),
      observation("obs_0002", 0, "Repeat B", 172),
      observation("obs_0003", 1, "Repeat A", 118),
      observation("obs_0004", 1, "Repeat B", 170),
    ]
  }

  fn write_temp_json_artifact(label: &str, raw: &str) -> PathBuf {
    let counter = TEST_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
      "auv-scroll-scan-{label}-{}-{counter}.json",
      std::process::id()
    ));
    fs::write(&path, raw).expect("write temp json artifact");
    path
  }
}
