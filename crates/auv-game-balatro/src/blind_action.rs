use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroPhase, BalatroState, ButtonTarget, SlotId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindSelectRequest {
  pub target: String,
  pub slot: SlotId,
  pub confirm_started: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindSkipRequest {
  pub target: String,
  pub confirm_exit: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSelectConfirmationFailure {
  OriginNotBlindSelection,
  PlayStateNotObserved,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSelectConfirmation {
  NotRequested,
  Started {
    after_phase: BalatroPhase,
    hand_count: usize,
  },
  NotStarted {
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: BlindSelectConfirmationFailure,
  },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSkipConfirmationFailure {
  OriginNotBlindSelection,
  BlindSelectionStillVisible,
  ResultingPhaseUnknown,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlindSkipConfirmation {
  NotRequested,
  Exited {
    after_phase: BalatroPhase,
  },
  NotExited {
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: BlindSkipConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlindSelectResult {
  pub target: String,
  pub slot: SlotId,
  pub selected_button: ButtonTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: BlindSelectConfirmation,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlindSkipResult {
  pub target: String,
  pub selected_button: ButtonTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: BlindSkipConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct BlindSelectCompleted<'a> {
  target: &'a str,
  slot: SlotId,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
  confirmation: &'a BlindSelectConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for BlindSelectCompleted<'_> {
  const NAME: &'static str = "auv.balatro.blind_select.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_blind_select_completed(result: &BlindSelectResult) {
  auv_tracing::emit_event!(BlindSelectCompleted {
    target: &result.target,
    slot: result.slot,
    selected_button: &result.selected_button,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_blind_select_completed(_result: &BlindSelectResult) {}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct BlindSkipCompleted<'a> {
  target: &'a str,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
  confirmation: &'a BlindSkipConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for BlindSkipCompleted<'_> {
  const NAME: &'static str = "auv.balatro.blind_skip.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_blind_skip_completed(result: &BlindSkipResult) {
  auv_tracing::emit_event!(BlindSkipCompleted {
    target: &result.target,
    selected_button: &result.selected_button,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_blind_skip_completed(_result: &BlindSkipResult) {}

pub(crate) fn evaluate_blind_select_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> BlindSelectConfirmation {
  if before.phase != BalatroPhase::BlindSelect {
    return BlindSelectConfirmation::NotStarted {
      before_phase: before.phase,
      after_phase: None,
      reason: BlindSelectConfirmationFailure::OriginNotBlindSelection,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return BlindSelectConfirmation::NotStarted {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSelectConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  if after.phase == BalatroPhase::Playing || !after.hand.is_empty() {
    return BlindSelectConfirmation::Started {
      after_phase: after.phase,
      hand_count: after.hand.len(),
    };
  }
  BlindSelectConfirmation::NotStarted {
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason: BlindSelectConfirmationFailure::PlayStateNotObserved,
  }
}

pub(crate) fn evaluate_blind_skip_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> BlindSkipConfirmation {
  if before.phase != BalatroPhase::BlindSelect {
    return BlindSkipConfirmation::NotExited {
      before_phase: before.phase,
      after_phase: None,
      reason: BlindSkipConfirmationFailure::OriginNotBlindSelection,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return BlindSkipConfirmation::NotExited {
        before_phase: before.phase,
        after_phase: None,
        reason: BlindSkipConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let reason = match after.phase {
    BalatroPhase::BlindSelect => BlindSkipConfirmationFailure::BlindSelectionStillVisible,
    BalatroPhase::Unknown => BlindSkipConfirmationFailure::ResultingPhaseUnknown,
    phase => {
      return BlindSkipConfirmation::Exited { after_phase: phase };
    }
  };
  BlindSkipConfirmation::NotExited {
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason,
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, FrameRef, ObjectZone, RoundState, ScoreState, StoreState};

  fn state(phase: BalatroPhase) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "blind-action-test.png".to_string(),
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

  fn button(id: &str) -> ButtonTarget {
    ButtonTarget {
      id: id.to_string(),
      label: id.to_string(),
      bbox: BoundingBox {
        x1: 1.0,
        y1: 2.0,
        x2: 3.0,
        y2: 4.0,
      },
      confidence: 0.95,
    }
  }

  #[test]
  fn skip_does_not_treat_an_unknown_resulting_phase_as_success() {
    let before = state(BalatroPhase::BlindSelect);
    let after = state(BalatroPhase::Unknown);

    assert!(matches!(
      evaluate_blind_skip_confirmation(&before, Ok(&after)),
      BlindSkipConfirmation::NotExited {
        reason: BlindSkipConfirmationFailure::ResultingPhaseUnknown,
        ..
      }
    ));
  }

  #[test]
  fn select_requires_a_blind_selection_origin_and_observed_play_state() {
    let wrong_origin = state(BalatroPhase::Store);
    let before = state(BalatroPhase::BlindSelect);
    let playing = state(BalatroPhase::Playing);

    assert!(matches!(
      evaluate_blind_select_confirmation(&wrong_origin, Ok(&playing)),
      BlindSelectConfirmation::NotStarted {
        reason: BlindSelectConfirmationFailure::OriginNotBlindSelection,
        ..
      }
    ));
    assert!(matches!(evaluate_blind_select_confirmation(&before, Ok(&playing)), BlindSelectConfirmation::Started { .. }));
  }

  #[test]
  fn direct_results_do_not_reintroduce_generic_verification() {
    let select = BlindSelectResult {
      target: "Balatro".to_string(),
      slot: SlotId::new(ObjectZone::Blind, 1),
      selected_button: button("button_level_select"),
      window_point: WindowPoint::new(12.0, 34.0),
      delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      confirmation: BlindSelectConfirmation::NotRequested,
    };
    let skip = BlindSkipResult {
      target: "Balatro".to_string(),
      selected_button: button("button_level_skip"),
      window_point: WindowPoint::new(12.0, 34.0),
      delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      confirmation: BlindSkipConfirmation::NotRequested,
    };

    for value in [
      serde_json::to_value(select).expect("serialize blind-select result"),
      serde_json::to_value(skip).expect("serialize blind-skip result"),
    ] {
      assert!(value.get("delivery").is_some());
      assert!(value.get("confirmation").is_some());
      assert!(value.get("verification").is_none());
      assert!(value.get("before_image").is_none());
      assert!(value.get("after_image").is_none());
    }
  }
}

#[cfg(all(test, feature = "tracing"))]
mod tracing_tests {
  use std::sync::Arc;

  use auv_driver::InputDeliveryPath;
  use auv_task_object_detection::BoundingBox;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;
  use crate::model::ObjectZone;

  fn button(id: &str) -> ButtonTarget {
    ButtonTarget {
      id: id.to_string(),
      label: id.to_string(),
      bbox: BoundingBox {
        x1: 1.0,
        y1: 2.0,
        x2: 3.0,
        y2: 4.0,
      },
      confidence: 0.95,
    }
  }

  #[test]
  fn completed_events_keep_domain_confirmation_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let select = BlindSelectResult {
        target: "Balatro".to_string(),
        slot: SlotId::new(ObjectZone::Blind, 1),
        selected_button: button("button_level_select"),
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        confirmation: BlindSelectConfirmation::Started {
          after_phase: BalatroPhase::Playing,
          hand_count: 5,
        },
      };
      let skip = BlindSkipResult {
        target: "Balatro".to_string(),
        selected_button: button("button_level_skip"),
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        confirmation: BlindSkipConfirmation::Exited {
          after_phase: BalatroPhase::Store,
        },
      };

      root.in_scope(|| {
        emit_blind_select_completed(&select);
        emit_blind_skip_completed(&skip);
      });
      dispatch.flush().await.expect("flush completed events");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("blind-action run");

      for name in [
        "auv.balatro.blind_select.completed",
        "auv.balatro.blind_skip.completed",
      ] {
        let event = snapshot.events().iter().find(|event| event.schema().name().as_str() == name).expect("blind completed event");
        assert!(event.payload().get().contains("\"confirmation\""));
        assert!(!event.payload().get().contains("\"delivery\""));
      }
    });
  }
}
