use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::hand_selection::{HandSelectionResult, HandSelectionState};
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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{
    BALATRO_STATE_SCHEMA_VERSION, BalatroPhase, CacheHint, ConsumableKind, FrameRef, ObjectZone, Reading, RoundState, ScoreState, StoreState,
  };

  fn state(phase: BalatroPhase) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "consumable-use-test.png".to_string(),
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

  fn consumable() -> ConsumableSlot {
    ConsumableSlot {
      slot: SlotId::new(ObjectZone::Consumable, 0),
      kind: ConsumableKind::Unknown,
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
  fn unrelated_phase_change_does_not_confirm_consumable_use() {
    let before = state(BalatroPhase::Playing);
    let after = state(BalatroPhase::Store);

    assert_eq!(
      evaluate_consumable_use_confirmation(&before, Ok(&after)),
      ConsumableUseConfirmation::NotConfirmed {
        reason: ConsumableUseConfirmationFailure::NoUseStateChange,
      }
    );
  }

  #[test]
  fn stopped_result_retains_prior_deliveries_without_generic_verification() {
    let result = ConsumableUseResult {
      target: "Balatro".to_string(),
      slot: SlotId::new(ObjectZone::Consumable, 0),
      consumable: consumable(),
      actions: vec![ConsumableUseAction::SelectConsumable {
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      }],
      state: ConsumableUseState::Stopped {
        reason: ConsumableUseStop::SelectedStateReadFailed {
          message: "capture failed".to_string(),
        },
      },
    };

    let value = serde_json::to_value(result).expect("serialize consumable-use result");

    assert!(value["actions"][0]["select_consumable"].get("delivery").is_some());
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
  use crate::model::{CacheHint, ConsumableKind, ObjectZone, Reading};

  #[test]
  fn completed_event_projects_action_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let slot = SlotId::new(ObjectZone::Consumable, 0);
      let result = ConsumableUseResult {
        target: "Balatro".to_string(),
        slot,
        consumable: ConsumableSlot {
          slot,
          kind: ConsumableKind::Unknown,
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
        actions: vec![ConsumableUseAction::SelectConsumable {
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        }],
        state: ConsumableUseState::Stopped {
          reason: ConsumableUseStop::SelectedStateReadFailed {
            message: "capture failed".to_string(),
          },
        },
      };

      root.in_scope(|| emit_consumable_use_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("consumable-use run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.consumable_use.completed")
        .expect("consumable-use completed event");

      assert!(event.payload().get().contains("\"select_consumable\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
