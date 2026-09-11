use std::path::Path;

use auv_inference_common::{ImageFrame, ImageSize, InferenceResult, ModelId};
use auv_task_object_detection::{Detection, DetectionOptions, DetectionResult, UltralyticsObjectDetector, UltralyticsObjectDetectorConfig};
use image::RgbImage;

use crate::config::{BalatroModelConfig, load_class_names};

#[derive(Debug)]
pub struct BalatroDetectors {
  entities: UltralyticsObjectDetector,
  cards: Option<UltralyticsObjectDetector>,
  card_identity: UltralyticsObjectDetector,
  card_enhancement: UltralyticsObjectDetector,
  card_edition: UltralyticsObjectDetector,
  card_seal: UltralyticsObjectDetector,
  ui: UltralyticsObjectDetector,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardAttributeDetectionSets {
  pub identity: Option<DetectionResult>,
  pub enhancement: Option<DetectionResult>,
  pub edition: Option<DetectionResult>,
  pub seal: Option<DetectionResult>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalatroDetectionSets {
  pub entities: DetectionResult,
  pub cards: Option<DetectionResult>,
  pub card_attributes: CardAttributeDetectionSets,
  pub ui: DetectionResult,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HandCrop {
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GameViewport {
  x: u32,
  y: u32,
  width: u32,
  height: u32,
}

impl BalatroDetectors {
  pub fn load(config: BalatroModelConfig) -> InferenceResult<Self> {
    let config = config.resolve().map_err(|error| auv_inference_common::InferenceError::Backend {
      message: error.to_string(),
    })?;
    let entities = UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig {
      model_id: ModelId("balatro-entities".to_owned()),
      model_path: config.entities_model,
      input_size: Some(640),
      options: balatro_detection_options(),
      device: config.device.clone(),
      class_names_override: Some(load_class_names(&config.entities_classes)?),
    })?;
    let cards = config
      .cards_model
      .map(|model_path| {
        UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig {
          model_id: ModelId("balatro-cards".to_owned()),
          model_path,
          input_size: Some(640),
          options: balatro_detection_options(),
          device: config.device.clone(),
          class_names_override: Some(load_class_names(&config.entities_classes)?),
        })
      })
      .transpose()?;
    let load_attribute = |model_id: &str, model_path, input_size| {
      UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig {
        model_id: ModelId(model_id.to_owned()),
        model_path,
        input_size: Some(input_size),
        options: balatro_detection_options(),
        device: config.device.clone(),
        class_names_override: None,
      })
    };
    let card_identity = load_attribute("balatro-card-identity", config.card_identity_model, 960)?;
    let card_enhancement = load_attribute("balatro-card-enhancement", config.card_enhancement_model, 640)?;
    let card_edition = load_attribute("balatro-card-edition", config.card_edition_model, 640)?;
    let card_seal = load_attribute("balatro-card-seal", config.card_seal_model, 640)?;
    let ui = UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig {
      model_id: ModelId("balatro-ui".to_owned()),
      model_path: config.ui_model,
      input_size: Some(640),
      options: balatro_detection_options(),
      device: config.device,
      class_names_override: Some(load_class_names(&config.ui_classes)?),
    })?;

    Ok(Self {
      entities,
      cards,
      card_identity,
      card_enhancement,
      card_edition,
      card_seal,
      ui,
    })
  }

  pub fn detect_path(&self, image: impl AsRef<Path>) -> InferenceResult<BalatroDetectionSets> {
    let image_path = image.as_ref();
    let image = image::open(image_path)?.to_rgb8();
    let image_size = ImageSize {
      width: image.width(),
      height: image.height(),
    };
    let (hand_crop, hand_frame) = crop_hand_frame(&image);
    let entities = self.entities.detect_path(image_path)?;
    let cards = self
      .cards
      .as_ref()
      .map(|detector| detector.detect_frame(&hand_frame).map(|result| remap_hand_detections(result, hand_crop, image_size)))
      .transpose()?;
    let card_attributes = CardAttributeDetectionSets {
      identity: Some(remap_card_attribute_detections(self.card_identity.detect_frame(&hand_frame)?, hand_crop, image_size)),
      enhancement: Some(remap_card_attribute_detections(self.card_enhancement.detect_frame(&hand_frame)?, hand_crop, image_size)),
      edition: Some(remap_card_attribute_detections(self.card_edition.detect_frame(&hand_frame)?, hand_crop, image_size)),
      seal: Some(remap_card_attribute_detections(self.card_seal.detect_frame(&hand_frame)?, hand_crop, image_size)),
    };
    let ui = self.ui.detect_path(image_path)?;
    Ok(BalatroDetectionSets {
      entities,
      cards,
      card_attributes,
      ui,
    })
  }
}

#[cfg(test)]
fn hand_crop(image_size: ImageSize) -> HandCrop {
  hand_crop_in_viewport(GameViewport {
    x: 0,
    y: 0,
    width: image_size.width.max(1),
    height: image_size.height.max(1),
  })
}

fn hand_crop_in_viewport(viewport: GameViewport) -> HandCrop {
  let width = viewport.width.max(1);
  let height = viewport.height.max(1);
  // NOTICE: These normalized bounds come from the 2043x1126 live Balatro
  // layout used by the Mod corpus. They exclude the confirmed left score-panel
  // and right deck-stack false positives while retaining highlighted hand
  // cards. Replace them when observation owns a typed, dynamically resolved
  // hand-region contract across UI scales.
  let x = ((width as f32) * 0.265).floor() as u32;
  let y = ((height as f32) * 0.52).floor() as u32;
  let right = ((width as f32) * 0.815).floor() as u32;
  let bottom = ((height as f32) * 0.84).floor() as u32;
  HandCrop {
    x: viewport.x + x,
    y: viewport.y + y,
    width: right.saturating_sub(x).max(1),
    height: bottom.saturating_sub(y).max(1),
  }
}

pub(crate) fn crop_hand_frame(image: &RgbImage) -> (HandCrop, ImageFrame) {
  let crop = hand_crop_in_viewport(resolve_game_viewport(image));
  let image = image::imageops::crop_imm(image, crop.x, crop.y, crop.width, crop.height).to_image();
  (crop, ImageFrame::new(image))
}

fn resolve_game_viewport(image: &RgbImage) -> GameViewport {
  let full = GameViewport {
    x: 0,
    y: 0,
    width: image.width().max(1),
    height: image.height().max(1),
  };
  if image.width() < 32 || image.height() < 32 {
    return full;
  }

  // NOTICE: The Mod corpus viewport is 2043x1126. A small tolerance accepts
  // ordinary capture rounding while distinguishing a 16:9 full-display
  // fallback from the game client area.
  let target_aspect = 2043.0 / 1126.0;
  let full_aspect = image.width() as f32 / image.height() as f32;
  if (full_aspect - target_aspect).abs() <= 0.015 {
    return full;
  }

  if let Some(viewport) = resolve_floating_game_viewport(image, target_aspect) {
    return viewport;
  }

  // Linux currently falls back to a primary-display capture when the window
  // API is unavailable. In that observed layout the game viewport is clipped
  // against the display's right and bottom edges. Scan possible top edges,
  // derive the matching left edge from the corpus aspect ratio, then require
  // both edges to survive grayscale thresholding. Requiring both long edges
  // prevents a stronger internal score-panel divider from winning.
  // TODO(balatro-floating-viewport): detect all four edges once Linux exposes
  // movable-window fixtures; this slice fixes the reproduced right/bottom-
  // clipped display fallback without claiming arbitrary desktop layouts.
  let maximum_top = image.height() * 2 / 5;
  let mut best = None;
  for y in 2..=maximum_top {
    let height = image.height() - y;
    let width = ((height as f32) * target_aspect).round() as u32;
    if width >= image.width() || width < image.width() * 3 / 5 {
      continue;
    }
    let x = image.width() - width;
    if x < 2 || x > image.width() * 2 / 5 {
      continue;
    }
    let horizontal = horizontal_edge_evidence(image, x, y);
    let vertical = vertical_edge_evidence(image, x, y);
    let score = horizontal.min(vertical);
    if best.is_none_or(|(_, best_score)| score > best_score) {
      best = Some((
        GameViewport {
          x,
          y,
          width,
          height,
        },
        score,
      ));
    }
  }

  best.filter(|(_, score)| *score >= 0.22).map(|(viewport, _)| viewport).unwrap_or(full)
}

fn resolve_floating_game_viewport(image: &RgbImage, target_aspect: f32) -> Option<GameViewport> {
  let horizontal = strongest_edges((2..image.height().saturating_sub(2)).map(|y| (y, horizontal_edge_evidence(image, 0, y))), 24);
  let vertical = strongest_edges((2..image.width().saturating_sub(2)).map(|x| (x, vertical_edge_evidence(image, x, 0))), 24);
  let mut best = None;
  for &(left, left_score) in &vertical {
    for &(right, right_score) in &vertical {
      if right <= left {
        continue;
      }
      let width = right - left + 1;
      if width < image.width() * 3 / 5 {
        continue;
      }
      for &(top, top_score) in &horizontal {
        for &(bottom, bottom_score) in &horizontal {
          if bottom <= top {
            continue;
          }
          let height = bottom - top + 1;
          if height < image.height() * 3 / 5 {
            continue;
          }
          let aspect_error = (width as f32 / height as f32 - target_aspect).abs();
          if aspect_error > 0.05 {
            continue;
          }
          let edge_score = left_score.min(right_score).min(top_score).min(bottom_score);
          let score = edge_score - aspect_error * 2.0;
          if best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((
              GameViewport {
                x: left,
                y: top,
                width,
                height,
              },
              score,
            ));
          }
        }
      }
    }
  }
  best.filter(|(_, score)| *score >= 0.08).map(|(viewport, _)| viewport)
}

fn strongest_edges(edges: impl Iterator<Item = (u32, f32)>, limit: usize) -> Vec<(u32, f32)> {
  let mut edges = edges.filter(|(_, score)| *score >= 0.08).collect::<Vec<_>>();
  edges.sort_by(|left, right| right.1.total_cmp(&left.1));
  let mut selected: Vec<(u32, f32)> = Vec::new();
  for edge in edges {
    if selected.iter().any(|(position, _)| position.abs_diff(edge.0) <= 3) {
      continue;
    }
    selected.push(edge);
    if selected.len() == limit {
      break;
    }
  }
  selected
}

fn horizontal_edge_evidence(image: &RgbImage, x: u32, y: u32) -> f32 {
  edge_evidence((x..image.width()).step_by(4).map(|sample_x| {
    let before = grayscale(image.get_pixel(sample_x, y - 2));
    let after = grayscale(image.get_pixel(sample_x, (y + 2).min(image.height() - 1)));
    before.abs_diff(after)
  }))
}

fn vertical_edge_evidence(image: &RgbImage, x: u32, y: u32) -> f32 {
  edge_evidence((y..image.height()).step_by(4).map(|sample_y| {
    let before = grayscale(image.get_pixel(x - 2, sample_y));
    let after = grayscale(image.get_pixel((x + 2).min(image.width() - 1), sample_y));
    before.abs_diff(after)
  }))
}

fn edge_evidence(samples: impl Iterator<Item = u8>) -> f32 {
  let mut count = 0_u32;
  let mut polarized = 0_u32;
  let mut strength = 0_u32;
  for contrast in samples {
    count += 1;
    polarized += u32::from(contrast >= 16);
    strength += u32::from(contrast.min(64));
  }
  if count == 0 {
    return 0.0;
  }
  let support = polarized as f32 / count as f32;
  let normalized_strength = strength as f32 / (count * 64) as f32;
  support * 0.7 + normalized_strength * 0.3
}

fn grayscale(pixel: &image::Rgb<u8>) -> u8 {
  ((u16::from(pixel[0]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[2]) * 29) >> 8) as u8
}

pub(crate) fn remap_hand_detections(mut result: DetectionResult, crop: HandCrop, image_size: ImageSize) -> DetectionResult {
  result
    .detections
    .retain(|detection| matches!(detection.label.as_str(), "poker_card_front" | "poker_card_back") && is_complete_hand_card(detection));
  remap_card_attribute_detections(result, crop, image_size)
}

pub(crate) fn remap_card_attribute_detections(mut result: DetectionResult, crop: HandCrop, image_size: ImageSize) -> DetectionResult {
  for detection in &mut result.detections {
    detection.bbox.x1 += crop.x as f32;
    detection.bbox.x2 += crop.x as f32;
    detection.bbox.y1 += crop.y as f32;
    detection.bbox.y2 += crop.y as f32;
  }
  result.image_size = image_size;
  result
}

fn is_complete_hand_card(detection: &Detection) -> bool {
  let width = detection.bbox.x2 - detection.bbox.x1;
  let height = detection.bbox.y2 - detection.bbox.y1;
  width.is_finite() && height.is_finite() && height > 0.0 && width / height >= 0.30
}

fn balatro_detection_options() -> DetectionOptions {
  DetectionOptions {
    confidence_threshold: 0.25,
    iou_threshold: 0.45,
    max_detections: 300,
  }
}

#[cfg(test)]
mod tests {
  use auv_task_object_detection::{BoundingBox, Detection};

  use super::*;

  #[test]
  fn hand_crop_excludes_score_panel_and_deck_stack() {
    let crop = hand_crop(ImageSize {
      width: 2043,
      height: 1126,
    });

    assert_eq!(
      crop,
      HandCrop {
        x: 541,
        y: 585,
        width: 1124,
        height: 360
      }
    );
  }

  #[test]
  fn hand_crop_is_relative_to_an_offset_game_viewport() {
    // ROOT CAUSE:
    //
    // If Linux window capture fell back to the full display, the normalized
    // hand crop was applied to the desktop instead of the Balatro viewport.
    // Before the fix, an offset viewport included the score panel and produced
    // extra card slots. The fix resolves the viewport before deriving the hand.
    let mut image = RgbImage::from_pixel(256, 144, image::Rgb([18, 18, 18]));
    for y in 31..144 {
      for x in 51..256 {
        let shade = 86 + ((x + y) % 19) as u8;
        image.put_pixel(x, y, image::Rgb([shade, shade, shade]));
      }
    }
    // A stronger internal panel boundary must not replace the outer viewport.
    for y in 31..144 {
      image.put_pixel(61, y, image::Rgb([230, 230, 230]));
    }

    let (crop, _) = crop_hand_frame(&image);

    assert_eq!(
      crop,
      HandCrop {
        x: 105,
        y: 89,
        width: 113,
        height: 36,
      }
    );
  }

  #[test]
  fn hand_crop_is_relative_to_a_floating_game_viewport() {
    // ROOT CAUSE:
    //
    // If the Linux window API was unavailable and Balatro did not touch the
    // display's right/bottom edges, viewport detection assumed a clipped
    // window and fell back to the whole display. The specialized hand crop
    // then clipped the first card and included the deck stack as extra slots.
    let mut image = RgbImage::from_pixel(256, 144, image::Rgb([18, 18, 18]));
    for y in 15..127 {
      for x in 7..211 {
        let shade = 82 + ((x + y) % 23) as u8;
        image.put_pixel(x, y, image::Rgb([shade, shade, shade]));
      }
    }
    for x in 7..211 {
      image.put_pixel(x, 15, image::Rgb([235, 235, 235]));
      image.put_pixel(x, 126, image::Rgb([235, 235, 235]));
    }
    for y in 15..127 {
      image.put_pixel(7, y, image::Rgb([235, 235, 235]));
      image.put_pixel(210, y, image::Rgb([235, 235, 235]));
      image.put_pixel(18, y, image::Rgb([245, 245, 245]));
    }

    let (crop, _) = crop_hand_frame(&image);

    assert_eq!(
      crop,
      HandCrop {
        x: 59,
        y: 71,
        width: 112,
        height: 36,
      }
    );
  }

  #[test]
  fn remap_hand_detections_restores_coordinates_and_filters_non_cards_and_clipped_edges() {
    // ROOT CAUSE:
    //
    // If the hand crop included a sliver of the deck stack, the specialized
    // model returned a very narrow poker_card_back and promoted it to a ninth
    // hand slot.
    //
    // Before the fix, label filtering retained that unusable partial box. The
    // fix requires enough card width to represent a complete clickable slot.
    let result = DetectionResult {
      image_size: ImageSize {
        width: 1124,
        height: 360,
      },
      detections: vec![
        Detection {
          class_id: 6,
          label: "poker_card_front".to_string(),
          confidence: 0.98,
          bbox: BoundingBox {
            x1: 10.0,
            y1: 20.0,
            x2: 110.0,
            y2: 220.0,
          },
        },
        Detection {
          class_id: 2,
          label: "joker_card".to_string(),
          confidence: 0.51,
          bbox: BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
        },
        Detection {
          class_id: 4,
          label: "poker_card_back".to_string(),
          confidence: 0.75,
          bbox: BoundingBox {
            x1: 1090.0,
            y1: 170.0,
            x2: 1118.0,
            y2: 312.0,
          },
        },
      ],
    };
    let image_size = ImageSize {
      width: 2043,
      height: 1126,
    };

    let remapped = remap_hand_detections(
      result,
      HandCrop {
        x: 541,
        y: 585,
        width: 1124,
        height: 360,
      },
      image_size,
    );

    assert_eq!(remapped.image_size, image_size);
    assert_eq!(remapped.detections.len(), 1);
    assert_eq!(
      remapped.detections[0].bbox,
      BoundingBox {
        x1: 551.0,
        y1: 605.0,
        x2: 651.0,
        y2: 805.0,
      }
    );
  }

  #[test]
  fn remap_card_attribute_detections_restores_full_frame_coordinates_without_filtering_labels() {
    // ROOT CAUSE:
    //
    // If card-attribute models received the full game frame, unrelated UI
    // regions produced high-confidence identities while the hand became too
    // small to read. Attribute inference must use the normalized hand crop,
    // then restore every attribute label to full-frame coordinates before
    // slot association.
    let crop = HandCrop {
      x: 541,
      y: 585,
      width: 1124,
      height: 360,
    };
    let image_size = ImageSize {
      width: 2043,
      height: 1126,
    };
    let result = DetectionResult {
      image_size: ImageSize {
        width: crop.width,
        height: crop.height,
      },
      detections: vec![Detection {
        class_id: 51,
        label: "S_A".to_string(),
        confidence: 0.99,
        bbox: BoundingBox {
          x1: 10.0,
          y1: 20.0,
          x2: 110.0,
          y2: 220.0,
        },
      }],
    };

    let remapped = remap_card_attribute_detections(result, crop, image_size);

    assert_eq!(remapped.image_size, image_size);
    assert_eq!(remapped.detections.len(), 1);
    assert_eq!(remapped.detections[0].label, "S_A");
    assert_eq!(
      remapped.detections[0].bbox,
      BoundingBox {
        x1: 551.0,
        y1: 605.0,
        x2: 651.0,
        y2: 805.0,
      }
    );
  }
}
