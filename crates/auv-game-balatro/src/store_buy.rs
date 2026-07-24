use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroState, ButtonTarget, SlotId, StoreItem};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreBuyRequest {
  pub target: String,
  pub slot: SlotId,
  pub confirm_purchase: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreBuyClick {
  pub window_point: WindowPoint,
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
    selection_window_point: WindowPoint,
    reason: &'a StoreBuyIncompleteReason,
  },
  Submitted {
    selection_window_point: WindowPoint,
    confirmation_button: &'a ButtonTarget,
    submission_window_point: WindowPoint,
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
      selection_window_point: selection.window_point,
      reason,
    },
    StoreBuyOutcome::Submitted {
      selection,
      confirmation_button,
      submission,
      confirmation,
    } => StoreBuyOutcomeFact::Submitted {
      selection_window_point: selection.window_point,
      confirmation_button,
      submission_window_point: submission.window_point,
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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{
    BALATRO_STATE_SCHEMA_VERSION, BalatroPhase, CacheHint, FrameRef, ObjectZone, Reading, RoundState, ScoreState, StoreItemKind, StoreState,
  };

  fn state(phase: BalatroPhase) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "store-buy-test.png".to_string(),
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

  fn item() -> StoreItem {
    StoreItem {
      slot: SlotId::new(ObjectZone::Store, 0),
      kind: StoreItemKind::Unknown,
      bbox: BoundingBox {
        x1: 1.0,
        y1: 2.0,
        x2: 3.0,
        y2: 4.0,
      },
      confidence: 0.95,
      reading: Reading::unread(),
      cache: CacheHint::default(),
    }
  }

  #[test]
  fn unrelated_phase_change_does_not_confirm_a_purchase() {
    let before = state(BalatroPhase::Store);
    let after = state(BalatroPhase::BlindSelect);

    assert_eq!(
      evaluate_store_buy_confirmation(&before, Ok(&after)),
      StoreBuyConfirmation::NotConfirmed {
        reason: StoreBuyConfirmationFailure::NoPurchaseStateChange,
      }
    );
  }

  #[test]
  fn selection_only_result_preserves_delivery_without_illegal_submission_options() {
    let result = StoreBuyResult {
      target: "Balatro".to_string(),
      slot: SlotId::new(ObjectZone::Store, 0),
      item: item(),
      outcome: StoreBuyOutcome::SelectionOnly {
        selection: StoreBuyClick {
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        },
        reason: StoreBuyIncompleteReason::PurchaseControlNotFound,
      },
    };

    let value = serde_json::to_value(result).expect("serialize store-buy result");

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
  use crate::model::{CacheHint, ObjectZone, Reading, StoreItemKind};

  #[test]
  fn completed_event_projects_store_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let slot = SlotId::new(ObjectZone::Store, 0);
      let result = StoreBuyResult {
        target: "Balatro".to_string(),
        slot,
        item: StoreItem {
          slot,
          kind: StoreItemKind::Unknown,
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
        outcome: StoreBuyOutcome::SelectionOnly {
          selection: StoreBuyClick {
            window_point: WindowPoint::new(12.0, 34.0),
            delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
          },
          reason: StoreBuyIncompleteReason::PurchaseControlNotFound,
        },
      };

      root.in_scope(|| emit_store_buy_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("store-buy run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.store_buy.completed")
        .expect("store-buy completed event");

      assert!(event.payload().get().contains("\"selection_only\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
