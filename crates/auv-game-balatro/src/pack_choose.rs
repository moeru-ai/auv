use std::fmt;

use auv_driver::{InputActionResult, WindowPoint};
use auv_task_object_detection::BoundingBox;
use serde::{Deserialize, Serialize};

use crate::hand_selection::{HandSelectionResult, HandSelectionState};
use crate::model::{ButtonTarget, SlotId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PackChoiceId(u32);

impl PackChoiceId {
  pub fn new(index: u32) -> Self {
    Self(index)
  }

  pub fn index(self) -> u32 {
    self.0
  }
}

impl fmt::Display for PackChoiceId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "pack:{}", self.0)
  }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PackChoice {
  pub id: PackChoiceId,
  pub detector_label: String,
  pub bbox: BoundingBox,
  pub confidence: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackChooseRequest {
  pub target: String,
  pub choice: PackChoiceId,
  pub hand_targets: Vec<SlotId>,
  pub confirm_applied: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseControl {
  DetectedButton { button: ButtonTarget },
  ActivePackLayout,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseAction {
  SelectChoice {
    window_point: WindowPoint,
    delivery: InputActionResult,
  },
  SelectHandTargets {
    selection: HandSelectionResult,
  },
  SubmitChoice {
    control: PackChooseControl,
    window_point: WindowPoint,
    delivery: InputActionResult,
  },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseConfirmationBasis {
  ChoiceCountDecreased,
  PackClosed,
  ChoiceCountDecreasedAndPackClosed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseConfirmationFailure {
  NoPackStateChange,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseConfirmation {
  NotRequested,
  Applied {
    basis: PackChooseConfirmationBasis,
    before_choice_count: usize,
    after_choice_count: usize,
  },
  NotConfirmed {
    reason: PackChooseConfirmationFailure,
    before_choice_count: usize,
    after_choice_count: Option<usize>,
  },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseStop {
  ChoiceSelectionFailed { message: String },
  SelectedStateReadFailed { message: String },
  HandTargetsNotReady,
  ConfirmControlNotFound { message: String },
  SubmissionFailed { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackChooseState {
  Stopped {
    reason: PackChooseStop,
  },
  Submitted {
    confirmation: PackChooseConfirmation,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PackChooseResult {
  pub target: String,
  pub choice: PackChoice,
  pub choice_was_already_selected: bool,
  pub actions: Vec<PackChooseAction>,
  pub state: PackChooseState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObservedPackState {
  pub choice_count: usize,
  pub skip_control_present: bool,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum PackChooseActionFact<'a> {
  SelectChoice {
    window_point: WindowPoint,
  },
  SelectHandTargets {
    requested: &'a [SlotId],
    result: &'a HandSelectionState,
    toggle_count: usize,
  },
  SubmitChoice {
    control: &'a PackChooseControl,
    window_point: WindowPoint,
  },
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct PackChooseCompleted<'a> {
  target: &'a str,
  choice: &'a PackChoice,
  choice_was_already_selected: bool,
  actions: Vec<PackChooseActionFact<'a>>,
  state: &'a PackChooseState,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for PackChooseCompleted<'_> {
  const NAME: &'static str = "auv.balatro.pack_choose.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_pack_choose_completed(result: &PackChooseResult) {
  let actions = result
    .actions
    .iter()
    .map(|action| match action {
      PackChooseAction::SelectChoice { window_point, .. } => PackChooseActionFact::SelectChoice {
        window_point: *window_point,
      },
      PackChooseAction::SelectHandTargets { selection } => PackChooseActionFact::SelectHandTargets {
        requested: &selection.requested,
        result: &selection.state,
        toggle_count: selection.toggles.len(),
      },
      PackChooseAction::SubmitChoice {
        control,
        window_point,
        ..
      } => PackChooseActionFact::SubmitChoice {
        control,
        window_point: *window_point,
      },
    })
    .collect();
  auv_tracing::emit_event!(PackChooseCompleted {
    target: &result.target,
    choice: &result.choice,
    choice_was_already_selected: result.choice_was_already_selected,
    actions,
    state: &result.state,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_pack_choose_completed(_result: &PackChooseResult) {}

pub(crate) fn evaluate_pack_choose_confirmation(
  before_choice_count: usize,
  after: Result<ObservedPackState, String>,
) -> PackChooseConfirmation {
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return PackChooseConfirmation::NotConfirmed {
        reason: PackChooseConfirmationFailure::StateReadFailed { message },
        before_choice_count,
        after_choice_count: None,
      };
    }
  };
  let choice_count_decreased = after.choice_count < before_choice_count;
  let pack_closed = !after.skip_control_present;
  let basis = match (choice_count_decreased, pack_closed) {
    (true, true) => Some(PackChooseConfirmationBasis::ChoiceCountDecreasedAndPackClosed),
    (true, false) => Some(PackChooseConfirmationBasis::ChoiceCountDecreased),
    (false, true) => Some(PackChooseConfirmationBasis::PackClosed),
    (false, false) => None,
  };
  match basis {
    Some(basis) => PackChooseConfirmation::Applied {
      basis,
      before_choice_count,
      after_choice_count: after.choice_count,
    },
    None => PackChooseConfirmation::NotConfirmed {
      reason: PackChooseConfirmationFailure::NoPackStateChange,
      before_choice_count,
      after_choice_count: Some(after.choice_count),
    },
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;

  use super::*;

  #[test]
  fn unchanged_open_pack_is_not_confirmed() {
    assert_eq!(
      evaluate_pack_choose_confirmation(
        3,
        Ok(ObservedPackState {
          choice_count: 3,
          skip_control_present: true,
        }),
      ),
      PackChooseConfirmation::NotConfirmed {
        reason: PackChooseConfirmationFailure::NoPackStateChange,
        before_choice_count: 3,
        after_choice_count: Some(3),
      }
    );
  }

  #[test]
  fn stopped_result_retains_choice_delivery_without_generic_verification() {
    let result = PackChooseResult {
      target: "Balatro".to_string(),
      choice: PackChoice {
        id: PackChoiceId::new(1),
        detector_label: "tarot_card".to_string(),
        bbox: BoundingBox {
          x1: 1.0,
          y1: 2.0,
          x2: 3.0,
          y2: 4.0,
        },
        confidence: 0.95,
      },
      choice_was_already_selected: false,
      actions: vec![PackChooseAction::SelectChoice {
        window_point: WindowPoint::new(12.0, 34.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      }],
      state: PackChooseState::Stopped {
        reason: PackChooseStop::SelectedStateReadFailed {
          message: "capture failed".to_string(),
        },
      },
    };

    let value = serde_json::to_value(result).expect("serialize pack-choose result");

    assert!(value["actions"][0]["select_choice"].get("delivery").is_some());
    assert!(value.get("verification").is_none());
    assert!(value.get("before_image").is_none());
    assert!(value.get("selected_image").is_none());
  }
}

#[cfg(all(test, feature = "tracing"))]
mod tracing_tests {
  use std::sync::Arc;

  use auv_driver::InputDeliveryPath;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;

  #[test]
  fn completed_event_projects_action_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let result = PackChooseResult {
        target: "Balatro".to_string(),
        choice: PackChoice {
          id: PackChoiceId::new(1),
          detector_label: "tarot_card".to_string(),
          bbox: BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
          confidence: 0.95,
        },
        choice_was_already_selected: false,
        actions: vec![PackChooseAction::SelectChoice {
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        }],
        state: PackChooseState::Stopped {
          reason: PackChooseStop::SelectedStateReadFailed {
            message: "capture failed".to_string(),
          },
        },
      };

      root.in_scope(|| emit_pack_choose_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("pack-choose run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.pack_choose.completed")
        .expect("pack-choose completed event");

      assert!(event.payload().get().contains("\"select_choice\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
