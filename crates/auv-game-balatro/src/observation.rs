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
mod tests {
  use auv_inference_common::ImageSize;
  use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
  use image::{Rgb, RgbImage};

  use super::*;
  use crate::cache::cache_hint_for_detection;
  use crate::detector::BalatroDetectionSets;
  use crate::model::{BALATRO_STATE_SCHEMA_VERSION, BalatroPhase, ConsumableKind, ObjectZone, SlotId, StoreItemKind};

  #[test]
  fn synthetic_hand_cards_sort_left_to_right_with_slot_ids() {
    let image = test_image();
    let detections = detection_sets(
      vec![
        detection("poker_card_front", 90.0, 20.0, 120.0, 70.0),
        detection("poker_card_back", 10.0, 20.0, 40.0, 70.0),
      ],
      vec![],
    );

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.hand.len(), 2);
    assert_eq!(state.hand[0].slot, SlotId::new(ObjectZone::Hand, 0));
    assert_eq!(state.hand[0].kind, "poker_card_back");
    assert_eq!(state.hand[1].slot, SlotId::new(ObjectZone::Hand, 1));
    assert_eq!(state.hand[1].kind, "poker_card_front");
  }

  #[test]
  fn store_phase_detects_store_controls() {
    let image = test_image();
    let detections = detection_sets(
      vec![detection("joker_card", 30.0, 40.0, 60.0, 90.0)],
      vec![detection("button_store_reroll", 10.0, 120.0, 50.0, 145.0)],
    );

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Store);
    assert!(state.store.is_store);
    assert!(state.store.can_reroll);
    assert_eq!(state.store.item_count, 1);
    assert_eq!(state.store.items[0].slot, SlotId::new(ObjectZone::Store, 0));
    assert_eq!(state.store.items[0].kind, StoreItemKind::Joker);
  }

  #[test]
  fn new_run_play_button_detects_main_menu_phase() {
    let image = test_image();
    let detections = detection_sets(vec![], vec![detection("button_new_run_play", 40.0, 120.0, 80.0, 145.0)]);

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::MainMenu);
  }

  #[test]
  fn game_over_buttons_detect_game_over_phase() {
    let image = test_image();
    let detections = detection_sets(
      vec![],
      vec![
        detection("button_new_run", 40.0, 120.0, 80.0, 145.0),
        detection("button_main_menu", 90.0, 120.0, 130.0, 145.0),
      ],
    );

    let state = build_state_from_detections("synthetic-game-over.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::GameOver);
  }

  #[test]
  fn empty_detection_sets_build_unknown_state_with_diagnostic() {
    let image = test_image();
    let detections = detection_sets(vec![], vec![]);

    let state = build_state_from_detections("synthetic-empty.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Unknown);
    assert_eq!(state.diagnostics.len(), 1);
    assert_eq!(state.diagnostics[0].code, "empty_detection_sets");
  }

  #[test]
  fn build_state_preserves_frame_schema_raw_evidence_and_static_surfaces() {
    let image = test_image();
    let detections = detection_sets(
      vec![
        detection("tarot_card", 5.0, 10.0, 25.0, 40.0),
        detection("planet_card", 30.0, 10.0, 50.0, 40.0),
        detection("spectral_card", 55.0, 10.0, 75.0, 40.0),
        detection("card_pack", 80.0, 10.0, 110.0, 45.0),
      ],
      vec![
        detection("button_play", 10.0, 120.0, 40.0, 145.0),
        detection("button_discard", 45.0, 120.0, 90.0, 145.0),
      ],
    );

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.schema_version, BALATRO_STATE_SCHEMA_VERSION);
    assert_eq!(state.frame.source, "synthetic.png");
    assert_eq!(state.frame.image_size, image_size(&image));
    assert_eq!(state.phase, BalatroPhase::Playing);
    assert_eq!(state.raw_entities.len(), 4);
    assert_eq!(state.raw_entities[0].model, "balatro-entities");
    assert_eq!(state.raw_ui.len(), 2);
    assert_eq!(state.raw_ui[0].model, "balatro-ui");
    assert!(state.scores.chips.is_none());
    assert!(state.rounds.cash.is_none());
    assert!(state.diagnostics.is_empty());

    assert_eq!(state.consumables.len(), 3);
    assert_eq!(state.consumables[0].slot, SlotId::new(ObjectZone::Consumable, 0));
    assert_eq!(state.consumables[0].kind, ConsumableKind::Tarot);
    assert_eq!(state.consumables[1].kind, ConsumableKind::Planet);
    assert_eq!(state.consumables[2].kind, ConsumableKind::Spectral);

    assert!(!state.store.is_store);
    assert_eq!(state.store.items.len(), 0);
    assert_eq!(state.store.item_count, 0);

    assert_eq!(state.buttons.len(), 2);
    assert_eq!(state.buttons[0].id, "button_play");
    assert_eq!(state.buttons[0].label, "play");
    assert_eq!(state.buttons[1].id, "button_discard");
    assert_eq!(state.buttons[1].label, "discard");
  }

  #[test]
  fn playing_consumables_do_not_create_store_slots() {
    let image = test_image();
    let detections = detection_sets(
      vec![
        detection("tarot_card", 5.0, 10.0, 25.0, 40.0),
        detection("planet_card", 30.0, 10.0, 50.0, 40.0),
      ],
      vec![
        detection("button_play", 10.0, 120.0, 40.0, 145.0),
        detection("button_discard", 45.0, 120.0, 90.0, 145.0),
      ],
    );

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Playing);
    assert_eq!(state.consumables.len(), 2);
    assert!(!state.store.is_store);
    assert_eq!(state.store.item_count, 0);
    assert!(state.store.items.is_empty());
  }

  #[test]
  fn store_items_exclude_owned_jokers_and_deck_stack_by_layout() {
    let image = test_image_with_size(1600, 960);
    let detections = detection_sets_for_image(
      &image,
      vec![
        detection("joker_card", 280.0, 80.0, 420.0, 260.0),
        detection("joker_card", 460.0, 80.0, 600.0, 260.0),
        detection("joker_card", 560.0, 390.0, 700.0, 610.0),
        detection("planet_card", 760.0, 390.0, 900.0, 610.0),
        detection("poker_card_front", 1340.0, 430.0, 1440.0, 570.0),
        detection("poker_card_back", 1360.0, 450.0, 1460.0, 590.0),
      ],
      vec![detection(
        "button_store_next_round",
        1320.0,
        760.0,
        1530.0,
        850.0,
      )],
    );

    let state = build_state_from_detections("synthetic-store.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Store);
    assert!(state.store.is_store);
    assert_eq!(state.store.item_count, 3);
    assert_eq!(state.store.items[0].slot, SlotId::new(ObjectZone::Store, 0));
    assert_eq!(state.store.items[0].kind, StoreItemKind::Joker);
    assert_eq!(state.store.items[0].bbox.x1, 560.0);
    assert_eq!(state.store.items[1].slot, SlotId::new(ObjectZone::Store, 1));
    assert_eq!(state.store.items[1].kind, StoreItemKind::Planet);
    assert_eq!(state.store.items[1].bbox.x1, 760.0);
    assert_eq!(state.store.items[2].slot, SlotId::new(ObjectZone::Store, 2));
    assert_eq!(state.store.items[2].kind, StoreItemKind::Voucher);
  }

  #[test]
  fn store_items_include_card_pack_in_right_area() {
    let image = test_image_with_size(1600, 960);
    let detections = detection_sets_for_image(
      &image,
      vec![detection("card_pack", 1220.0, 390.0, 1560.0, 610.0)],
      vec![detection(
        "button_store_next_round",
        1320.0,
        760.0,
        1530.0,
        850.0,
      )],
    );

    let state = build_state_from_detections("synthetic-store-pack.png", image_size(&image), &image, detections, false);

    assert_eq!(state.store.item_count, 1);
    assert_eq!(state.store.items[0].slot, SlotId::new(ObjectZone::Store, 0));
    assert_eq!(state.store.items[0].kind, StoreItemKind::CardPack);
    assert_eq!(state.store.items[0].bbox.x1, 1220.0);
    assert_eq!(state.store.items[0].bbox.y1, 390.0);
  }

  #[test]
  fn voucher_store_candidate_uses_layout_fallback_without_detector_class() {
    let image = test_image_with_size(1600, 960);
    let detections = detection_sets_for_image(
      &image,
      vec![detection("joker_card", 560.0, 390.0, 700.0, 610.0)],
      vec![detection(
        "button_store_next_round",
        1320.0,
        760.0,
        1530.0,
        850.0,
      )],
    );

    let state = build_state_from_detections("synthetic-store-voucher.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Store);
    assert_eq!(state.store.item_count, 2);
    assert_eq!(state.store.items[0].kind, StoreItemKind::Joker);
    assert_eq!(state.store.items[1].slot, SlotId::new(ObjectZone::Store, 1));
    assert_eq!(state.store.items[1].kind, StoreItemKind::Voucher);
    assert!(state.store.items[1].confidence < 0.5);
  }

  #[test]
  fn store_items_include_lower_row_card_pack_at_small_window_scale() {
    let image = test_image_with_size(950, 583);
    let detections = detection_sets_for_image(
      &image,
      vec![detection("card_pack", 542.0, 415.0, 628.0, 553.0)],
      vec![detection(
        "button_store_next_round",
        273.0,
        236.0,
        397.0,
        300.0,
      )],
    );

    let state = build_state_from_detections("synthetic-small-store-pack.png", image_size(&image), &image, detections, false);

    assert_eq!(state.store.item_count, 1);
    assert_eq!(state.store.items[0].slot, SlotId::new(ObjectZone::Store, 0));
    assert_eq!(state.store.items[0].kind, StoreItemKind::CardPack);
  }

  #[test]
  fn hand_cards_can_infer_playing_phase_before_play_buttons_are_visible() {
    let image = test_image();
    let detections = detection_sets(
      vec![detection("poker_card_front", 40.0, 40.0, 70.0, 100.0)],
      vec![
        detection("button_sort_hand_rank", 10.0, 120.0, 40.0, 145.0),
        detection("button_sort_hand_suits", 45.0, 120.0, 90.0, 145.0),
      ],
    );

    let state = build_state_from_detections("synthetic.png", image_size(&image), &image, detections, false);

    assert_eq!(state.phase, BalatroPhase::Playing);
  }

  #[test]
  fn cache_hint_does_not_panic_for_partially_out_of_bounds_bbox() {
    let image = test_image();
    let detection = Detection {
      class_id: 0,
      label: "joker_card".to_owned(),
      confidence: 0.9,
      bbox: BoundingBox {
        x1: -5.0,
        y1: 2.0,
        x2: 8.0,
        y2: 20.0,
      },
    };

    let hint = cache_hint_for_detection(&detection, &image, false);

    assert!(hint.needs_reading);
    assert!(hint.visual_fingerprint.is_some());
  }

  #[test]
  fn no_cache_keeps_visual_fingerprint_for_verification_evidence() {
    let image = test_image();
    let detection = Detection {
      class_id: 0,
      label: "poker_card_front".to_owned(),
      confidence: 0.9,
      bbox: BoundingBox {
        x1: 2.0,
        y1: 2.0,
        x2: 16.0,
        y2: 20.0,
      },
    };

    let hint = cache_hint_for_detection(&detection, &image, true);

    assert!(hint.needs_reading);
    assert!(hint.visual_fingerprint.is_some());
    assert!(hint.changed_since_last_read);
  }

  fn detection_sets(entities: Vec<Detection>, ui: Vec<Detection>) -> BalatroDetectionSets {
    let image = test_image();
    detection_sets_for_image(&image, entities, ui)
  }

  fn detection_sets_for_image(image: &RgbImage, entities: Vec<Detection>, ui: Vec<Detection>) -> BalatroDetectionSets {
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size: image_size(image),
        detections: entities,
      },
      ui: DetectionResult {
        image_size: image_size(image),
        detections: ui,
      },
    }
  }

  fn detection(label: &str, x1: f32, y1: f32, x2: f32, y2: f32) -> Detection {
    Detection {
      class_id: 0,
      label: label.to_owned(),
      confidence: 0.9,
      bbox: BoundingBox { x1, y1, x2, y2 },
    }
  }

  fn test_image() -> RgbImage {
    test_image_with_size(160, 160)
  }

  fn test_image_with_size(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| Rgb([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8]))
  }

  fn image_size(image: &RgbImage) -> ImageSize {
    ImageSize {
      width: image.width(),
      height: image.height(),
    }
  }
}
