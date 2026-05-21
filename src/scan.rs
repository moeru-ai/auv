use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookDecisionRecord {
  pub hook_name: String,
  pub page_index: usize,
  pub action: HookAction,
  pub reason: String,
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

    // WORKAROUND: First scan progress uses OCR novelty only. Add lightweight
    // screenshot-region diffing before treating no-progress as strong bottom
    // evidence in broader workflows. Current novelty keys include page-local
    // x/y/width/height bounds so repeated visible labels at different positions
    // are not treated as inherently stale.
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
  let rows = value
    .get("rows")
    .and_then(Value::as_array)
    .ok_or_else(|| "malformed observe JSON: missing rows array".to_string())?;

  rows
    .iter()
    .enumerate()
    .map(|(row_index, row)| observation_from_row(page_index, row_index, row, &source_artifact))
    .collect()
}

#[derive(Default)]
struct ScanWindowRegionState {
  pages: Vec<ScanPageRecord>,
  observations: Vec<CollectionObservation>,
  known_observation_signatures: BTreeSet<String>,
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
  let page_observations =
    observations_from_first_json_artifact(page_index, &observe_result, source_artifact)?;
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
      // REVIEW: "Complete by no visual progress" is an evidence claim, not a proof
      // that the application has no hidden content. Keep this wording visible in
      // reports and docs.
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

pub fn hook_decision_from_variables(
  hook_name: &str,
  page_index: usize,
  variables: &BTreeMap<String, String>,
) -> AuvResult<Option<HookDecisionRecord>> {
  // REVIEW: Hook recipes currently communicate decisions through exported
  // variables such as last.scan.hook.action. Revisit if hooks need a smaller
  // manifest or a first-class typed return artifact.
  let Some(action) = variables.get("last.scan.hook.action") else {
    return Ok(None);
  };
  let action = parse_hook_action(action)?;
  let reason = variables
    .get("last.scan.hook.reason")
    .cloned()
    .unwrap_or_else(|| "hook did not provide a reason".to_string());
  Ok(Some(HookDecisionRecord {
    hook_name: hook_name.to_string(),
    page_index,
    action,
    reason,
  }))
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

// REVIEW: This first merge rule is intentionally conservative and only merges
// adjacent-page overlap with nearly identical y positions. Revisit after
// real scan artifacts show whether OCR y jitter needs a wider threshold.
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
    attributes: BTreeMap::new(),
  })
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
  let json_path = first_artifact_with_extension(result, "json")
    .ok_or_else(|| "observe window region did not produce a JSON artifact".to_string())?;
  let raw = fs::read_to_string(&json_path).map_err(|error| {
    format!(
      "failed to read observe JSON {}: {error}",
      json_path.display()
    )
  })?;
  observations_from_observe_json(page_index, &raw, source_artifact)
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
    .filter(|observation| {
      !observation.normalized_text_key.is_empty()
        && known_observation_signatures.insert(observation_signature(observation))
    })
    .count()
}

fn observation_signature(observation: &CollectionObservation) -> String {
  format!(
    "{}|x={}|y={}|w={}|h={}",
    observation.normalized_text_key,
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
  }

  #[test]
  fn hook_decision_rejects_unknown_action() {
    let variables = BTreeMap::from([("last.scan.hook.action".to_string(), "teleport".to_string())]);

    let error = hook_decision_from_variables("per_page_after_observe", 0, &variables)
      .expect_err("invalid action should fail");

    assert!(error.contains("invalid scan hook action"));
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
      action: HookAction::AdjustRegion,
      reason: "need a wider region".to_string(),
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
        "text": "Alpha",
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
}
