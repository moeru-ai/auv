use std::fmt;

use auv_inference_common::{BoundingBox, Detection, ImageSize};
use serde::{Deserialize, Serialize};

pub const BALATRO_STATE_SCHEMA_VERSION: &str = "auv.game.balatro.state.v0";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BalatroPhase {
  Playing,
  Store,
  BlindSelect,
  GameOver,
  MainMenu,
  Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectZone {
  Hand,
  Joker,
  Consumable,
  Store,
  Button,
  Score,
  Round,
  Blind,
  Unknown,
}

impl ObjectZone {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Hand => "hand",
      Self::Joker => "joker",
      Self::Consumable => "consumable",
      Self::Store => "store",
      Self::Button => "button",
      Self::Score => "score",
      Self::Round => "round",
      Self::Blind => "blind",
      Self::Unknown => "unknown",
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SlotId {
  pub zone: ObjectZone,
  pub index: u32,
}

impl SlotId {
  pub fn new(zone: ObjectZone, index: u32) -> Self {
    Self { zone, index }
  }
}

impl fmt::Display for SlotId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "{}:{}", self.zone.as_str(), self.index)
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FrameRef {
  pub source: String,
  pub image_size: ImageSize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ObjectEvidence {
  pub model: String,
  pub detection: Detection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
  Unread,
  Cached,
  Read,
  NeedsRefresh,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Reading {
  pub status: ReadingStatus,
  pub text: Option<String>,
  pub confidence: Option<f32>,
}

impl Reading {
  pub fn unread() -> Self {
    Self {
      status: ReadingStatus::Unread,
      text: None,
      confidence: None,
    }
  }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CacheHint {
  pub needs_reading: bool,
  pub visual_fingerprint: Option<String>,
  pub changed_since_last_read: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CardSlot {
  pub slot: SlotId,
  pub kind: String,
  pub bbox: BoundingBox,
  pub confidence: f32,
  pub reading: Reading,
  pub cache: CacheHint,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JokerSlot {
  pub slot: SlotId,
  pub bbox: BoundingBox,
  pub confidence: f32,
  pub reading: Reading,
  pub cache: CacheHint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableKind {
  Tarot,
  Planet,
  Spectral,
  Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ConsumableSlot {
  pub slot: SlotId,
  pub kind: ConsumableKind,
  pub bbox: BoundingBox,
  pub confidence: f32,
  pub reading: Reading,
  pub cache: CacheHint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreItemKind {
  Joker,
  Tarot,
  Planet,
  Spectral,
  CardPack,
  PlayingCard,
  Voucher,
  Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StoreItem {
  pub slot: SlotId,
  pub kind: StoreItemKind,
  pub bbox: BoundingBox,
  pub confidence: f32,
  pub reading: Reading,
  pub cache: CacheHint,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScoreState {
  pub chips: Option<String>,
  pub mult: Option<String>,
  pub current_score: Option<String>,
  pub round_score: Option<String>,
  pub target_score: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoundState {
  pub cash: Option<String>,
  pub hands_left: Option<String>,
  pub discards_left: Option<String>,
  pub ante_current: Option<String>,
  pub ante_left: Option<String>,
  pub round_current: Option<String>,
  pub round_left: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ButtonTarget {
  pub id: String,
  pub label: String,
  pub bbox: BoundingBox,
  pub confidence: f32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StoreState {
  pub is_store: bool,
  pub item_count: u32,
  pub can_reroll: bool,
  pub can_next_round: bool,
  pub items: Vec<StoreItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BalatroDiagnostic {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct BalatroState {
  pub schema_version: String,
  pub frame: FrameRef,
  pub phase: BalatroPhase,
  pub scores: ScoreState,
  pub rounds: RoundState,
  pub hand: Vec<CardSlot>,
  pub jokers: Vec<JokerSlot>,
  pub consumables: Vec<ConsumableSlot>,
  pub store: StoreState,
  pub buttons: Vec<ButtonTarget>,
  pub diagnostics: Vec<BalatroDiagnostic>,
  pub raw_entities: Vec<ObjectEvidence>,
  pub raw_ui: Vec<ObjectEvidence>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn slot_id_formats_zone_and_index() {
    assert_eq!(SlotId::new(ObjectZone::Hand, 3).to_string(), "hand:3");
    assert_eq!(SlotId::new(ObjectZone::Store, 1).to_string(), "store:1");
  }

  #[test]
  fn phase_serializes_as_snake_case() {
    assert_eq!(serde_json::to_string(&BalatroPhase::Store).unwrap(), "\"store\"");
    assert_eq!(serde_json::to_string(&BalatroPhase::GameOver).unwrap(), "\"game_over\"");
  }
}
