use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::hand_selection::HandSelectionResult;
#[cfg(feature = "tracing")]
use crate::hand_selection::HandSelectionState;
use crate::model::{ActionPoint, BalatroPhase, BalatroState, ButtonTarget, SlotId};

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
    control: CardCommitControl,
    point: ActionPoint,
    delivery: InputActionResult,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CardCommitControl {
  DetectedButton {
    button: ButtonTarget,
  },
  PlayingHandLayout {
    sort_rank: ButtonTarget,
    sort_suits: ButtonTarget,
  },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardCommitChange {
  PhaseChanged,
  HandCountChanged,
  RoundScoreChanged,
  HandsLeftChanged,
  DiscardsLeftChanged,
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
  UnstableVisualChange,
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
    control: &'a CardCommitControl,
    point: &'a ActionPoint,
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
  const VERSION: u32 = 3;
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
      CardCommitAction::Submit { control, point, .. } => CardCommitActionFact::Submit { control, point },
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
  if observed_value_changed(&before.scores.round_score, &after.scores.round_score) {
    changes.push(CardCommitChange::RoundScoreChanged);
  }
  if observed_value_changed(&before.rounds.hands_left, &after.rounds.hands_left) {
    changes.push(CardCommitChange::HandsLeftChanged);
  }
  if observed_value_changed(&before.rounds.discards_left, &after.rounds.discards_left) {
    changes.push(CardCommitChange::DiscardsLeftChanged);
  }
  let had_commit_control = before.buttons.iter().any(|button| matches!(button.id.as_str(), "button_play" | "button_discard"));
  let commit_control_cleared = !after.buttons.iter().any(|button| matches!(button.id.as_str(), "button_play" | "button_discard"));
  if hand_fingerprints_changed(before, after) && had_commit_control && commit_control_cleared {
    changes.push(CardCommitChange::HandFingerprintsChanged);
  }
  match CardCommitChanges::from_observed(changes) {
    Some(changes) => CardCommitConfirmation::Applied { changes },
    None => CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::NoHandStateChange,
    },
  }
}

pub(crate) fn card_commit_confirmation_has_structural_change(confirmation: &CardCommitConfirmation) -> bool {
  matches!(
    confirmation,
    CardCommitConfirmation::Applied { changes }
      if changes.iter().any(|change| !matches!(change, CardCommitChange::HandFingerprintsChanged))
  )
}

pub(crate) fn card_commit_confirmation_allows_resubmit(confirmation: &CardCommitConfirmation) -> bool {
  matches!(
    confirmation,
    CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::NoHandStateChange,
    }
  )
}

fn observed_value_changed(before: &Option<String>, after: &Option<String>) -> bool {
  matches!((before, after), (Some(before), Some(after)) if before != after)
}

pub(crate) fn reject_fingerprint_only_confirmation(confirmation: CardCommitConfirmation) -> CardCommitConfirmation {
  if matches!(&confirmation, CardCommitConfirmation::Applied { .. }) && !card_commit_confirmation_has_structural_change(&confirmation) {
    CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::UnstableVisualChange,
    }
  } else {
    confirmation
  }
}

fn hand_fingerprints_changed(before: &BalatroState, after: &BalatroState) -> bool {
  let before = before.hand.iter().filter_map(|card| card.cache.visual_fingerprint.as_deref()).collect::<Vec<_>>();
  let after = after.hand.iter().filter_map(|card| card.cache.visual_fingerprint.as_deref()).collect::<Vec<_>>();
  !before.is_empty() && !after.is_empty() && before != after
}

#[cfg(test)]
mod tests {
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::BoundingBox;

  use super::*;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, CacheHint, CardSlot, FrameRef, ObjectZone, Reading, RoundState, ScoreState, StoreState};

  #[test]
  fn selection_only_fingerprint_change_does_not_confirm_submission() {
    // ROOT CAUSE:
    //
    // If submission confirmation used the pre-selection frame as its baseline,
    // raising a selected card changed its fingerprint and falsely proved Play.
    //
    // Before the fix, card_commit passed the pre-selection state here.
    // The fix compares the after frame with the fully selected state.
    let pre_selection = playing_state("lowered-card");
    let selected = playing_state("raised-card");

    assert_eq!(
      evaluate_card_commit_confirmation(&pre_selection, Ok(&selected)),
      CardCommitConfirmation::NotConfirmed {
        reason: CardCommitConfirmationFailure::NoHandStateChange,
      }
    );
    assert_eq!(
      evaluate_card_commit_confirmation(&selected, Ok(&selected)),
      CardCommitConfirmation::NotConfirmed {
        reason: CardCommitConfirmationFailure::NoHandStateChange,
      }
    );
  }

  #[test]
  fn fingerprint_only_confirmation_is_not_structural() {
    // ROOT CAUSE:
    //
    // If one UI detection missed the still-active Play button, normal detector
    // box jitter changed card fingerprints and falsely proved submission.
    // Fingerprint-only evidence therefore needs repeated observation at the
    // command layer; phase and hand-count changes remain immediately strong.
    let fingerprint_only = CardCommitConfirmation::Applied {
      changes: CardCommitChanges::from_observed(vec![CardCommitChange::HandFingerprintsChanged]).unwrap(),
    };
    let phase_change = CardCommitConfirmation::Applied {
      changes: CardCommitChanges::from_observed(vec![CardCommitChange::PhaseChanged]).unwrap(),
    };

    assert!(!card_commit_confirmation_has_structural_change(&fingerprint_only));
    assert!(!card_commit_confirmation_allows_resubmit(&fingerprint_only));
    assert!(card_commit_confirmation_has_structural_change(&phase_change));
    assert_eq!(
      reject_fingerprint_only_confirmation(fingerprint_only),
      CardCommitConfirmation::NotConfirmed {
        reason: CardCommitConfirmationFailure::UnstableVisualChange,
      }
    );
  }

  #[test]
  fn submission_is_retried_only_when_nothing_changed() {
    // ROOT CAUSE:
    //
    // If the first post-click frame landed during Balatro's scoring animation,
    // card fingerprints changed before score OCR became stable. Treating that
    // partial evidence like no response clicked Play a second time.
    //
    // Before the fix, every non-structural confirmation allowed resubmission.
    // The fix retries only when the observed frame contains no state change.
    assert!(card_commit_confirmation_allows_resubmit(&CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::NoHandStateChange,
    }));
    assert!(!card_commit_confirmation_allows_resubmit(&CardCommitConfirmation::NotConfirmed {
      reason: CardCommitConfirmationFailure::StateReadFailed {
        message: "capture failed".to_string(),
      },
    }));
  }

  fn playing_state(fingerprint: &str) -> BalatroState {
    BalatroState {
      schema_version: BALATRO_STATE_SCHEMA_VERSION.to_string(),
      frame: FrameRef {
        source: "test://frame".to_string(),
        image_size: ImageSize {
          width: 100,
          height: 100,
        },
      },
      phase: BalatroPhase::Playing,
      scores: ScoreState::default(),
      rounds: RoundState::default(),
      hand: vec![CardSlot {
        slot: SlotId::new(ObjectZone::Hand, 0),
        kind: "poker_card_front".to_string(),
        bbox: BoundingBox {
          x1: 10.0,
          y1: 10.0,
          x2: 30.0,
          y2: 50.0,
        },
        confidence: 1.0,
        reading: Reading::unread(),
        attributes: Default::default(),
        cache: CacheHint {
          visual_fingerprint: Some(fingerprint.to_string()),
          ..CacheHint::default()
        },
      }],
      jokers: Vec::new(),
      consumables: Vec::new(),
      store: StoreState::default(),
      buttons: Vec::new(),
      diagnostics: Vec::new(),
      raw_entities: Vec::new(),
      raw_ui: Vec::new(),
    }
  }
}
