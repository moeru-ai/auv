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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, BalatroPhase, BalatroState, ButtonTarget, FrameRef, RoundState, ScoreState, StoreState};

  fn state_with_skip_button(present: bool) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "pack-skip-test.png".to_string(),
        image_size: ImageSize {
          width: 1600,
          height: 960,
        },
      },
      phase: BalatroPhase::Unknown,
      scores: ScoreState::default(),
      rounds: RoundState::default(),
      hand: Vec::new(),
      jokers: Vec::new(),
      consumables: Vec::new(),
      store: StoreState::default(),
      buttons: present
        .then(|| ButtonTarget {
          id: "button_card_pack_skip".to_string(),
          label: "skip".to_string(),
          bbox: auv_task_object_detection::BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
          confidence: 0.95,
        })
        .into_iter()
        .collect(),
      diagnostics: Vec::new(),
      raw_entities: Vec::new(),
      raw_ui: Vec::new(),
    }
  }

  #[test]
  fn confirmation_requires_the_pack_skip_control_to_disappear() {
    let exited = state_with_skip_button(false);
    let still_open = state_with_skip_button(true);

    assert!(matches!(evaluate_pack_skip_confirmation(Ok(&exited)), PackSkipConfirmation::Confirmed { .. }));
    assert!(matches!(
      evaluate_pack_skip_confirmation(Ok(&still_open)),
      PackSkipConfirmation::NotConfirmed {
        reason: PackSkipConfirmationFailure::SkipControlStillVisible,
        ..
      }
    ));
  }

  #[test]
  fn observation_failure_is_not_reported_as_an_execution_failure() {
    assert!(matches!(
      evaluate_pack_skip_confirmation(Err("detector unavailable".to_string())),
      PackSkipConfirmation::NotConfirmed {
        reason: PackSkipConfirmationFailure::StateReadFailed { .. },
        ..
      }
    ));
  }

  #[test]
  fn direct_result_has_typed_delivery_and_confirmation_without_generic_verification() {
    let result = PackSkipResult {
      target: "Balatro".to_string(),
      selected_button: state_with_skip_button(true).buttons.remove(0),
      window_point: WindowPoint::new(12.0, 34.0),
      delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      confirmation: PackSkipConfirmation::NotRequested,
    };

    let value = serde_json::to_value(result).expect("serialize typed pack-skip result");

    assert!(value.get("delivery").is_some());
    assert_eq!(value.get("confirmation"), Some(&serde_json::json!("not_requested")));
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

  #[test]
  fn completed_event_records_confirmation_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let result = PackSkipResult {
        target: "Balatro".to_string(),
        selected_button: ButtonTarget {
          id: "button_card_pack_skip".to_string(),
          label: "skip".to_string(),
          bbox: auv_task_object_detection::BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
          confidence: 0.95,
        },
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        confirmation: PackSkipConfirmation::Confirmed {
          after_phase: BalatroPhase::BlindSelect,
        },
      };

      root.in_scope(|| emit_pack_skip_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("pack-skip run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.pack_skip.completed")
        .expect("pack-skip completed event");

      assert!(event.payload().get().contains("\"confirmation\""));
      assert!(!event.payload().get().contains("\"delivery\""));
    });
  }
}
