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

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;

  use super::*;
  use crate::model::ObjectZone;

  #[test]
  fn incomplete_selection_retains_prior_deliveries_without_a_passed_boolean() {
    let slot = SlotId::new(ObjectZone::Hand, 1);
    let result = HandSelectionResult {
      requested: vec![slot],
      toggles: vec![HandSelectionToggle {
        kind: HandSelectionToggleKind::SelectRequested,
        attempt: 1,
        slot,
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      }],
      state: HandSelectionState::Incomplete {
        last_selected: Vec::new(),
        message: "state read failed".to_string(),
      },
    };

    let value = serde_json::to_value(result).expect("serialize hand selection");

    assert!(value["toggles"][0].get("delivery").is_some());
    assert!(value.get("passed").is_none());
    assert!(value.get("verification").is_none());
    assert!(value.get("selected_image").is_none());
  }
}
