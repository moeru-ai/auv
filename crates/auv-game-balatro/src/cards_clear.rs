use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::SlotId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardsClearRequest {
  pub target: String,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardSelectionToggle {
  pub slot: SlotId,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardsClearIncompleteReason {
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardsClearOutcome {
  Cleared,
  RemainingSelected { slots: Vec<SlotId> },
  Incomplete { reason: CardsClearIncompleteReason },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardsClearResult {
  pub target: String,
  pub initially_selected: Vec<SlotId>,
  pub toggles: Vec<CardSelectionToggle>,
  pub outcome: CardsClearOutcome,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct CardSelectionToggleFact {
  slot: SlotId,
  window_point: WindowPoint,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct CardsClearCompleted<'a> {
  target: &'a str,
  initially_selected: &'a [SlotId],
  toggles: Vec<CardSelectionToggleFact>,
  outcome: &'a CardsClearOutcome,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for CardsClearCompleted<'_> {
  const NAME: &'static str = "auv.balatro.cards_clear.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_cards_clear_completed(result: &CardsClearResult) {
  auv_tracing::emit_event!(CardsClearCompleted {
    target: &result.target,
    initially_selected: &result.initially_selected,
    toggles: result
      .toggles
      .iter()
      .map(|toggle| CardSelectionToggleFact {
        slot: toggle.slot,
        window_point: toggle.window_point,
      })
      .collect(),
    outcome: &result.outcome,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_cards_clear_completed(_result: &CardsClearResult) {}

pub(crate) fn classify_cards_clear_outcome(remaining: Vec<SlotId>) -> CardsClearOutcome {
  if remaining.is_empty() {
    CardsClearOutcome::Cleared
  } else {
    CardsClearOutcome::RemainingSelected { slots: remaining }
  }
}
