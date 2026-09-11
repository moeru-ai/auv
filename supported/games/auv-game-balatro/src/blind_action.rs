use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{ActionPoint, BalatroPhase, BalatroState, ButtonTarget, SlotId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindSelectRequest {
  pub target: String,
  pub slot: SlotId,
  pub confirm_started: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindSkipRequest {
  pub target: String,
  pub confirm_exit: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSelectConfirmationFailure {
  OriginNotBlindSelection,
  PlayStateNotObserved,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSelectConfirmation {
  NotRequested,
  Started {
    after_phase: BalatroPhase,
    hand_count: usize,
  },
  NotStarted {
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: BlindSelectConfirmationFailure,
  },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSkipConfirmationFailure {
  OriginNotBlindSelection,
  BlindSelectionStillVisible,
  ResultingPhaseUnknown,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSkipConfirmation {
  NotRequested,
  Exited {
    after_phase: BalatroPhase,
  },
  NotExited {
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: BlindSkipConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlindSelectAttempt {
  pub selected_button: ButtonTarget,
  pub point: ActionPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlindSelectResult {
  pub target: String,
  pub slot: SlotId,
  pub attempts: Vec<BlindSelectAttempt>,
  pub confirmation: BlindSelectConfirmation,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlindSkipResult {
  pub target: String,
  pub selected_button: ButtonTarget,
  // TODO(balatro-remote-blind-skip): migrate this to ActionPoint when the
  // owner-approved remote skip operation lands.
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: BlindSkipConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct BlindSelectCompleted<'a> {
  target: &'a str,
  slot: SlotId,
  attempts: &'a [BlindSelectAttempt],
  confirmation: &'a BlindSelectConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for BlindSelectCompleted<'_> {
  const NAME: &'static str = "auv.balatro.blind_select.completed";
  const VERSION: u32 = 3;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_blind_select_completed(result: &BlindSelectResult) {
  auv_tracing::emit_event!(BlindSelectCompleted {
    target: &result.target,
    slot: result.slot,
    attempts: &result.attempts,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_blind_select_completed(_result: &BlindSelectResult) {}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct BlindSkipCompleted<'a> {
  target: &'a str,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
  confirmation: &'a BlindSkipConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for BlindSkipCompleted<'_> {
  const NAME: &'static str = "auv.balatro.blind_skip.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_blind_skip_completed(result: &BlindSkipResult) {
  auv_tracing::emit_event!(BlindSkipCompleted {
    target: &result.target,
    selected_button: &result.selected_button,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_blind_skip_completed(_result: &BlindSkipResult) {}

pub(crate) fn evaluate_blind_select_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> BlindSelectConfirmation {
  if before.phase != BalatroPhase::BlindSelect {
    return BlindSelectConfirmation::NotStarted {
      before_phase: before.phase,
      after_phase: None,
      reason: BlindSelectConfirmationFailure::OriginNotBlindSelection,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return BlindSelectConfirmation::NotStarted {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSelectConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  if after.phase == BalatroPhase::Playing || !after.hand.is_empty() {
    return BlindSelectConfirmation::Started {
      after_phase: after.phase,
      hand_count: after.hand.len(),
    };
  }
  BlindSelectConfirmation::NotStarted {
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason: BlindSelectConfirmationFailure::PlayStateNotObserved,
  }
}

pub(crate) fn evaluate_blind_skip_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> BlindSkipConfirmation {
  if before.phase != BalatroPhase::BlindSelect {
    return BlindSkipConfirmation::NotExited {
      before_phase: before.phase,
      after_phase: None,
      reason: BlindSkipConfirmationFailure::OriginNotBlindSelection,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return BlindSkipConfirmation::NotExited {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSkipConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let reason = match after.phase {
    BalatroPhase::BlindSelect => BlindSkipConfirmationFailure::BlindSelectionStillVisible,
    BalatroPhase::Unknown => BlindSkipConfirmationFailure::ResultingPhaseUnknown,
    phase => {
      return BlindSkipConfirmation::Exited { after_phase: phase };
    }
  };
  BlindSkipConfirmation::NotExited {
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason,
  }
}
