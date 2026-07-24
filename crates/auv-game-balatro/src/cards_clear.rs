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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;

  use super::*;
  use crate::model::ObjectZone;

  #[test]
  fn remaining_selection_is_a_typed_outcome_not_an_execution_error() {
    let slot = SlotId::new(ObjectZone::Hand, 2);

    assert_eq!(classify_cards_clear_outcome(vec![slot]), CardsClearOutcome::RemainingSelected { slots: vec![slot] });
    assert_eq!(classify_cards_clear_outcome(Vec::new()), CardsClearOutcome::Cleared);
  }

  #[test]
  fn result_keeps_each_delivery_without_a_generic_verification_object() {
    let slot = SlotId::new(ObjectZone::Hand, 2);
    let result = CardsClearResult {
      target: "Balatro".to_string(),
      initially_selected: vec![slot],
      toggles: vec![CardSelectionToggle {
        slot,
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      }],
      outcome: CardsClearOutcome::Cleared,
    };

    let value = serde_json::to_value(result).expect("serialize cards-clear result");

    assert!(value["toggles"][0].get("delivery").is_some());
    assert_eq!(value.get("outcome"), Some(&serde_json::json!("cleared")));
    assert!(value.get("verification").is_none());
    assert!(value.get("before_image").is_none());
    assert!(value.get("after_image").is_none());
  }
}

#[cfg(all(test, feature = "tracing"))]
mod tracing_tests {
  use std::sync::Arc;

  use auv_driver::InputDeliveryPath;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;
  use crate::model::ObjectZone;

  #[test]
  fn completed_event_keeps_action_facts_without_copying_driver_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let slot = SlotId::new(ObjectZone::Hand, 2);
      let result = CardsClearResult {
        target: "Balatro".to_string(),
        initially_selected: vec![slot],
        toggles: vec![CardSelectionToggle {
          slot,
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        }],
        outcome: CardsClearOutcome::Cleared,
      };

      root.in_scope(|| emit_cards_clear_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("cards-clear run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.cards_clear.completed")
        .expect("cards-clear completed event");

      assert!(event.payload().get().contains("\"toggles\""));
      assert!(event.payload().get().contains("\"outcome\""));
      assert!(!event.payload().get().contains("\"delivery\""));
    });
  }
}
