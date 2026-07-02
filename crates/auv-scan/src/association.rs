//! Adjacent-frame observation association (crate-local read-model).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameObservation {
  pub observation_id: String,
  pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssociationDiagnostic {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
pub fn associate_adjacent_frames(
  previous: &[FrameObservation],
  current: &[FrameObservation],
) -> Vec<AssociationResult> {
  if previous.is_empty() && current.is_empty() {
    return Vec::new();
  }
  let mut results = Vec::new();
  for obs in current {
    let matches: Vec<_> = previous
      .iter()
      .filter(|prev| prev.label == obs.label)
      .collect();
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

#[cfg(test)]
mod tests {
  use super::*;
  use serde::Deserialize;
  use std::path::Path;

  #[derive(Debug, Deserialize)]
  struct ObservationFixture {
    observation_id: String,
    label: String,
  }

  #[derive(Debug, Deserialize)]
  struct LinkedExpect {
    kind: String,
    track_id: String,
    previous_observation_id: String,
    current_observation_id: String,
  }

  #[derive(Debug, Deserialize)]
  struct AmbiguousExpect {
    kind: String,
    label: String,
    diagnostic_code: String,
  }

  #[derive(Debug, Deserialize)]
  struct AssociationFixture {
    scenario: String,
    previous: Vec<ObservationFixture>,
    current: Vec<ObservationFixture>,
    expect: serde_json::Value,
  }

  fn load_association_fixture(scenario_dir: &str) -> AssociationFixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/association")
      .join(scenario_dir)
      .join("manifest.json");
    let text = std::fs::read_to_string(&path).expect("read fixture");
    serde_json::from_str(&text).expect("parse fixture")
  }

  fn observations(raw: &[ObservationFixture]) -> Vec<FrameObservation> {
    raw
      .iter()
      .map(|o| FrameObservation {
        observation_id: o.observation_id.clone(),
        label: o.label.clone(),
      })
      .collect()
  }

  #[test]
  fn associate_adjacent_frames_links_stable_label_fixture() {
    let fixture = load_association_fixture("association_stable_v0");
    assert_eq!(fixture.scenario, "association_stable_v0");
    let results = associate_adjacent_frames(
      &observations(&fixture.previous),
      &observations(&fixture.current),
    );
    let expect: LinkedExpect = serde_json::from_value(fixture.expect).expect("linked expect");
    assert_eq!(expect.kind, "linked");
    assert_eq!(results.len(), 1);
    assert!(matches!(
      &results[0],
      AssociationResult::Linked {
        track_id,
        previous_observation_id,
        current_observation_id,
      } if track_id == &expect.track_id
        && previous_observation_id == &expect.previous_observation_id
        && current_observation_id == &expect.current_observation_id
    ));
  }

  #[test]
  fn associate_adjacent_frames_emits_ambiguous_association_fixture() {
    let fixture = load_association_fixture("association_ambiguous_v0");
    assert_eq!(fixture.scenario, "association_ambiguous_v0");
    let results = associate_adjacent_frames(
      &observations(&fixture.previous),
      &observations(&fixture.current),
    );
    let expect: AmbiguousExpect = serde_json::from_value(fixture.expect).expect("ambiguous expect");
    assert_eq!(expect.kind, "ambiguous_association");
    assert!(matches!(
      results[0],
      AssociationResult::AmbiguousAssociation {
        ref label,
        ref diagnostic,
        ..
      } if label == &expect.label && diagnostic.code == expect.diagnostic_code
    ));
  }
}
