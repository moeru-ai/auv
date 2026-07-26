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
