use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::hand_selection::{HandSelectionResult, HandSelectionState};
use crate::model::{BalatroPhase, BalatroState, ButtonTarget, SlotId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardsSelectRequest {
  pub target: String,
  pub slots: Vec<SlotId>,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardsSelectResult {
  pub target: String,
  pub selection: HandSelectionResult,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitKind {
  Play,
  Discard,
}

impl CardCommitKind {
  pub(crate) fn button_id(self) -> &'static str {
    match self {
      Self::Play => "button_play",
      Self::Discard => "button_discard",
    }
  }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardCommitRequest {
  pub target: String,
  pub slots: Vec<SlotId>,
  pub confirm_change: bool,
  pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitAction {
  SelectHandTargets {
    selection: HandSelectionResult,
  },
  Submit {
    button: ButtonTarget,
    window_point: WindowPoint,
    delivery: InputActionResult,
  },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitChange {
  PhaseChanged,
  HandCountChanged,
  HandFingerprintsChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CardCommitChanges {
  first: CardCommitChange,
  additional: Vec<CardCommitChange>,
}

impl CardCommitChanges {
  fn from_observed(changes: Vec<CardCommitChange>) -> Option<Self> {
    let mut changes = changes.into_iter();
    let first = changes.next()?;
    Some(Self {
      first,
      additional: changes.collect(),
    })
  }

  pub fn iter(&self) -> impl Iterator<Item = CardCommitChange> + '_ {
    std::iter::once(self.first).chain(self.additional.iter().copied())
  }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitConfirmationFailure {
  OriginNotPlaying,
  NoHandStateChange,
  StateReadFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitConfirmation {
  NotRequested,
  Applied {
    changes: CardCommitChanges,
  },
  NotConfirmed {
    reason: CardCommitConfirmationFailure,
  },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitStop {
  HandTargetsNotReady,
  CommitControlNotFound { message: String },
  SubmissionFailed { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitState {
  Stopped {
    reason: CardCommitStop,
  },
  Submitted {
    confirmation: CardCommitConfirmation,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardCommitResult {
  pub target: String,
  pub kind: CardCommitKind,
  pub requested_slots: Vec<SlotId>,
  pub actions: Vec<CardCommitAction>,
  pub state: CardCommitState,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct CardsSelectCompleted<'a> {
  target: &'a str,
  requested: &'a [SlotId],
  result: &'a HandSelectionState,
  toggle_count: usize,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for CardsSelectCompleted<'_> {
  const NAME: &'static str = "auv.balatro.cards_select.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_cards_select_completed(result: &CardsSelectResult) {
  auv_tracing::emit_event!(CardsSelectCompleted {
    target: &result.target,
    requested: &result.selection.requested,
    result: &result.selection.state,
    toggle_count: result.selection.toggles.len(),
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_cards_select_completed(_result: &CardsSelectResult) {}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CardCommitActionFact<'a> {
  SelectHandTargets {
    requested: &'a [SlotId],
    result: &'a HandSelectionState,
    toggle_count: usize,
  },
  Submit {
    button: &'a ButtonTarget,
    window_point: WindowPoint,
  },
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct CardCommitCompleted<'a> {
  target: &'a str,
  kind: CardCommitKind,
  requested_slots: &'a [SlotId],
  actions: Vec<CardCommitActionFact<'a>>,
  state: &'a CardCommitState,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for CardCommitCompleted<'_> {
  const NAME: &'static str = "auv.balatro.card_commit.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_card_commit_completed(result: &CardCommitResult) {
  let actions = result
    .actions
    .iter()
    .map(|action| match action {
      CardCommitAction::SelectHandTargets { selection } => CardCommitActionFact::SelectHandTargets {
        requested: &selection.requested,
        result: &selection.state,
        toggle_count: selection.toggles.len(),
      },
      CardCommitAction::Submit {
        button,
        window_point,
        ..
      } => CardCommitActionFact::Submit {
        button,
        window_point: *window_point,
      },
    })
    .collect();
  auv_tracing::emit_event!(CardCommitCompleted {
    target: &result.target,
    kind: result.kind,
    requested_slots: &result.requested_slots,
    actions,
    state: &result.state,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_card_commit_completed(_result: &CardCommitResult) {}

pub(crate) fn evaluate_card_commit_confirmation(before: &BalatroState, after: Result<&BalatroState, String>) -> CardCommitConfirmation {
  if before.phase != BalatroPhase::Playing {
    return CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::OriginNotPlaying,
    };
  }
  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return CardCommitConfirmation::NotConfirmed {
        reason: CardCommitConfirmationFailure::StateReadFailed { message },
      };
    }
  };
  let mut changes = Vec::new();
  if after.phase != before.phase {
    changes.push(CardCommitChange::PhaseChanged);
  }
  if after.hand.len() != before.hand.len() {
    changes.push(CardCommitChange::HandCountChanged);
  }
  if hand_fingerprints_changed(before, after) {
    changes.push(CardCommitChange::HandFingerprintsChanged);
  }
  match CardCommitChanges::from_observed(changes) {
    Some(changes) => CardCommitConfirmation::Applied { changes },
    None => CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::NoHandStateChange,
    },
  }
}

fn hand_fingerprints_changed(before: &BalatroState, after: &BalatroState) -> bool {
  let before = before.hand.iter().filter_map(|card| card.cache.visual_fingerprint.as_deref()).collect::<Vec<_>>();
  let after = after.hand.iter().filter_map(|card| card.cache.visual_fingerprint.as_deref()).collect::<Vec<_>>();
  !before.is_empty() && !after.is_empty() && before != after
}

#[cfg(test)]
mod tests {
  use auv_driver::InputDeliveryPath;
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, CacheHint, CardSlot, FrameRef, ObjectZone, Reading, RoundState, ScoreState, StoreState};

  fn state(phase: BalatroPhase, fingerprints: &[&str]) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "card-commit-test.png".to_string(),
        image_size: ImageSize {
          width: 1600,
          height: 960,
        },
      },
      phase,
      scores: ScoreState::default(),
      rounds: RoundState::default(),
      hand: fingerprints
        .iter()
        .enumerate()
        .map(|(index, fingerprint)| CardSlot {
          slot: SlotId::new(ObjectZone::Hand, index as u32),
          kind: "poker_card_front".to_string(),
          bbox: BoundingBox {
            x1: index as f32 * 20.0,
            y1: 2.0,
            x2: index as f32 * 20.0 + 10.0,
            y2: 20.0,
          },
          confidence: 0.95,
          reading: Reading::unread(),
          cache: CacheHint {
            visual_fingerprint: Some((*fingerprint).to_string()),
            ..CacheHint::default()
          },
        })
        .collect(),
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
  fn changed_fingerprints_confirm_commit_without_generic_evidence_strings() {
    let before = state(BalatroPhase::Playing, &["a", "b"]);
    let after = state(BalatroPhase::Playing, &["c", "d"]);
    let CardCommitConfirmation::Applied { changes } = evaluate_card_commit_confirmation(&before, Ok(&after)) else {
      panic!("changed hand should confirm commit");
    };

    assert_eq!(changes.iter().collect::<Vec<_>>(), vec![CardCommitChange::HandFingerprintsChanged]);
  }

  #[test]
  fn stopped_result_retains_selection_delivery_without_generic_verification() {
    let slot = SlotId::new(ObjectZone::Hand, 0);
    let result = CardCommitResult {
      target: "Balatro".to_string(),
      kind: CardCommitKind::Play,
      requested_slots: vec![slot],
      actions: vec![CardCommitAction::SelectHandTargets {
        selection: HandSelectionResult {
          requested: vec![slot],
          toggles: vec![crate::HandSelectionToggle {
            kind: crate::HandSelectionToggleKind::SelectRequested,
            attempt: 1,
            slot,
            window_point: WindowPoint::new(12.0, 34.0),
            delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
          }],
          state: HandSelectionState::NotMatched {
            selected: Vec::new(),
          },
        },
      }],
      state: CardCommitState::Stopped {
        reason: CardCommitStop::HandTargetsNotReady,
      },
    };

    let value = serde_json::to_value(result).expect("serialize card-commit result");

    assert!(value["actions"][0]["select_hand_targets"]["selection"]["toggles"][0].get("delivery").is_some());
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

  #[test]
  fn completed_event_projects_action_facts_without_copying_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let slot = SlotId::new(crate::ObjectZone::Hand, 0);
      let result = CardCommitResult {
        target: "Balatro".to_string(),
        kind: CardCommitKind::Discard,
        requested_slots: vec![slot],
        actions: vec![CardCommitAction::Submit {
          button: ButtonTarget {
            id: "button_discard".to_string(),
            label: "button_discard".to_string(),
            bbox: BoundingBox {
              x1: 1.0,
              y1: 2.0,
              x2: 3.0,
              y2: 4.0,
            },
            confidence: 0.95,
          },
          window_point: WindowPoint::new(12.0, 34.0),
          delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        }],
        state: CardCommitState::Submitted {
          confirmation: CardCommitConfirmation::NotRequested,
        },
      };

      root.in_scope(|| emit_card_commit_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("card-commit run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.card_commit.completed")
        .expect("card-commit completed event");

      assert!(event.payload().get().contains("\"submit\""));
      assert!(!event.payload().get().contains("\"delivery\""));
      assert!(!event.payload().get().contains("verification"));
    });
  }
}
