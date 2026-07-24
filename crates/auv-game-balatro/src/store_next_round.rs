use auv_driver::geometry::Point;
use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroPhase, BalatroState, ButtonTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreNextRoundConfirmationRequest {
  None,
  Targeted,
  Weak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreNextRoundRequest {
  pub target: String,
  pub confirmation: StoreNextRoundConfirmationRequest,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundTarget {
  DetectedButton { button: ButtonTarget },
  StoreLayout { frame_point: Point },
}

impl StoreNextRoundTarget {
  pub(crate) fn frame_point(&self) -> Point {
    match self {
      Self::DetectedButton { button } => {
        Point::new(f64::from((button.bbox.x1 + button.bbox.x2) / 2.0), f64::from((button.bbox.y1 + button.bbox.y2) / 2.0))
      }
      Self::StoreLayout { frame_point } => *frame_point,
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmationStrength {
  Targeted,
  Weak,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmationFailure {
  StoreOriginUnconfirmed,
  ExpectedBlindSelection,
  ExpectedKnownStoreExit,
  ObservationFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreNextRoundConfirmation {
  NotRequested,
  Confirmed {
    strength: StoreNextRoundConfirmationStrength,
    before_phase: BalatroPhase,
    after_phase: BalatroPhase,
  },
  NotConfirmed {
    requested: StoreNextRoundConfirmationStrength,
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: StoreNextRoundConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoreNextRoundResult {
  pub target: String,
  pub selected_target: StoreNextRoundTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: StoreNextRoundConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct StoreNextRoundCompleted<'a> {
  target: &'a str,
  selected_target: &'a StoreNextRoundTarget,
  window_point: WindowPoint,
  confirmation: &'a StoreNextRoundConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for StoreNextRoundCompleted<'_> {
  const NAME: &'static str = "auv.balatro.store_next_round.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_store_next_round_completed(result: &StoreNextRoundResult) {
  auv_tracing::emit_event!(StoreNextRoundCompleted {
    target: &result.target,
    selected_target: &result.selected_target,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_store_next_round_completed(_result: &StoreNextRoundResult) {}

pub(crate) fn evaluate_store_next_round_confirmation(
  request: StoreNextRoundConfirmationRequest,
  before: &BalatroState,
  after: Result<&BalatroState, String>,
) -> StoreNextRoundConfirmation {
  let requested = match request {
    StoreNextRoundConfirmationRequest::None => return StoreNextRoundConfirmation::NotRequested,
    StoreNextRoundConfirmationRequest::Targeted => StoreNextRoundConfirmationStrength::Targeted,
    StoreNextRoundConfirmationRequest::Weak => StoreNextRoundConfirmationStrength::Weak,
  };
  if before.phase != BalatroPhase::Store && !before.store.is_store {
    return StoreNextRoundConfirmation::NotConfirmed {
      requested,
      before_phase: before.phase,
      after_phase: None,
      reason: StoreNextRoundConfirmationFailure::StoreOriginUnconfirmed,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return StoreNextRoundConfirmation::NotConfirmed {
        requested,
        before_phase: before.phase,
        after_phase: None,
        reason: StoreNextRoundConfirmationFailure::ObservationFailed { message },
      };
    }
  };
  let confirmed = match requested {
    StoreNextRoundConfirmationStrength::Targeted => after.phase == BalatroPhase::BlindSelect,
    StoreNextRoundConfirmationStrength::Weak => !matches!(after.phase, BalatroPhase::Store | BalatroPhase::Unknown),
  };
  if confirmed {
    return StoreNextRoundConfirmation::Confirmed {
      strength: requested,
      before_phase: before.phase,
      after_phase: after.phase,
    };
  }
  StoreNextRoundConfirmation::NotConfirmed {
    requested,
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason: match requested {
      StoreNextRoundConfirmationStrength::Targeted => StoreNextRoundConfirmationFailure::ExpectedBlindSelection,
      StoreNextRoundConfirmationStrength::Weak => StoreNextRoundConfirmationFailure::ExpectedKnownStoreExit,
    },
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, FrameRef, RoundState, ScoreState, StoreState};

  fn state(phase: BalatroPhase) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "store-next-round-test.png".to_string(),
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
      store: StoreState {
        is_store: phase == BalatroPhase::Store,
        ..StoreState::default()
      },
      buttons: Vec::new(),
      diagnostics: Vec::new(),
      raw_entities: Vec::new(),
      raw_ui: Vec::new(),
    }
  }

  #[test]
  fn targeted_confirmation_requires_blind_selection() {
    let before = state(BalatroPhase::Store);
    let playing = state(BalatroPhase::Playing);

    assert!(matches!(
      evaluate_store_next_round_confirmation(StoreNextRoundConfirmationRequest::Targeted, &before, Ok(&playing)),
      StoreNextRoundConfirmation::NotConfirmed {
        reason: StoreNextRoundConfirmationFailure::ExpectedBlindSelection,
        ..
      }
    ));
  }

  #[test]
  fn weak_confirmation_accepts_a_known_store_exit_but_not_unknown() {
    let before = state(BalatroPhase::Store);
    let playing = state(BalatroPhase::Playing);
    let unknown = state(BalatroPhase::Unknown);

    assert!(matches!(
      evaluate_store_next_round_confirmation(StoreNextRoundConfirmationRequest::Weak, &before, Ok(&playing)),
      StoreNextRoundConfirmation::Confirmed {
        strength: StoreNextRoundConfirmationStrength::Weak,
        after_phase: BalatroPhase::Playing,
        ..
      }
    ));
    assert!(matches!(
      evaluate_store_next_round_confirmation(StoreNextRoundConfirmationRequest::Weak, &before, Ok(&unknown)),
      StoreNextRoundConfirmation::NotConfirmed {
        reason: StoreNextRoundConfirmationFailure::ExpectedKnownStoreExit,
        ..
      }
    ));
  }

  #[test]
  fn direct_result_keeps_delivery_and_confirmation_typed_without_generic_verification() {
    let result = StoreNextRoundResult {
      target: "Balatro".to_string(),
      selected_target: StoreNextRoundTarget::StoreLayout {
        frame_point: Point::new(12.0, 34.0),
      },
      window_point: WindowPoint::new(56.0, 78.0),
      delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      confirmation: StoreNextRoundConfirmation::NotRequested,
    };

    let value = serde_json::to_value(result).expect("serialize typed result");

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
  fn completed_event_records_domain_confirmation_without_embedding_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let result = StoreNextRoundResult {
        target: "Balatro".to_string(),
        selected_target: StoreNextRoundTarget::StoreLayout {
          frame_point: Point::new(12.0, 34.0),
        },
        window_point: WindowPoint::new(56.0, 78.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        confirmation: StoreNextRoundConfirmation::Confirmed {
          strength: StoreNextRoundConfirmationStrength::Targeted,
          before_phase: BalatroPhase::Store,
          after_phase: BalatroPhase::BlindSelect,
        },
      };

      root.in_scope(|| emit_store_next_round_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("store-next-round run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.store_next_round.completed")
        .expect("store-next-round completed event");

      assert!(event.payload().get().contains("\"confirmation\""));
      assert!(!event.payload().get().contains("\"delivery\""));
    });
  }
}
