use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroPhase, BalatroState, ButtonTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackSkipRequest {
  pub target: String,
  pub confirm_exit: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackSkipConfirmationFailure {
  SkipControlStillVisible,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackSkipConfirmation {
  NotRequested,
  Confirmed {
    after_phase: BalatroPhase,
  },
  NotConfirmed {
    after_phase: Option<BalatroPhase>,
    reason: PackSkipConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PackSkipResult {
  pub target: String,
  pub selected_button: ButtonTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: PackSkipConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct PackSkipCompleted<'a> {
  target: &'a str,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
  confirmation: &'a PackSkipConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for PackSkipCompleted<'_> {
  const NAME: &'static str = "auv.balatro.pack_skip.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_pack_skip_completed(result: &PackSkipResult) {
  auv_tracing::emit_event!(PackSkipCompleted {
    target: &result.target,
    selected_button: &result.selected_button,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_pack_skip_completed(_result: &PackSkipResult) {}

pub(crate) fn evaluate_pack_skip_confirmation(after: Result<&BalatroState, String>) -> PackSkipConfirmation {
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return PackSkipConfirmation::NotConfirmed {
        after_phase: None,
        reason: PackSkipConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  if after.buttons.iter().all(|button| button.id != "button_card_pack_skip") {
    return PackSkipConfirmation::Confirmed {
      after_phase: after.phase,
    };
  }
  PackSkipConfirmation::NotConfirmed {
    after_phase: Some(after.phase),
    reason: PackSkipConfirmationFailure::SkipControlStillVisible,
  }
}
