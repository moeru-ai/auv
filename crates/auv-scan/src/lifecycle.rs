//! Evidence-first anchor lifecycle evaluator (crate-local v1; no durable wire).

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionEvidence {
  pub kind: String,
  pub ref_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
  Observed {
    observation_id: String,
    evidence: TransitionEvidence,
  },
  AssociationLinked {
    track_id: String,
    evidence: TransitionEvidence,
  },
  Stale {
    reason_code: String,
    evidence: TransitionEvidence,
  },
  ReacquisitionNeeded {
    track_id: String,
    evidence: TransitionEvidence,
  },
  Reacquired {
    track_id: String,
    evidence: TransitionEvidence,
  },
  Lost {
    track_id: String,
    evidence: TransitionEvidence,
  },
  AmbiguousReacquire {
    track_id: String,
    evidence: TransitionEvidence,
  },
  ObservationFailed {
    reason_code: String,
    evidence: TransitionEvidence,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleVerdict {
  Reacquired { track_id: String },
  Lost { track_id: String },
  AmbiguousReacquire { track_id: String },
  ObservationFailed { reason_code: String },
  Incomplete,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LifecycleError {
  #[error("lifecycle event missing transition evidence at index {index}")]
  MissingEvidence { index: usize },
  #[error("empty lifecycle event stream")]
  EmptyEvents,
}

fn has_evidence(event: &LifecycleEvent) -> bool {
  match event {
    LifecycleEvent::Observed { evidence, .. }
    | LifecycleEvent::AssociationLinked { evidence, .. }
    | LifecycleEvent::Stale { evidence, .. }
    | LifecycleEvent::ReacquisitionNeeded { evidence, .. }
    | LifecycleEvent::Reacquired { evidence, .. }
    | LifecycleEvent::Lost { evidence, .. }
    | LifecycleEvent::AmbiguousReacquire { evidence, .. }
    | LifecycleEvent::ObservationFailed { evidence, .. } => !evidence.kind.is_empty() && !evidence.ref_id.is_empty(),
  }
}

/// Evaluate a baked lifecycle event stream into a terminal verdict (evidence-first).
pub fn evaluate_lifecycle(events: &[LifecycleEvent]) -> Result<LifecycleVerdict, LifecycleError> {
  if events.is_empty() {
    return Err(LifecycleError::EmptyEvents);
  }
  for (index, event) in events.iter().enumerate() {
    if !has_evidence(event) {
      return Err(LifecycleError::MissingEvidence { index });
    }
  }

  for event in events.iter().rev() {
    match event {
      LifecycleEvent::Reacquired { track_id, .. } => {
        return Ok(LifecycleVerdict::Reacquired {
          track_id: track_id.clone(),
        });
      }
      LifecycleEvent::Lost { track_id, .. } => {
        return Ok(LifecycleVerdict::Lost {
          track_id: track_id.clone(),
        });
      }
      LifecycleEvent::AmbiguousReacquire { track_id, .. } => {
        return Ok(LifecycleVerdict::AmbiguousReacquire {
          track_id: track_id.clone(),
        });
      }
      LifecycleEvent::ObservationFailed { reason_code, .. } => {
        return Ok(LifecycleVerdict::ObservationFailed {
          reason_code: reason_code.clone(),
        });
      }
      _ => {}
    }
  }

  Ok(LifecycleVerdict::Incomplete)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde::Deserialize;
  use std::path::Path;

  #[derive(Debug, Deserialize)]
  struct EvidenceFixture {
    kind: String,
    ref_id: String,
  }

  #[derive(Debug, Deserialize)]
  struct LifecycleEventFixture {
    event: String,
    observation_id: Option<String>,
    track_id: Option<String>,
    reason_code: Option<String>,
    evidence: EvidenceFixture,
  }

  #[derive(Debug, Deserialize)]
  struct LifecycleExpectFixture {
    verdict: String,
    track_id: Option<String>,
    #[allow(dead_code)]
    reason_code: Option<String>,
  }

  #[derive(Debug, Deserialize)]
  struct LifecycleFixture {
    scenario: String,
    expect: LifecycleExpectFixture,
    events: Vec<LifecycleEventFixture>,
  }

  fn parse_lifecycle_event(raw: &LifecycleEventFixture) -> LifecycleEvent {
    let evidence = TransitionEvidence {
      kind: raw.evidence.kind.clone(),
      ref_id: raw.evidence.ref_id.clone(),
    };
    match raw.event.as_str() {
      "observed" => LifecycleEvent::Observed {
        observation_id: raw.observation_id.clone().expect("observation_id"),
        evidence,
      },
      "association_linked" => LifecycleEvent::AssociationLinked {
        track_id: raw.track_id.clone().expect("track_id"),
        evidence,
      },
      "stale" => LifecycleEvent::Stale {
        reason_code: raw.reason_code.clone().expect("reason_code"),
        evidence,
      },
      "reacquisition_needed" => LifecycleEvent::ReacquisitionNeeded {
        track_id: raw.track_id.clone().expect("track_id"),
        evidence,
      },
      "reacquired" => LifecycleEvent::Reacquired {
        track_id: raw.track_id.clone().expect("track_id"),
        evidence,
      },
      "lost" => LifecycleEvent::Lost {
        track_id: raw.track_id.clone().expect("track_id"),
        evidence,
      },
      "ambiguous_reacquire" => LifecycleEvent::AmbiguousReacquire {
        track_id: raw.track_id.clone().expect("track_id"),
        evidence,
      },
      "observation_failed" => LifecycleEvent::ObservationFailed {
        reason_code: raw.reason_code.clone().expect("reason_code"),
        evidence,
      },
      other => panic!("unknown lifecycle event kind: {other}"),
    }
  }

  fn load_lifecycle_fixture(scenario_dir: &str) -> (LifecycleFixture, Vec<LifecycleEvent>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/lifecycle").join(scenario_dir).join("events.json");
    let text = std::fs::read_to_string(&path).expect("read fixture");
    let fixture: LifecycleFixture = serde_json::from_str(&text).expect("parse fixture");
    let events = fixture.events.iter().map(parse_lifecycle_event).collect::<Vec<_>>();
    (fixture, events)
  }

  fn assert_fixture_verdict(fixture: &LifecycleFixture, verdict: LifecycleVerdict) {
    match fixture.expect.verdict.as_str() {
      "reacquired" => assert_eq!(
        verdict,
        LifecycleVerdict::Reacquired {
          track_id: fixture.expect.track_id.clone().expect("track_id")
        }
      ),
      "lost" => assert_eq!(
        verdict,
        LifecycleVerdict::Lost {
          track_id: fixture.expect.track_id.clone().expect("track_id")
        }
      ),
      "ambiguous_reacquire" => assert_eq!(
        verdict,
        LifecycleVerdict::AmbiguousReacquire {
          track_id: fixture.expect.track_id.clone().expect("track_id")
        }
      ),
      other => panic!("unknown expected verdict: {other}"),
    }
  }

  #[test]
  fn evaluate_lifecycle_reacquired_fixture() {
    let (fixture, events) = load_lifecycle_fixture("lifecycle_reacquired_v0");
    assert_eq!(fixture.scenario, "lifecycle_reacquired_v0");
    let verdict = evaluate_lifecycle(&events).expect("evaluate");
    assert_fixture_verdict(&fixture, verdict);
  }

  #[test]
  fn evaluate_lifecycle_lost_fixture() {
    let (fixture, events) = load_lifecycle_fixture("lifecycle_lost_v0");
    assert_eq!(fixture.scenario, "lifecycle_lost_v0");
    let verdict = evaluate_lifecycle(&events).expect("evaluate");
    assert_fixture_verdict(&fixture, verdict);
  }

  #[test]
  fn evaluate_lifecycle_ambiguous_reacquire_fixture() {
    let (fixture, events) = load_lifecycle_fixture("lifecycle_ambiguous_reacquire_v0");
    assert_eq!(fixture.scenario, "lifecycle_ambiguous_reacquire_v0");
    let verdict = evaluate_lifecycle(&events).expect("evaluate");
    assert_fixture_verdict(&fixture, verdict);
  }

  #[test]
  fn evaluate_lifecycle_rejects_missing_evidence() {
    let events = vec![LifecycleEvent::Lost {
      track_id: "track-a".into(),
      evidence: TransitionEvidence {
        kind: String::new(),
        ref_id: "x".into(),
      },
    }];
    let err = evaluate_lifecycle(&events).expect_err("missing");
    assert_eq!(err, LifecycleError::MissingEvidence { index: 0 });
  }
}
