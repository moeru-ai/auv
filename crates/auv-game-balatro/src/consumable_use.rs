use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::hand_selection::HandSelectionResult;
#[cfg(feature = "tracing")]
use crate::hand_selection::HandSelectionState;
use crate::model::{BalatroState, ButtonTarget, ConsumableSlot, SlotId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumableUseRequest {
  pub target: String,
  pub slot: SlotId,
  pub hand_targets: Vec<SlotId>,
  pub confirm_use: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseControl {
  DetectedButton { button: ButtonTarget },
  SelectedConsumableLayout,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseAction {
  SelectHandTargets {
    selection: HandSelectionResult,
  },
  SelectConsumable {
    window_point: WindowPoint,
    delivery: InputActionResult,
  },
  SubmitUse {
    control: ConsumableUseControl,
    window_point: WindowPoint,
    delivery: InputActionResult,
  },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseConfirmationBasis {
  ConsumableRemoved,
  ScoresChanged,
  ConsumableRemovedAndScoresChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseConfirmationFailure {
  NoUseStateChange,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseConfirmation {
  NotRequested,
  Used {
    basis: ConsumableUseConfirmationBasis,
  },
  NotConfirmed {
    reason: ConsumableUseConfirmationFailure,
  },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseStop {
  HandTargetsNotReady,
  SelectedStateReadFailed { message: String },
  UseControlNotFound { message: String },
  ConsumableSelectionFailed { message: String },
  UseSubmissionFailed { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableUseState {
  Stopped {
    reason: ConsumableUseStop,
  },
  Submitted {
    confirmation: ConsumableUseConfirmation,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConsumableUseResult {
  pub target: String,
  pub slot: SlotId,
  pub consumable: ConsumableSlot,
  pub actions: Vec<ConsumableUseAction>,
  pub state: ConsumableUseState,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ConsumableUseActionFact<'a> {
  SelectHandTargets {
    requested: &'a [SlotId],
    result: &'a HandSelectionState,
    toggle_count: usize,
  },
  SelectConsumable {
    window_point: WindowPoint,
  },
  SubmitUse {
    control: &'a ConsumableUseControl,
    window_point: WindowPoint,
  },
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct ConsumableUseCompleted<'a> {
  target: &'a str,
  slot: SlotId,
  consumable: &'a ConsumableSlot,
  actions: Vec<ConsumableUseActionFact<'a>>,
  state: &'a ConsumableUseState,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for ConsumableUseCompleted<'_> {
  const NAME: &'static str = "auv.balatro.consumable_use.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_consumable_use_completed(result: &ConsumableUseResult) {
  let actions = result
    .actions
    .iter()
    .map(|action| match action {
      ConsumableUseAction::SelectHandTargets { selection } => ConsumableUseActionFact::SelectHandTargets {
        requested: &selection.requested,
        result: &selection.state,
        toggle_count: selection.toggles.len(),
      },
      ConsumableUseAction::SelectConsumable { window_point, .. } => ConsumableUseActionFact::SelectConsumable {
        window_point: *window_point,
      },
      ConsumableUseAction::SubmitUse {
        control,
        window_point,
        ..
      } => ConsumableUseActionFact::SubmitUse {
        control,
        window_point: *window_point,
      },
    })
    .collect();
  auv_tracing::emit_event!(ConsumableUseCompleted {
    target: &result.target,
    slot: result.slot,
    consumable: &result.consumable,
    actions,
    state: &result.state,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_consumable_use_completed(_result: &ConsumableUseResult) {}

pub(crate) fn evaluate_consumable_use_confirmation(
  before: &BalatroState,
  after: Result<&BalatroState, String>,
) -> ConsumableUseConfirmation {
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return ConsumableUseConfirmation::NotConfirmed {
        reason: ConsumableUseConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let consumable_removed = after.consumables.len() < before.consumables.len();
  let scores_changed = after.scores != before.scores;
  let basis = match (consumable_removed, scores_changed) {
    (true, true) => Some(ConsumableUseConfirmationBasis::ConsumableRemovedAndScoresChanged),
    (true, false) => Some(ConsumableUseConfirmationBasis::ConsumableRemoved),
    (false, true) => Some(ConsumableUseConfirmationBasis::ScoresChanged),
    (false, false) => None,
  };
  match basis {
    Some(basis) => ConsumableUseConfirmation::Used { basis },
    None => ConsumableUseConfirmation::NotConfirmed {
      reason: ConsumableUseConfirmationFailure::NoUseStateChange,
    },
  }
}
