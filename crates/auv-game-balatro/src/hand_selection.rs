use auv_driver::{InputActionResult, WindowPoint};
use serde::Serialize;

use crate::model::SlotId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandSelectionToggleKind {
  ClearUnexpected,
  SelectRequested,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HandSelectionToggle {
  pub kind: HandSelectionToggleKind,
  pub attempt: u8,
  pub slot: SlotId,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandSelectionState {
  Matched {
    selected: Vec<SlotId>,
  },
  NotMatched {
    selected: Vec<SlotId>,
  },
  Incomplete {
    last_selected: Vec<SlotId>,
    message: String,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HandSelectionResult {
  pub requested: Vec<SlotId>,
  pub toggles: Vec<HandSelectionToggle>,
  pub state: HandSelectionState,
}

impl HandSelectionResult {
  pub fn is_matched(&self) -> bool {
    matches!(self.state, HandSelectionState::Matched { .. })
  }
}
