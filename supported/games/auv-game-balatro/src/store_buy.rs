use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::model::{ActionPoint, BalatroState, ButtonTarget, SlotId, StoreItem};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreBuyRequest {
  pub target: String,
  pub slot: SlotId,
  pub confirm_purchase: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreBuyClick {
  pub point: ActionPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBuyConfirmationBasis {
  StoreItemRemoved,
  JokerAdded,
  ConsumableAdded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBuyConfirmationFailure {
  NoPurchaseStateChange,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBuyConfirmation {
  NotRequested,
  Purchased { basis: StoreBuyConfirmationBasis },
  NotConfirmed { reason: StoreBuyConfirmationFailure },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBuyIncompleteReason {
  StateReadFailed { message: String },
  PurchaseControlNotFound,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreBuyOutcome {
  SelectionOnly {
    selection: StoreBuyClick,
    reason: StoreBuyIncompleteReason,
  },
  Submitted {
    selection: StoreBuyClick,
    confirmation_button: ButtonTarget,
    submission: StoreBuyClick,
    confirmation: StoreBuyConfirmation,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreBuyResult {
  pub target: String,
  pub slot: SlotId,
  pub item: StoreItem,
  pub outcome: StoreBuyOutcome,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum StoreBuyOutcomeFact<'a> {
  SelectionOnly {
    selection_point: &'a ActionPoint,
    reason: &'a StoreBuyIncompleteReason,
  },
  Submitted {
    selection_point: &'a ActionPoint,
    confirmation_button: &'a ButtonTarget,
    submission_point: &'a ActionPoint,
    confirmation: &'a StoreBuyConfirmation,
  },
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct StoreBuyCompleted<'a> {
  target: &'a str,
  slot: SlotId,
  item: &'a StoreItem,
  outcome: StoreBuyOutcomeFact<'a>,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for StoreBuyCompleted<'_> {
  const NAME: &'static str = "auv.balatro.store_buy.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_store_buy_completed(result: &StoreBuyResult) {
  let outcome = match &result.outcome {
    StoreBuyOutcome::SelectionOnly { selection, reason } => StoreBuyOutcomeFact::SelectionOnly {
      selection_point: &selection.point,
      reason,
    },
    StoreBuyOutcome::Submitted {
      selection,
      confirmation_button,
      submission,
      confirmation,
    } => StoreBuyOutcomeFact::Submitted {
      selection_point: &selection.point,
      confirmation_button,
      submission_point: &submission.point,
      confirmation,
    },
  };
  auv_tracing::emit_event!(StoreBuyCompleted {
    target: &result.target,
    slot: result.slot,
    item: &result.item,
    outcome,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_store_buy_completed(_result: &StoreBuyResult) {}

pub(crate) fn evaluate_store_buy_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> StoreBuyConfirmation {
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return StoreBuyConfirmation::NotConfirmed {
        reason: StoreBuyConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let basis = if after.store.items.len() < before.store.items.len() {
    Some(StoreBuyConfirmationBasis::StoreItemRemoved)
  } else if after.jokers.len() > before.jokers.len() {
    Some(StoreBuyConfirmationBasis::JokerAdded)
  } else if after.consumables.len() > before.consumables.len() {
    Some(StoreBuyConfirmationBasis::ConsumableAdded)
  } else {
    None
  };
  match basis {
    Some(basis) => StoreBuyConfirmation::Purchased { basis },
    None => StoreBuyConfirmation::NotConfirmed {
      reason: StoreBuyConfirmationFailure::NoPurchaseStateChange,
    },
  }
}
