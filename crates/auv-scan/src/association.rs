//! Adjacent-frame item association (crate-local read-model).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameItem {
  pub item_id: String,
  pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociationDiagnostic {
  pub code: String,
  pub message: String,
}

// NOTICE(domain-result-names): Serialized item IDs use the current names only;
// historical read compatibility requires a named consumer. See
// `docs/ai/references/runtime/2026-10-08-domain-result-naming-migration.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AssociationResult {
  Linked {
    track_id: String,
    previous_item_id: String,
    current_item_id: String,
  },
  NewTrack {
    track_id: String,
    current_item_id: String,
  },
  AmbiguousAssociation {
    label: String,
    candidate_item_ids: Vec<String>,
    diagnostic: AssociationDiagnostic,
  },
}

fn new_track_id(label: &str) -> String {
  format!("track-{label}")
}

/// Associate items across adjacent frames by normalized label equality.
pub fn associate_adjacent_frames(previous: &[FrameItem], current: &[FrameItem]) -> Vec<AssociationResult> {
  if previous.is_empty() && current.is_empty() {
    return Vec::new();
  }
  let mut results = Vec::new();
  for obs in current {
    let matches: Vec<_> = previous.iter().filter(|prev| prev.label == obs.label).collect();
    match matches.len() {
      0 => results.push(AssociationResult::NewTrack {
        track_id: new_track_id(&obs.label),
        current_item_id: obs.item_id.clone(),
      }),
      1 => results.push(AssociationResult::Linked {
        track_id: new_track_id(&obs.label),
        previous_item_id: matches[0].item_id.clone(),
        current_item_id: obs.item_id.clone(),
      }),
      _ => results.push(AssociationResult::AmbiguousAssociation {
        label: obs.label.clone(),
        candidate_item_ids: matches.iter().map(|m| m.item_id.clone()).collect(),
        diagnostic: AssociationDiagnostic {
          code: "ambiguous_association".into(),
          message: format!("multiple previous items match label={}", obs.label),
        },
      }),
    }
  }
  results
}
