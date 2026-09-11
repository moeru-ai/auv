use auv_driver::{Capture, RecognizedText, Rect, TextRecognition};
use auv_inference_common::ImageSize;
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
use image::{Rgb, RgbImage, RgbaImage};

use crate::cache::cache_hint_for_detection;
use crate::model::{BalatroPhase, StoreItemKind};

use super::{
  BalatroDetectionSets, CardAttributeDetectionSets, balatro_runner_options, build_state_from_detections, detector_spec,
  driver_runner_options, enrich_ui_numeric_readings_from_recognition, load_remote_class_names, ocr_capture_for_ui,
  store_items_for_store_context,
};

#[test]
fn observation_routes_distinct_driver_and_balatro_runner_classes() {
  let driver = driver_runner_options();
  let balatro = balatro_runner_options();

  assert_eq!(driver.runner_class.as_str(), "auv.core.local");
  assert_eq!(balatro.runner_class.as_str(), "auv.game.balatro");
}

#[tokio::test]
async fn class_assets_load_outside_the_async_runtime_thread() {
  // ROOT CAUSE:
  //
  // If a remote observation resolved synchronous Hugging Face assets directly
  // inside Tokio, hf-hub tried to start a nested runtime and panicked. Asset
  // resolution must stay on the blocking pool while gRPC remains async.
  let directory = tempfile::tempdir().expect("class fixture directory");
  let entities = directory.path().join("entities.txt");
  let ui = directory.path().join("ui.txt");
  std::fs::write(&entities, "card\njoker\n").expect("write entity classes");
  std::fs::write(&ui, "button_play\n").expect("write UI classes");
  let config = crate::config::BalatroModelConfig {
    entities_classes: crate::config::BalatroModelAsset::local(entities),
    ui_classes: crate::config::BalatroModelAsset::local(ui),
    ..Default::default()
  };

  let (entities, ui) = load_remote_class_names(&config).await.expect("load class assets");

  assert_eq!(entities, ["card", "joker"]);
  assert_eq!(ui, ["button_play"]);
}

#[test]
fn cache_hint_handles_partially_out_of_bounds_bbox() {
  let image = RgbImage::from_fn(160, 160, |x, y| Rgb([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8]));
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
fn specialized_card_detections_replace_noisy_entity_cards_for_the_hand() {
  let image_size = ImageSize {
    width: 1000,
    height: 600,
  };
  let card = |x1: f32, confidence: f32| Detection {
    class_id: 6,
    label: "poker_card_front".to_string(),
    confidence,
    bbox: BoundingBox {
      x1,
      y1: 300.0,
      x2: x1 + 80.0,
      y2: 520.0,
    },
  };
  let state = build_state_from_detections(
    "fixture.png",
    image_size,
    &RgbImage::new(image_size.width, image_size.height),
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: vec![card(20.0, 0.55), card(400.0, 0.91), card(800.0, 0.60)],
      },
      cards: Some(DetectionResult {
        image_size,
        detections: vec![card(410.0, 0.99)],
      }),
      card_attributes: CardAttributeDetectionSets {
        identity: Some(DetectionResult {
          image_size,
          detections: vec![Detection {
            class_id: 51,
            label: "S_A".to_string(),
            confidence: 0.98,
            bbox: card(412.0, 0.98).bbox,
          }],
        }),
        enhancement: Some(DetectionResult {
          image_size,
          detections: vec![Detection {
            class_id: 2,
            label: "steel".to_string(),
            confidence: 0.97,
            bbox: card(412.0, 0.97).bbox,
          }],
        }),
        edition: Some(DetectionResult {
          image_size,
          detections: vec![Detection {
            class_id: 1,
            label: "foil".to_string(),
            confidence: 0.96,
            bbox: card(412.0, 0.96).bbox,
          }],
        }),
        seal: Some(DetectionResult {
          image_size,
          detections: vec![Detection {
            class_id: 3,
            label: "red".to_string(),
            confidence: 0.95,
            bbox: card(412.0, 0.95).bbox,
          }],
        }),
      },
      ui: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
    },
    true,
  );

  assert_eq!(state.hand.len(), 1);
  assert_eq!(state.hand[0].bbox.x1, 410.0);
  assert_eq!(state.hand[0].reading.text.as_deref(), Some("S_A"));
  assert_eq!(state.hand[0].attributes.enhancement.as_ref().map(|attribute| attribute.label.as_str()), Some("steel"));
  assert_eq!(state.hand[0].attributes.edition.as_ref().map(|attribute| attribute.label.as_str()), Some("foil"));
  assert_eq!(state.hand[0].attributes.seal.as_ref().map(|attribute| attribute.label.as_str()), Some("red"));
  assert!(state.raw_entities.iter().any(|evidence| evidence.model == "balatro-cards"));
  assert!(state.raw_entities.iter().any(|evidence| evidence.model == "balatro-card-identity"));
}

#[test]
fn detector_spec_preserves_model_input_size_and_cuda_index() {
  let spec = detector_spec(
    "balatro-card-identity",
    &crate::config::BalatroModelAsset::local("identity.onnx"),
    &auv_inference_ultralytics::InferenceDevice::Cuda(2),
    960,
    Vec::new(),
  )
  .expect("valid detector spec");

  assert_eq!(spec.input_size, Some(960));
  let device = spec.device.expect("device");
  assert_eq!(device.kind, crate::api::v1::InferenceDeviceKind::Cuda as i32);
  assert_eq!(device.index, Some(2));
}

#[test]
fn one_identity_detection_cannot_read_multiple_overlapping_hand_slots() {
  // ROOT CAUSE:
  //
  // If fanned hand boxes overlap, independently selecting each slot's best IoU
  // reused one identity detection for adjacent cards. Attribute association is
  // a one-to-one assignment even when a single box clears multiple thresholds.
  let image_size = ImageSize {
    width: 500,
    height: 300,
  };
  let card = |x1: f32| Detection {
    class_id: 6,
    label: "poker_card_front".to_string(),
    confidence: 0.9,
    bbox: BoundingBox {
      x1,
      y1: 100.0,
      x2: x1 + 120.0,
      y2: 260.0,
    },
  };
  let state = build_state_from_detections(
    "fixture://overlapping-hand",
    image_size,
    &RgbImage::new(image_size.width, image_size.height),
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
      cards: Some(DetectionResult {
        image_size,
        detections: vec![card(100.0), card(150.0)],
      }),
      card_attributes: CardAttributeDetectionSets {
        identity: Some(DetectionResult {
          image_size,
          detections: vec![Detection {
            class_id: 0,
            label: "H_A".to_string(),
            confidence: 0.99,
            bbox: card(125.0).bbox,
          }],
        }),
        ..Default::default()
      },
      ui: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
    },
    true,
  );

  assert_eq!(state.hand.iter().filter(|card| card.reading.text.as_deref() == Some("H_A")).count(), 1);
}

#[test]
fn ocr_capture_is_limited_to_detected_numeric_ui_and_preserves_screen_projection() {
  // ROOT CAUSE:
  //
  // If live observation sent the entire display back to Linux Tesseract, OCR
  // dominated latency even though Balatro numeric readings occupy a small UI
  // region. The fix derives one padded OCR crop from numeric UI detections and
  // ignores unrelated controls.
  let image_size = ImageSize {
    width: 1000,
    height: 600,
  };
  let detection = |label: &str, bbox: BoundingBox| Detection {
    class_id: 0,
    label: label.to_string(),
    confidence: 0.9,
    bbox,
  };
  let ui = DetectionResult {
    image_size,
    detections: vec![
      detection(
        "ui_score_chips",
        BoundingBox {
          x1: 100.0,
          y1: 120.0,
          x2: 200.0,
          y2: 180.0,
        },
      ),
      detection(
        "ui_data_cash",
        BoundingBox {
          x1: 300.0,
          y1: 400.0,
          x2: 450.0,
          y2: 500.0,
        },
      ),
      detection(
        "button_play",
        BoundingBox {
          x1: 0.0,
          y1: 0.0,
          x2: 1000.0,
          y2: 600.0,
        },
      ),
    ],
  };

  let capture = Capture {
    image: RgbaImage::new(1000, 600),
    bounds: Rect::new(10.0, 20.0, 500.0, 300.0),
    scale_factor: 2.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };

  let crop = ocr_capture_for_ui(&capture, &ui).expect("numeric UI should produce an OCR capture");

  assert_eq!(crop.image.dimensions(), (370, 392));
  assert_eq!(crop.bounds, Rect::new(55.0, 77.0, 185.0, 196.0));
  assert_eq!(crop.scale_factor, 2.0);
  assert_eq!(crop.backend, "fixture");
}

#[test]
fn ocr_capture_is_absent_without_numeric_ui() {
  let image_size = ImageSize {
    width: 1000,
    height: 600,
  };
  let ui = DetectionResult {
    image_size,
    detections: vec![Detection {
      class_id: 0,
      label: "button_play".to_string(),
      confidence: 0.9,
      bbox: BoundingBox {
        x1: 100.0,
        y1: 100.0,
        x2: 200.0,
        y2: 200.0,
      },
    }],
  };

  let capture = Capture {
    image: RgbaImage::new(1000, 600),
    bounds: Rect::new(0.0, 0.0, 1000.0, 600.0),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };

  assert_eq!(ocr_capture_for_ui(&capture, &ui), None);
}

#[test]
fn driver_runner_ocr_logical_bounds_enrich_pixel_space_numeric_detection() {
  let image = RgbImage::new(200, 100);
  let image_size = ImageSize {
    width: 200,
    height: 100,
  };
  let ui = DetectionResult {
    image_size,
    detections: vec![
      Detection {
        class_id: 0,
        label: "ui_score_chips".to_string(),
        confidence: 0.9,
        bbox: BoundingBox {
          x1: 90.0,
          y1: 20.0,
          x2: 130.0,
          y2: 60.0,
        },
      },
      Detection {
        class_id: 1,
        label: "button_play".to_string(),
        confidence: 0.9,
        bbox: BoundingBox {
          x1: 10.0,
          y1: 70.0,
          x2: 40.0,
          y2: 90.0,
        },
      },
    ],
  };
  let mut state = build_state_from_detections(
    "daemon://window/test",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
      cards: None,
      card_attributes: Default::default(),
      ui,
    },
    true,
  );
  let capture = Capture {
    image: RgbaImage::new(200, 100),
    bounds: Rect::new(10.0, 20.0, 20.0, 10.0),
    scale_factor: 10.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  };
  let recognition = TextRecognition {
    text: "$1,234".to_string(),
    regions: vec![RecognizedText {
      text: "$1,234".to_string(),
      bounds: Rect::new(20.0, 23.0, 2.0, 1.0),
      confidence: Some(0.99),
    }],
  };

  enrich_ui_numeric_readings_from_recognition(&mut state, &capture, recognition);

  assert_eq!(state.scores.chips.as_deref(), Some("$1234"));
}

#[test]
fn cash_out_control_classifies_the_payout_phase() {
  let image_size = ImageSize {
    width: 2560,
    height: 1440,
  };
  let image = RgbImage::new(image_size.width, image_size.height);
  let state = build_state_from_detections(
    "fixture://cash-out",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
      cards: None,
      card_attributes: Default::default(),
      ui: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 2,
          label: "button_cash_out".to_string(),
          confidence: 0.96,
          bbox: BoundingBox {
            x1: 1044.0,
            y1: 588.0,
            x2: 1670.0,
            y2: 695.0,
          },
        }],
      },
    },
    true,
  );

  assert_eq!(state.phase, BalatroPhase::CashOut);
}

#[test]
fn deck_stack_detection_is_not_promoted_to_an_owned_consumable() {
  // ROOT CAUSE:
  //
  // If the entity model mislabeled the right-side deck stack as a tarot card,
  // every tarot-shaped detection was promoted into the owned consumable row.
  // The fix keeps promotion inside the top owned-consumable layout band.
  let image_size = ImageSize {
    width: 2560,
    height: 1440,
  };
  let image = RgbImage::new(image_size.width, image_size.height);
  let state = build_state_from_detections(
    "fixture://playing-with-deck-stack-false-positive",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 7,
          label: "tarot_card".to_string(),
          confidence: 0.36,
          bbox: BoundingBox {
            x1: 2260.0,
            y1: 610.0,
            x2: 2490.0,
            y2: 1040.0,
          },
        }],
      },
      cards: None,
      card_attributes: Default::default(),
      ui: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 1,
          label: "button_play".to_string(),
          confidence: 0.95,
          bbox: BoundingBox {
            x1: 780.0,
            y1: 1120.0,
            x2: 1040.0,
            y2: 1230.0,
          },
        }],
      },
    },
    true,
  );

  assert!(state.consumables.is_empty());
  assert_eq!(state.raw_entities[0].detection.label, "tarot_card");
}

#[test]
fn top_row_consumable_is_promoted_to_owned_inventory() {
  let image_size = ImageSize {
    width: 2560,
    height: 1440,
  };
  let image = RgbImage::new(image_size.width, image_size.height);
  let state = build_state_from_detections(
    "fixture://owned-consumable",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 7,
          label: "tarot_card".to_string(),
          confidence: 0.91,
          bbox: BoundingBox {
            x1: 1700.0,
            y1: 90.0,
            x2: 1880.0,
            y2: 390.0,
          },
        }],
      },
      cards: None,
      card_attributes: Default::default(),
      ui: DetectionResult {
        image_size,
        detections: Vec::new(),
      },
    },
    true,
  );

  assert_eq!(state.consumables.len(), 1);
}

#[test]
fn store_joker_is_not_promoted_to_owned_inventory() {
  // ROOT CAUSE:
  //
  // Store and owned jokers share the `joker_card` detector label. Without a
  // zone gate, a shop product appeared in both `store.items` and `jokers`, so
  // `jokers ls` auto-read inventory that the player did not own.
  let image_size = ImageSize {
    width: 2560,
    height: 1440,
  };
  let image = RgbImage::new(image_size.width, image_size.height);
  let state = build_state_from_detections(
    "fixture://store-joker",
    image_size,
    &image,
    BalatroDetectionSets {
      entities: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 1,
          label: "joker_card".to_string(),
          confidence: 0.93,
          bbox: BoundingBox {
            x1: 1420.0,
            y1: 575.0,
            x2: 1590.0,
            y2: 825.0,
          },
        }],
      },
      cards: None,
      card_attributes: Default::default(),
      ui: DetectionResult {
        image_size,
        detections: vec![Detection {
          class_id: 2,
          label: "button_store_reroll".to_string(),
          confidence: 0.95,
          bbox: BoundingBox {
            x1: 1000.0,
            y1: 1000.0,
            x2: 1200.0,
            y2: 1100.0,
          },
        }],
      },
    },
    true,
  );

  assert!(state.jokers.is_empty());
  assert_eq!(state.store.items.len(), 2, "one detector item plus the voucher fallback");
}

#[test]
fn overlapping_store_class_predictions_share_one_slot() {
  let image = RgbImage::new(2560, 1440);
  let bbox = BoundingBox {
    x1: 1420.0,
    y1: 575.0,
    x2: 1590.0,
    y2: 825.0,
  };
  let entities = vec![
    Detection {
      class_id: 1,
      label: "joker_card".to_string(),
      confidence: 0.89,
      bbox,
    },
    Detection {
      class_id: 2,
      label: "tarot_card".to_string(),
      confidence: 0.91,
      bbox,
    },
  ];

  let items = store_items_for_store_context(&entities, &image, true);

  assert_eq!(items.len(), 2, "one detector item plus the voucher fallback");
  assert_eq!(items[0].kind, StoreItemKind::Tarot);
}
