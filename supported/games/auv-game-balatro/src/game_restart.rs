use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::model::{ActionPoint, BalatroPhase, BalatroState, ButtonTarget};

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
  NewRunTabLayout,
  NewRunPlayLayout,
  GameOverLayout,
  LocalizedTitleLayout,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GameRestartClick {
  pub target: GameRestartTarget,
  pub point: ActionPoint,
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
  point: &'a ActionPoint,
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
  const VERSION: u32 = 2;
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
        point: &click.point,
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
