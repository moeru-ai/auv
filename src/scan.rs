use std::collections::BTreeMap;
use std::path::PathBuf;

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
