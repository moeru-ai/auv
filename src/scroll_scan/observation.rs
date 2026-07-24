// File: src/scroll_scan/observation.rs
use super::{CollectionObservation, ObservationCluster};

pub fn normalize_observation_text(raw: &str) -> String {
  raw.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_lowercase()
}

pub fn conservative_merge_observations(observations: &[CollectionObservation]) -> Vec<ObservationCluster> {
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
        let decision = merge_decision(observation, candidate).expect("merge decision should exist when adjacent observations merge");
        merge_reason = decision.reason.to_string();
        confidence = decision.confidence;
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
pub(crate) fn should_merge_adjacent_observations(left: &CollectionObservation, right: &CollectionObservation) -> bool {
  merge_decision(left, right).is_some()
}

struct MergeDecision {
  reason: &'static str,
  confidence: f64,
}

fn merge_decision(left: &CollectionObservation, right: &CollectionObservation) -> Option<MergeDecision> {
  if left.section_context != right.section_context {
    return None;
  }
  if left.page_index.abs_diff(right.page_index) != 1 {
    return None;
  }

  if left.normalized_text_key.is_empty() || left.normalized_text_key != right.normalized_text_key {
    return None;
  }
  if (left.bounds.y - right.bounds.y).abs() > 8 {
    return None;
  }

  Some(MergeDecision {
    reason: "same_text_adjacent_page_near_y",
    confidence: 0.72,
  })
}
