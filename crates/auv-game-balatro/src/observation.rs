use std::path::Path;

use auv_inference_common::{ImageSize, InferenceError};
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
use image::RgbImage;
use thiserror::Error;

use crate::cache::cache_hint_for_detection;
use crate::config::BalatroModelConfig;
use crate::detector::{BalatroDetectionSets, BalatroDetectors};
use crate::model::{
  BALATRO_STATE_SCHEMA_VERSION, BalatroDiagnostic, BalatroPhase, BalatroState, ButtonTarget, CacheHint, CardSlot, ConsumableKind,
  ConsumableSlot, FrameRef, JokerSlot, ObjectEvidence, ObjectZone, Reading, RoundState, ScoreState, SlotId, StoreItem, StoreItemKind,
  StoreState,
};

#[derive(Debug, Error)]
pub enum ObservationError {
  #[error("inference error: {0}")]
  Inference(#[from] InferenceError),
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),
  #[error("image error: {0}")]
  Image(#[from] image::ImageError),
}

pub fn observe_image(image_path: impl AsRef<Path>, config: &BalatroModelConfig, no_cache: bool) -> Result<BalatroState, ObservationError> {
  let image_path = image_path.as_ref();
  let image = image::open(image_path)?.to_rgb8();
  let image_size = ImageSize {
    width: image.width(),
    height: image.height(),
  };
  let detectors = BalatroDetectors::load(config.clone())?;
  let detections = detectors.detect_path(image_path)?;

  Ok(build_state_from_detections(image_path.display().to_string(), image_size, &image, detections, no_cache))
}

pub fn build_state_from_detections(
  source: impl Into<String>,
  image_size: ImageSize,
  image: &RgbImage,
  detections: BalatroDetectionSets,
  no_cache: bool,
) -> BalatroState {
  let raw_entities = evidence_from_result(&detections.entities, "balatro-entities");
  let raw_ui = evidence_from_result(&detections.ui, "balatro-ui");
  let mut entity_detections = detections.entities.detections;
  let mut ui_detections = detections.ui.detections;

  entity_detections.sort_by(compare_left_to_right);
  ui_detections.sort_by(compare_left_to_right);

  let hand = card_slots(
    entity_detections.iter().filter(|detection| matches_label(detection, &["poker_card_front", "poker_card_back"])),
    ObjectZone::Hand,
    image,
    no_cache,
  );
  let jokers = entity_detections
    .iter()
    .filter(|detection| detection.label == "joker_card")
    .enumerate()
    .map(|(index, detection)| JokerSlot {
      slot: SlotId::new(ObjectZone::Joker, index as u32),
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect();
  let consumables = entity_detections
    .iter()
    .filter_map(|detection| consumable_kind(&detection.label).map(|kind| (detection, kind)))
    .enumerate()
    .map(|(index, (detection, kind))| ConsumableSlot {
      slot: SlotId::new(ObjectZone::Consumable, index as u32),
      kind,
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect();
  let buttons: Vec<ButtonTarget> = ui_detections
    .iter()
    .filter(|detection| detection.label.starts_with("button_"))
    .map(|detection| ButtonTarget {
      id: detection.label.clone(),
      label: detection.label.strip_prefix("button_").unwrap_or(&detection.label).to_owned(),
      bbox: detection.bbox,
      confidence: detection.confidence,
    })
    .collect();
  let store = store_state(&entity_detections, &ui_detections, image, no_cache);
  let phase = infer_phase(&entity_detections, &ui_detections);
  let diagnostics = diagnostics_for_detections(&entity_detections, &ui_detections);

  BalatroState {
    schema_version: BALATRO_STATE_SCHEMA_VERSION.to_owned(),
    frame: FrameRef {
      source: source.into(),
      image_size,
    },
    phase,
    scores: ScoreState::default(),
    rounds: RoundState::default(),
    hand,
    jokers,
    consumables,
    store,
    buttons,
    diagnostics,
    raw_entities,
    raw_ui,
  }
}

fn diagnostics_for_detections(entities: &[Detection], ui: &[Detection]) -> Vec<BalatroDiagnostic> {
  if entities.is_empty() && ui.is_empty() {
    return vec![BalatroDiagnostic {
      code: "empty_detection_sets".to_string(),
      message: "Balatro detectors returned no entity or UI boxes for this frame".to_string(),
    }];
  }
  Vec::new()
}

fn evidence_from_result(result: &DetectionResult, model: &str) -> Vec<ObjectEvidence> {
  result
    .detections
    .iter()
    .cloned()
    .map(|detection| ObjectEvidence {
      model: model.to_owned(),
      detection,
    })
    .collect()
}

fn card_slots<'a>(detections: impl Iterator<Item = &'a Detection>, zone: ObjectZone, image: &RgbImage, no_cache: bool) -> Vec<CardSlot> {
  detections
    .enumerate()
    .map(|(index, detection)| CardSlot {
      slot: SlotId::new(zone, index as u32),
      kind: detection.label.clone(),
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect()
}

fn store_state(entities: &[Detection], ui: &[Detection], image: &RgbImage, no_cache: bool) -> StoreState {
  let can_reroll = ui.iter().any(|detection| detection.label == "button_store_reroll");
  let can_next_round = ui.iter().any(|detection| detection.label == "button_store_next_round");
  let is_store = can_reroll || can_next_round || ui.iter().any(is_store_control);
  let items = if is_store {
    store_items_for_store_context(entities, image, no_cache)
  } else {
    Vec::new()
  };
  let item_count = items.len() as u32;

  StoreState {
    is_store,
    item_count,
    can_reroll,
    can_next_round,
    items,
  }
}

fn store_items_for_store_context(entities: &[Detection], image: &RgbImage, no_cache: bool) -> Vec<StoreItem> {
  let mut items = entities
    .iter()
    .filter_map(|detection| store_item_kind(&detection.label).map(|kind| (detection, kind)))
    .filter(|(detection, kind)| is_store_item_candidate(detection, kind, image.width(), image.height()))
    .enumerate()
    .map(|(index, (detection, kind))| StoreItem {
      slot: SlotId::new(ObjectZone::Store, index as u32),
      kind,
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect::<Vec<_>>();

  append_voucher_layout_candidate(&mut items, image.width(), image.height());
  items
}

fn is_store_item_candidate(detection: &Detection, kind: &StoreItemKind, image_width: u32, image_height: u32) -> bool {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  let center_x = center_x(detection) / width;
  let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0 / height;

  let (max_x, min_y, max_y) = match kind {
    StoreItemKind::CardPack => (0.90, 0.22, 0.96),
    _ => (0.82, 0.32, 0.75),
  };

  // Thresholds are normalized from live Balatro store captures: keep store
  // products while excluding top owned joker/consumable rows and the visually
  // anchored right deck stack. Card packs can sit in the lower shop row at
  // smaller window scales, so their vertical band is intentionally taller than
  // ordinary card products.
  (0.20..=max_x).contains(&center_x) && (min_y..=max_y).contains(&center_y)
}

fn infer_phase(entities: &[Detection], ui: &[Detection]) -> BalatroPhase {
  if ui.iter().any(is_store_control) {
    BalatroPhase::Store
  } else if ui.iter().any(|detection| matches_label(detection, &["button_play", "button_discard"]))
    || (entities.iter().any(is_hand_card) && ui.iter().any(is_hand_sort_control))
  {
    BalatroPhase::Playing
  } else if ui.iter().any(|detection| {
    matches_label(
      detection,
      &[
        "button_select_blind",
        "button_skip_blind",
        "button_level_select",
      ],
    )
  }) {
    BalatroPhase::BlindSelect
  } else if ui.iter().any(|detection| detection.label == "button_new_run")
    && ui.iter().any(|detection| detection.label == "button_main_menu")
  {
    BalatroPhase::GameOver
  } else if ui.iter().any(|detection| {
    matches_label(
      detection,
      &[
        "button_main_menu_play",
        "button_new_run",
        "button_new_run_play",
      ],
    )
  }) {
    BalatroPhase::MainMenu
  } else {
    BalatroPhase::Unknown
  }
}

fn is_hand_card(detection: &Detection) -> bool {
  matches_label(detection, &["poker_card_front", "poker_card_back"])
}

fn is_hand_sort_control(detection: &Detection) -> bool {
  matches_label(detection, &["button_sort_hand_rank", "button_sort_hand_suits"])
}

fn is_store_control(detection: &Detection) -> bool {
  matches_label(
    detection,
    &[
      "button_store_reroll",
      "button_store_next_round",
      "button_purchase",
    ],
  )
}

fn consumable_kind(label: &str) -> Option<ConsumableKind> {
  match label {
    "tarot_card" => Some(ConsumableKind::Tarot),
    "planet_card" => Some(ConsumableKind::Planet),
    "spectral_card" => Some(ConsumableKind::Spectral),
    _ => None,
  }
}

fn store_item_kind(label: &str) -> Option<StoreItemKind> {
  match label {
    "joker_card" => Some(StoreItemKind::Joker),
    "tarot_card" => Some(StoreItemKind::Tarot),
    "planet_card" => Some(StoreItemKind::Planet),
    "spectral_card" => Some(StoreItemKind::Spectral),
    "card_pack" => Some(StoreItemKind::CardPack),
    "poker_card_front" => Some(StoreItemKind::PlayingCard),
    // TODO(voucher-detection): detector-backed voucher labels are deferred
    // until the entities dataset grows that class. Store observation currently
    // adds a low-confidence layout fallback candidate instead.
    _ => None,
  }
}

fn append_voucher_layout_candidate(items: &mut Vec<StoreItem>, image_width: u32, image_height: u32) {
  if image_width < 600 || image_height < 400 {
    return;
  }
  if !items.iter().any(|item| item.kind != StoreItemKind::CardPack) {
    return;
  }
  let bbox = voucher_layout_bbox(image_width, image_height);
  if items.iter().any(|item| bbox_overlap_ratio(item.bbox, bbox) > 0.25) {
    return;
  }
  let slot = SlotId::new(ObjectZone::Store, items.len() as u32);
  items.push(StoreItem {
    slot,
    kind: StoreItemKind::Voucher,
    bbox,
    confidence: 0.35,
    reading: Reading::unread(),
    cache: CacheHint {
      needs_reading: true,
      visual_fingerprint: None,
      changed_since_last_read: false,
    },
  });
}

fn voucher_layout_bbox(image_width: u32, image_height: u32) -> BoundingBox {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  BoundingBox {
    x1: width * 0.22,
    y1: height * 0.58,
    x2: width * 0.39,
    y2: height * 0.86,
  }
}

fn bbox_overlap_ratio(left: BoundingBox, right: BoundingBox) -> f32 {
  let x1 = left.x1.max(right.x1);
  let y1 = left.y1.max(right.y1);
  let x2 = left.x2.min(right.x2);
  let y2 = left.y2.min(right.y2);
  let overlap_width = (x2 - x1).max(0.0);
  let overlap_height = (y2 - y1).max(0.0);
  let overlap_area = overlap_width * overlap_height;
  let right_area = ((right.x2 - right.x1).max(0.0) * (right.y2 - right.y1).max(0.0)).max(1.0);
  overlap_area / right_area
}

fn matches_label(detection: &Detection, labels: &[&str]) -> bool {
  labels.iter().any(|label| detection.label == *label)
}

fn compare_left_to_right(left: &Detection, right: &Detection) -> std::cmp::Ordering {
  center_x(left).partial_cmp(&center_x(right)).unwrap_or(std::cmp::Ordering::Equal)
}

fn center_x(detection: &Detection) -> f32 {
  (detection.bbox.x1 + detection.bbox.x2) / 2.0
}

#[cfg(test)]
#[path = "observation_test.rs"]
mod tests;
