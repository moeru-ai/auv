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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, BalatroPhase, CacheHint, FrameRef, Reading, RoundState, ScoreState, StoreState};

  fn state(phase: BalatroPhase) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "object-sell-test.png".to_string(),
        image_size: ImageSize {
          width: 1600,
          height: 960,
        },
      },
      phase,
      scores: ScoreState::default(),
      rounds: RoundState::default(),
      hand: Vec::new(),
      jokers: Vec::new(),
      consumables: Vec::new(),
      store: StoreState::default(),
      buttons: Vec::new(),
      diagnostics: Vec::new(),
      raw_entities: Vec::new(),
      raw_ui: Vec::new(),
    }
  }

  #[test]
  fn unrelated_phase_change_does_not_confirm_a_sale() {
    let before = state(BalatroPhase::Playing);
    let after = state(BalatroPhase::Store);

    assert_eq!(
      evaluate_object_sell_confirmation(ObjectZone::Joker, &before, Ok(&after)),
      ObjectSellConfirmation::NotConfirmed {
        reason: ObjectSellConfirmationFailure::NoSaleStateChange,
      }
    );
  }

  #[test]
  fn cash_change_can_confirm_when_object_detection_is_noisy() {
    let mut before = state(BalatroPhase::Playing);
    let mut after = state(BalatroPhase::Playing);
    before.rounds.cash = Some("$4".to_string());
    after.rounds.cash = Some("$5".to_string());

    assert_eq!(
      evaluate_object_sell_confirmation(ObjectZone::Consumable, &before, Ok(&after)),
      ObjectSellConfirmation::Sold {
        basis: ObjectSellConfirmationBasis::CashChanged,
      }
    );
  }

  #[test]
  fn selection_only_result_preserves_delivery_without_generic_verification() {
    let slot = SlotId::new(ObjectZone::Joker, 0);
    let result = ObjectSellResult {
      target: "Balatro".to_string(),
      slot,
      object: SellableObject::Joker {
        joker: JokerSlot {
          slot,
          bbox: BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
          confidence: 0.95,
          reading: Reading::unread(),
          cache: CacheHint::default(),
        },
      },
      outcome: ObjectSellOutcome::SelectionOnly {
        selection: ObjectSellClick {
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        },
        reason: ObjectSellIncompleteReason::SellControlNotFound,
      },
    };

    let value = serde_json::to_value(result).expect("serialize object-sell result");

    assert!(value["outcome"]["selection_only"]["selection"].get("delivery").is_some());
    assert!(value.get("verification").is_none());
    assert!(value.get("before_image").is_none());
    assert!(value.get("selected_image").is_none());
  }
}

#[cfg(all(test, feature = "tracing"))]
mod tracing_tests {
  use std::sync::Arc;

  use auv_driver::InputDeliveryPath;
  use auv_task_object_detection::BoundingBox;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;
  use crate::model::{CacheHint, Reading};

  #[test]
  fn completed_event_projects_sale_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let slot = SlotId::new(ObjectZone::Joker, 0);
      let result = ObjectSellResult {
        target: "Balatro".to_string(),
        slot,
        object: SellableObject::Joker {
          joker: JokerSlot {
            slot,
            bbox: BoundingBox {
              x1: 1.0,
              y1: 2.0,
              x2: 3.0,
              y2: 4.0,
            },
            confidence: 0.95,
            reading: Reading::unread(),
            cache: CacheHint::default(),
          },
        },
        outcome: ObjectSellOutcome::SelectionOnly {
          selection: ObjectSellClick {
            window_point: WindowPoint::new(12.0, 34.0),
            delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
          },
          reason: ObjectSellIncompleteReason::SellControlNotFound,
        },
      };

      root.in_scope(|| emit_object_sell_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("object-sell run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.object_sell.completed")
        .expect("object-sell completed event");

      assert!(event.payload().get().contains("\"selection_only\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
