use super::*;

#[test]
fn remote_hover_text_promotes_an_object_read_and_records_evidence() {
  // ROOT CAUSE:
  //
  // If a Balatro command ran inside a routed non-macOS Device context,
  // object reads stopped after detection and always returned `unread` because
  // only the local macOS branch performed hover OCR.
  //
  // The fix promotes non-empty routed OCR text while preserving the hover
  // frame and region that explain where the reading came from.
  let mut read = ObjectReadResult {
    slot: SlotId::new(ObjectZone::Joker, 0),
    kind: "joker".to_string(),
    bbox: BoundingBox {
      x1: 10.0,
      y1: 20.0,
      x2: 30.0,
      y2: 60.0,
    },
    confidence: 0.9,
    reading: ObjectReadValue::unread(),
    evidence: ObjectReadEvidence {
      frame: "daemon://display/primary".to_string(),
      source: "observation_without_hover_ocr".to_string(),
      hover_required: true,
      hover_frame: None,
      hover_ocr_region: None,
      hover_error: None,
    },
  };

  apply_hover_read_observation(
    &mut read,
    auv_driver::TextRecognition {
      text: "Telegram\nBlueprint\nCopies Joker to the right".to_string(),
      regions: vec![
        auv_driver::RecognizedText {
          text: "Telegram".to_string(),
          bounds: Rect::new(900.0, 900.0, 100.0, 20.0),
          confidence: Some(0.99),
        },
        auv_driver::RecognizedText {
          text: "Blueprint".to_string(),
          bounds: Rect::new(15.0, 18.0, 80.0, 15.0),
          confidence: Some(0.92),
        },
        auv_driver::RecognizedText {
          text: "Copies Joker to the right".to_string(),
          bounds: Rect::new(15.0, 36.0, 160.0, 15.0),
          confidence: Some(0.88),
        },
      ],
    },
    "daemon://hover/0".to_string(),
  );

  assert_eq!(read.reading.status, "read");
  assert_eq!(read.reading.raw_text.as_deref(), Some("Blueprint\nCopies Joker to the right"));
  assert!(!read.evidence.hover_required);
  assert_eq!(read.evidence.source, "remote_hover_ocr");
  assert_eq!(read.evidence.hover_frame.as_deref(), Some("daemon://hover/0"));
  assert_eq!(read.evidence.hover_ocr_region, Some(object_hover_ocr_region()));
}

#[test]
fn card_commit_accepts_the_same_hand_detector_and_device_as_state_observation() {
  // ROOT CAUSE:
  //
  // If a remote card action always constructed `BalatroModelConfig::default`,
  // `game state --cards-model ... --device cuda:0` exposed eight slots while
  // `cards play` silently switched back to the CPU entities detector and saw
  // nine. The action must carry its observation policy explicitly.
  let args = CliArgs::try_parse_from([
    "auv-game-balatro",
    "cards",
    "play",
    "--slots",
    "hand:3,hand:4",
    "--cards-model",
    "/models/cards.onnx",
    "--device",
    "cuda:0",
  ])
  .expect("card action arguments");

  let Command::Cards(CardsArgs {
    command: CardsCommand::Play(args),
  }) = args.command
  else {
    panic!("expected cards play command");
  };
  let config = BalatroModelConfig::from_operation_args(&args.control);
  assert_eq!(config.cards_model, Some(crate::config::BalatroModelAsset::local("/models/cards.onnx")));
  assert_eq!(config.device, InferenceDevice::Cuda(0));
}

#[test]
fn raised_hand_card_is_selected_when_commit_buttons_are_missed() {
  // ROOT CAUSE:
  //
  // If the UI detector missed Play Hand and Discard, selected-state inference
  // returned false before inspecting card geometry. The remote action then
  // clicked already-raised cards again instead of submitting the hand.
  //
  // The selected card remains 30 pixels above the lower-row baseline here;
  // commit-button detection is intentionally absent.
  let state = playing_hand_state(Vec::new(), vec![820.0, 850.0, 850.0]);

  assert!(hand_slot_is_selected(&state, 0));
  assert!(!hand_slot_is_selected(&state, 1));
}

#[test]
fn shallow_detector_jitter_is_not_a_selected_card() {
  // ROOT CAUSE:
  //
  // A 21-pixel detector shift on an unselected card crossed the old 18-pixel
  // threshold. The command then treated a stale capture as selected and could
  // report a no-op Play as successful.
  let state = playing_hand_state(Vec::new(), vec![829.0, 850.0, 850.0]);

  assert!(!hand_slot_is_selected(&state, 0));
}

#[test]
fn card_commit_uses_sort_controls_as_playing_layout_evidence() {
  // ROOT CAUSE:
  //
  // The live Linux UI model reliably detected both Sort Hand controls but
  // omitted the active Play Hand and Discard controls. Requiring the latter
  // left a correctly selected hand with no safe submission target.
  let sort_rank = button("button_sort_hand_rank", 1112.0, 1175.0, 1193.0, 1238.0);
  let sort_suits = button("button_sort_hand_suits", 1200.0, 1177.0, 1283.0, 1239.0);
  let state = playing_hand_state(vec![sort_rank.clone(), sort_suits.clone()], vec![850.0; 8]);

  let (control, point) = resolve_card_commit_target(&state, CardCommitKind::Play).expect("playing layout target");

  assert_eq!(
    control,
    CardCommitControl::PlayingHandLayout {
      sort_rank,
      sort_suits,
    }
  );
  assert!((point.x - 984.0).abs() < 3.0, "unexpected play x: {}", point.x);
  assert!((point.y - 1207.5).abs() < 3.0, "unexpected play y: {}", point.y);
}

fn playing_hand_state(buttons: Vec<ButtonTarget>, y_positions: Vec<f32>) -> BalatroState {
  BalatroState {
    schema_version: crate::model::BALATRO_STATE_SCHEMA_VERSION.to_string(),
    frame: crate::model::FrameRef {
      source: "test://frame".to_string(),
      image_size: auv_inference_common::ImageSize {
        width: 2560,
        height: 1440,
      },
    },
    phase: BalatroPhase::Playing,
    scores: Default::default(),
    rounds: Default::default(),
    hand: y_positions
      .into_iter()
      .enumerate()
      .map(|(index, y1)| CardSlot {
        slot: SlotId::new(ObjectZone::Hand, index as u32),
        kind: "poker_card_front".to_string(),
        bbox: BoundingBox {
          x1: 650.0 + index as f32 * 125.0,
          y1,
          x2: 835.0 + index as f32 * 125.0,
          y2: y1 + 240.0,
        },
        confidence: 0.95,
        reading: crate::model::Reading::unread(),
        attributes: Default::default(),
        cache: Default::default(),
      })
      .collect(),
    jokers: Vec::new(),
    consumables: Vec::new(),
    store: Default::default(),
    buttons,
    diagnostics: Vec::new(),
    raw_entities: Vec::new(),
    raw_ui: Vec::new(),
  }
}

fn button(id: &str, x1: f32, y1: f32, x2: f32, y2: f32) -> ButtonTarget {
  ButtonTarget {
    id: id.to_string(),
    label: id.trim_start_matches("button_").to_string(),
    bbox: BoundingBox { x1, y1, x2, y2 },
    confidence: 0.95,
  }
}

#[test]
fn ui_digit_reader_segments_multiple_glyphs() {
  let image = synthetic_ui_digit_image("300");

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

  assert_eq!(reading.as_deref(), Some("300"));
}

#[test]
fn ui_digit_score_reading_formats_mult_label() {
  assert_eq!(ui_digit_text_for_label("ui_score_mult", "3").as_deref(), Some("x3"));
  assert_eq!(ui_digit_text_for_label("ui_score_target_score", "300").as_deref(), Some("300"));
}

#[test]
fn ui_digit_score_reading_drops_round_score_chip_icon() {
  assert_eq!(ui_digit_text_for_label("ui_score_round_score", "00").as_deref(), Some("0"));
  assert_eq!(ui_digit_text_for_label("ui_score_round_score", "0300").as_deref(), Some("300"));
}

#[test]
fn ui_digit_reader_matches_balatro_thick_one() {
  let mask = mask_from_rows([
    "####.", "####.", ".###.", ".###.", ".###.", "#####", "#####",
  ]);

  assert_eq!(infer_ui_digit_from_mask(&mask), Some(1));
}

#[test]
fn ui_digit_reader_keeps_scaled_five_distinct_from_three() {
  // ROOT CAUSE:
  //
  // If nearest-neighbor scaling made the top, middle, and bottom strokes two
  // sample rows thick, the thin five template was farther away than three.
  // The template now represents the observed thick Balatro glyph.
  let mask = mask_from_rows([
    "#####", "#####", "####.", "#####", "...##", "#####", "#####",
  ]);

  assert_eq!(infer_ui_digit_from_mask(&mask), Some(5));
}

#[test]
fn ambiguous_ui_digit_is_unread_instead_of_becoming_a_false_score() {
  let three = mask_from_rows(UI_DIGIT_TEMPLATES.iter().find(|template| template.digit == 3).unwrap().rows);
  let five = mask_from_rows(UI_DIGIT_TEMPLATES.iter().find(|template| template.digit == 5).unwrap().rows);
  let mut ambiguous = three;
  for index in (0..UI_DIGIT_MASK_CELLS).step_by(2) {
    ambiguous[index] = five[index];
  }

  assert_eq!(infer_ui_digit_from_mask(&ambiguous), None);
}

#[test]
fn white_ui_digit_reader_ignores_colored_score_background() {
  let mut image = RgbaImage::from_pixel(80, 56, image::Rgba([220, 70, 60, 255]));
  draw_synthetic_ui_digit(&mut image, '0', 20, image::Rgba([245, 245, 245, 255]));

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::White);

  assert_eq!(reading.as_deref(), Some("0"));
}

#[test]
fn ui_digit_reader_ignores_score_punctuation_sized_glyphs() {
  let mut image = RgbaImage::from_pixel(240, 56, image::Rgba([20, 25, 24, 255]));
  let color = image::Rgba([240, 80, 60, 255]);
  draw_synthetic_ui_digit_scaled(&mut image, '1', 0, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 44, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 90, 5, color);
  draw_synthetic_ui_digit_scaled(&mut image, '0', 132, 8, color);
  draw_synthetic_ui_digit_scaled(&mut image, '4', 176, 8, color);

  let reading = infer_ui_digit_text_from_image_with_foreground(&image, UiDigitForeground::Colored);

  assert_eq!(reading.as_deref(), Some("1404"));
}

fn synthetic_ui_digit_image(text: &str) -> RgbaImage {
  let scale = 8;
  let gap = 4;
  let width = text.len() as u32 * UI_DIGIT_MASK_W as u32 * scale + text.len().saturating_sub(1) as u32 * gap;
  let height = UI_DIGIT_MASK_H as u32 * scale;
  let mut image = RgbaImage::from_pixel(width, height, image::Rgba([20, 25, 24, 255]));
  let mut cursor_x = 0;
  for character in text.chars() {
    draw_synthetic_ui_digit(&mut image, character, cursor_x, image::Rgba([240, 80, 60, 255]));
    cursor_x += UI_DIGIT_MASK_W as u32 * scale + gap;
  }
  image
}

fn draw_synthetic_ui_digit(image: &mut RgbaImage, character: char, cursor_x: u32, color: image::Rgba<u8>) {
  draw_synthetic_ui_digit_scaled(image, character, cursor_x, 8, color);
}

fn draw_synthetic_ui_digit_scaled(image: &mut RgbaImage, character: char, cursor_x: u32, scale: u32, color: image::Rgba<u8>) {
  let digit = character.to_digit(10).unwrap() as u8;
  let template = UI_DIGIT_TEMPLATES.iter().find(|template| template.digit == digit).unwrap();
  for (row_index, row) in template.rows.iter().enumerate() {
    for (column_index, pixel) in row.chars().enumerate() {
      if pixel != '#' {
        continue;
      }
      for y in 0..scale {
        for x in 0..scale {
          image.put_pixel(cursor_x + column_index as u32 * scale + x, row_index as u32 * scale + y, color);
        }
      }
    }
  }
}

fn mask_from_rows(rows: [&str; 7]) -> [bool; UI_DIGIT_MASK_CELLS] {
  let mut mask = [false; UI_DIGIT_MASK_CELLS];
  for (row_index, row) in rows.iter().enumerate() {
    for (column_index, character) in row.chars().enumerate() {
      mask[row_index * UI_DIGIT_MASK_W + column_index] = character == '#';
    }
  }
  mask
}
