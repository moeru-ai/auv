use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::hand_selection::HandSelectionResult;
#[cfg(feature = "tracing")]
use crate::hand_selection::HandSelectionState;
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
