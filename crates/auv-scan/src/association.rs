//! Adjacent-frame observation association (crate-local read-model).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameObservation {
  pub observation_id: String,
  pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociationDiagnostic {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AssociationResult {
  Linked {
    track_id: String,
    previous_observation_id: String,
    current_observation_id: String,
  },
  NewTrack {
    track_id: String,
    current_observation_id: String,
  },
  AmbiguousAssociation {
    label: String,
    candidate_observation_ids: Vec<String>,
    diagnostic: AssociationDiagnostic,
  },
}

fn new_track_id(label: &str) -> String {
  format!("track-{label}")
}

/// Associate observations across adjacent frames by normalized label equality.
pub fn associate_adjacent_frames(previous: &[FrameObservation], current: &[FrameObservation]) -> Vec<AssociationResult> {
  if previous.is_empty() && current.is_empty() {
    return Vec::new();
  }
  let mut results = Vec::new();
  for obs in current {
    let matches: Vec<_> = previous.iter().filter(|prev| prev.label == obs.label).collect();
    match matches.len() {
      0 => results.push(AssociationResult::NewTrack {
        track_id: new_track_id(&obs.label),
        current_observation_id: obs.observation_id.clone(),
      }),
      1 => results.push(AssociationResult::Linked {
        track_id: new_track_id(&obs.label),
        previous_observation_id: matches[0].observation_id.clone(),
        current_observation_id: obs.observation_id.clone(),
      }),
      _ => results.push(AssociationResult::AmbiguousAssociation {
        label: obs.label.clone(),
        candidate_observation_ids: matches.iter().map(|m| m.observation_id.clone()).collect(),
        diagnostic: AssociationDiagnostic {
          code: "ambiguous_association".into(),
          message: format!("multiple previous observations match label={}", obs.label),
        },
      }),
    }
  }
  results
}
