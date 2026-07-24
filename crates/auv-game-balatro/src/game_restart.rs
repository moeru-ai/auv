use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroPhase, BalatroState, ButtonTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameRestartRequest {
  pub target: String,
  pub confirm_started: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameRestartTarget {
  DetectedButton { button: ButtonTarget },
  GameOverLayout,
  LocalizedTitleLayout,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GameRestartClick {
  pub target: GameRestartTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameRestartOutcome {
  NotChecked,
  Started { phase: BalatroPhase },
  NotStarted { phase: BalatroPhase },
  Incomplete { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GameRestartResult {
  pub target: String,
  pub clicks: Vec<GameRestartClick>,
  pub outcome: GameRestartOutcome,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct GameRestartClickFact<'a> {
  target: &'a GameRestartTarget,
  window_point: WindowPoint,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct GameRestartCompleted<'a> {
  target: &'a str,
  clicks: Vec<GameRestartClickFact<'a>>,
  outcome: &'a GameRestartOutcome,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for GameRestartCompleted<'_> {
  const NAME: &'static str = "auv.balatro.game_restart.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_game_restart_completed(result: &GameRestartResult) {
  auv_tracing::emit_event!(GameRestartCompleted {
    target: &result.target,
    clicks: result
      .clicks
      .iter()
      .map(|click| GameRestartClickFact {
        target: &click.target,
        window_point: click.window_point,
      })
      .collect(),
    outcome: &result.outcome,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_game_restart_completed(_result: &GameRestartResult) {}

pub(crate) fn classify_game_restart_outcome(after: Result<&BalatroState, String>) -> GameRestartOutcome {
  let after = match after {
    Ok(after) => after,
    Err(message) => return GameRestartOutcome::Incomplete { message },
  };
  if matches!(after.phase, BalatroPhase::BlindSelect | BalatroPhase::Playing | BalatroPhase::Store) {
    GameRestartOutcome::Started { phase: after.phase }
  } else {
    GameRestartOutcome::NotStarted { phase: after.phase }
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
        source: "game-restart-test.png".to_string(),
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
  fn unknown_phase_is_not_a_success_and_read_failure_is_incomplete() {
    assert_eq!(
      classify_game_restart_outcome(Ok(&state(BalatroPhase::Unknown))),
      GameRestartOutcome::NotStarted {
        phase: BalatroPhase::Unknown,
      }
    );
    assert!(matches!(classify_game_restart_outcome(Err("capture failed".to_string())), GameRestartOutcome::Incomplete { .. }));
  }

  #[test]
  fn result_exposes_every_delivery_without_verification_retry_fields() {
    let result = GameRestartResult {
      target: "Balatro".to_string(),
      clicks: vec![GameRestartClick {
        target: GameRestartTarget::GameOverLayout,
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      }],
      outcome: GameRestartOutcome::NotChecked,
    };

    let value = serde_json::to_value(result).expect("serialize game-restart result");

    assert!(value["clicks"][0].get("delivery").is_some());
    assert!(value.get("verification").is_none());
    assert!(!value.to_string().contains("verification_retry"));
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
  fn completed_event_records_click_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let result = GameRestartResult {
        target: "Balatro".to_string(),
        clicks: vec![GameRestartClick {
          target: GameRestartTarget::GameOverLayout,
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        }],
        outcome: GameRestartOutcome::Started {
          phase: BalatroPhase::BlindSelect,
        },
      };

      root.in_scope(|| emit_game_restart_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("game-restart run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.game_restart.completed")
        .expect("game-restart completed event");

      assert!(event.payload().get().contains("\"clicks\""));
      assert!(event.payload().get().contains("\"outcome\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
