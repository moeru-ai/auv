use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroState, ButtonTarget, ConsumableSlot, JokerSlot, ObjectZone, SlotId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectSellRequest {
  pub target: String,
  pub slot: SlotId,
  pub confirm_sale: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SellableObject {
  Joker { joker: JokerSlot },
  Consumable { consumable: ConsumableSlot },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObjectSellClick {
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSellConfirmationBasis {
  ObjectRemoved,
  CashChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSellConfirmationFailure {
  NoSaleStateChange,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSellConfirmation {
  NotRequested,
  Sold {
    basis: ObjectSellConfirmationBasis,
  },
  NotConfirmed {
    reason: ObjectSellConfirmationFailure,
  },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSellIncompleteReason {
  StateReadFailed { message: String },
  SellControlNotFound,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSellOutcome {
  SelectionOnly {
    selection: ObjectSellClick,
    reason: ObjectSellIncompleteReason,
  },
  Submitted {
    selection: ObjectSellClick,
    sell_button: ButtonTarget,
    submission: ObjectSellClick,
    confirmation: ObjectSellConfirmation,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObjectSellResult {
  pub target: String,
  pub slot: SlotId,
  pub object: SellableObject,
  pub outcome: ObjectSellOutcome,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ObjectSellOutcomeFact<'a> {
  SelectionOnly {
    selection_window_point: WindowPoint,
    reason: &'a ObjectSellIncompleteReason,
  },
  Submitted {
    selection_window_point: WindowPoint,
    sell_button: &'a ButtonTarget,
    submission_window_point: WindowPoint,
    confirmation: &'a ObjectSellConfirmation,
  },
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct ObjectSellCompleted<'a> {
  target: &'a str,
  slot: SlotId,
  object: &'a SellableObject,
  outcome: ObjectSellOutcomeFact<'a>,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for ObjectSellCompleted<'_> {
  const NAME: &'static str = "auv.balatro.object_sell.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_object_sell_completed(result: &ObjectSellResult) {
  let outcome = match &result.outcome {
    ObjectSellOutcome::SelectionOnly { selection, reason } => ObjectSellOutcomeFact::SelectionOnly {
      selection_window_point: selection.window_point,
      reason,
    },
    ObjectSellOutcome::Submitted {
      selection,
      sell_button,
      submission,
      confirmation,
    } => ObjectSellOutcomeFact::Submitted {
      selection_window_point: selection.window_point,
      sell_button,
      submission_window_point: submission.window_point,
      confirmation,
    },
  };
  auv_tracing::emit_event!(ObjectSellCompleted {
    target: &result.target,
    slot: result.slot,
    object: &result.object,
    outcome,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_object_sell_completed(_result: &ObjectSellResult) {}

pub(crate) fn evaluate_object_sell_confirmation(
  zone: ObjectZone,
  before: &BalatroState,
  after: Result<&BalatroState, String>,
) -> ObjectSellConfirmation {
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return ObjectSellConfirmation::NotConfirmed {
        reason: ObjectSellConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let object_removed = match zone {
    ObjectZone::Joker => after.jokers.len() < before.jokers.len(),
    ObjectZone::Consumable => after.consumables.len() < before.consumables.len(),
    _ => false,
  };
  if object_removed {
    return ObjectSellConfirmation::Sold {
      basis: ObjectSellConfirmationBasis::ObjectRemoved,
    };
  }
  if matches!(
    (&before.rounds.cash, &after.rounds.cash),
    (Some(before_cash), Some(after_cash)) if before_cash != after_cash
  ) {
    return ObjectSellConfirmation::Sold {
      basis: ObjectSellConfirmationBasis::CashChanged,
    };
  }
  ObjectSellConfirmation::NotConfirmed {
    reason: ObjectSellConfirmationFailure::NoSaleStateChange,
  }
}
