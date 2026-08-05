use auv_driver::InputActionResult;
use auv_driver::geometry::Point;
use serde::{Deserialize, Serialize};

use crate::model::{ActionPoint, BalatroPhase, BalatroState, ButtonTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreNextRoundConfirmationRequest {
  None,
  Targeted,
  Weak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreNextRoundRequest {
  pub target: String,
  pub confirmation: StoreNextRoundConfirmationRequest,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundTarget {
  DetectedButton { button: ButtonTarget },
  StoreLayout { frame_point: Point },
}

impl StoreNextRoundTarget {
  pub(crate) fn frame_point(&self) -> Point {
    match self {
      Self::DetectedButton { button } => {
        Point::new(f64::from((button.bbox.x1 + button.bbox.x2) / 2.0), f64::from((button.bbox.y1 + button.bbox.y2) / 2.0))
      }
      Self::StoreLayout { frame_point } => *frame_point,
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmationStrength {
  Targeted,
  Weak,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmationFailure {
  StoreOriginUnconfirmed,
  ExpectedBlindSelection,
  ExpectedKnownStoreExit,
  ObservationFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmation {
  NotRequested,
  Confirmed {
    strength: StoreNextRoundConfirmationStrength,
    before_phase: BalatroPhase,
    after_phase: BalatroPhase,
  },
  NotConfirmed {
    requested: StoreNextRoundConfirmationStrength,
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: StoreNextRoundConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreNextRoundAttempt {
  pub selected_target: StoreNextRoundTarget,
  pub point: ActionPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreNextRoundResult {
  pub target: String,
  pub attempts: Vec<StoreNextRoundAttempt>,
  pub confirmation: StoreNextRoundConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct StoreNextRoundCompleted<'a> {
  target: &'a str,
  attempts: &'a [StoreNextRoundAttempt],
  confirmation: &'a StoreNextRoundConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for StoreNextRoundCompleted<'_> {
  const NAME: &'static str = "auv.balatro.store_next_round.completed";
  const VERSION: u32 = 3;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_store_next_round_completed(result: &StoreNextRoundResult) {
  auv_tracing::emit_event!(StoreNextRoundCompleted {
    target: &result.target,
    attempts: &result.attempts,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_store_next_round_completed(_result: &StoreNextRoundResult) {}

pub(crate) fn evaluate_store_next_round_confirmation(
  request: StoreNextRoundConfirmationRequest,
  before: &BalatroState,
  after: Result<&BalatroState, String>,
) -> StoreNextRoundConfirmation {
  let requested = match request {
    StoreNextRoundConfirmationRequest::None => return StoreNextRoundConfirmation::NotRequested,
    StoreNextRoundConfirmationRequest::Targeted => StoreNextRoundConfirmationStrength::Targeted,
    StoreNextRoundConfirmationRequest::Weak => StoreNextRoundConfirmationStrength::Weak,
  };
  if before.phase != BalatroPhase::Store && !before.store.is_store {
    return StoreNextRoundConfirmation::NotConfirmed {
      requested,
      before_phase: before.phase,
      after_phase: None,
      reason: StoreNextRoundConfirmationFailure::StoreOriginUnconfirmed,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return StoreNextRoundConfirmation::NotConfirmed {
        requested,
        before_phase: before.phase,
        after_phase: None,
        reason: StoreNextRoundConfirmationFailure::ObservationFailed { message },
      };
    }
  };
  let confirmed = match requested {
    StoreNextRoundConfirmationStrength::Targeted => after.phase == BalatroPhase::BlindSelect,
    StoreNextRoundConfirmationStrength::Weak => !matches!(after.phase, BalatroPhase::Store | BalatroPhase::Unknown),
  };
  if confirmed {
    return StoreNextRoundConfirmation::Confirmed {
      strength: requested,
      before_phase: before.phase,
      after_phase: after.phase,
    };
  }
  StoreNextRoundConfirmation::NotConfirmed {
    requested,
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason: match requested {
      StoreNextRoundConfirmationStrength::Targeted => StoreNextRoundConfirmationFailure::ExpectedBlindSelection,
      StoreNextRoundConfirmationStrength::Weak => StoreNextRoundConfirmationFailure::ExpectedKnownStoreExit,
    },
  }
}
