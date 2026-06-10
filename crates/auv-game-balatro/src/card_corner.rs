use auv_inference_common::{InferenceError, InferenceResult};
use auv_inference_ort::{ExecutionProvider, F32Tensor, OrtModelConfig, OrtSession, softmax, top1};
use image::{RgbImage, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::{BalatroModelAsset, BalatroModelConfig, BalatroModelConfigError};

pub const CARD_CORNER_IMAGE_SIZE: u32 = 64;
pub const CARD_CORNER_INPUT_NAME: &str = "images";
pub const CARD_CORNER_RANK_OUTPUT_NAME: &str = "rank_logits";
pub const CARD_CORNER_SUIT_OUTPUT_NAME: &str = "suit_logits";
pub const CARD_CORNER_MODEL_ID: &str = "balatro-card-corner-classifier";

pub const RANK_LABELS: [&str; 13] = [
  "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K",
];
pub const SUIT_LABELS: [&str; 4] = ["spades", "hearts", "clubs", "diamonds"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardCornerClassifierConfig {
  pub model_path: PathBuf,
  pub execution_provider: ExecutionProvider,
  pub image_size: u32,
}

impl CardCornerClassifierConfig {
  pub fn new(model_path: PathBuf) -> Self {
    Self {
      model_path,
      execution_provider: ExecutionProvider::Cpu,
      image_size: CARD_CORNER_IMAGE_SIZE,
    }
  }

  pub fn default_model() -> Result<Self, BalatroModelConfigError> {
    Self::from_model_asset(&BalatroModelConfig::default().card_corner_model)
  }

  pub fn from_model_asset(asset: &BalatroModelAsset) -> Result<Self, BalatroModelConfigError> {
    Ok(Self::new(asset.resolve_path()?))
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CardCornerPrediction {
  pub rank: String,
  pub suit: String,
  pub rank_confidence: f32,
  pub suit_confidence: f32,
}

#[derive(Debug)]
pub struct CardCornerClassifier {
  session: OrtSession,
  image_size: u32,
}

impl CardCornerClassifier {
  pub fn load(config: CardCornerClassifierConfig) -> InferenceResult<Self> {
    let session = OrtSession::load(OrtModelConfig {
      model_path: config.model_path,
      execution_provider: config.execution_provider,
    })?;

    Ok(Self {
      session,
      image_size: config.image_size,
    })
  }

  pub fn predict(&self, image: &RgbImage) -> InferenceResult<CardCornerPrediction> {
    let input = card_corner_input_tensor(image, self.image_size);
    let outputs = self.session.run_f32(input)?;
    let rank_logits = named_output(&outputs, CARD_CORNER_RANK_OUTPUT_NAME)?;
    let suit_logits = named_output(&outputs, CARD_CORNER_SUIT_OUTPUT_NAME)?;

    prediction_from_logits(rank_logits, suit_logits)
  }
}

pub fn rank_label(index: usize) -> Option<&'static str> {
  RANK_LABELS.get(index).copied()
}

pub fn suit_label(index: usize) -> Option<&'static str> {
  SUIT_LABELS.get(index).copied()
}

pub fn card_corner_input_tensor(image: &RgbImage, image_size: u32) -> F32Tensor {
  let resized = image::imageops::resize(image, image_size, image_size, FilterType::Triangle);
  let size = image_size as usize;
  let plane_size = size * size;
  let mut data = vec![0.0; 3 * plane_size];

  for (x, y, pixel) in resized.enumerate_pixels() {
    let offset = y as usize * size + x as usize;
    data[offset] = f32::from(pixel[0]) / 255.0;
    data[plane_size + offset] = f32::from(pixel[1]) / 255.0;
    data[plane_size * 2 + offset] = f32::from(pixel[2]) / 255.0;
  }

  F32Tensor {
    name: CARD_CORNER_INPUT_NAME.to_string(),
    shape: vec![1, 3, size, size],
    data,
  }
}

pub fn prediction_from_logits(
  rank_logits: &F32Tensor,
  suit_logits: &F32Tensor,
) -> InferenceResult<CardCornerPrediction> {
  let rank_probabilities = softmax(&rank_logits.data);
  let suit_probabilities = softmax(&suit_logits.data);
  let rank = top1(&rank_probabilities).ok_or_else(|| InferenceError::Backend {
    message: "rank logits were empty".to_string(),
  })?;
  let suit = top1(&suit_probabilities).ok_or_else(|| InferenceError::Backend {
    message: "suit logits were empty".to_string(),
  })?;
  let rank_label = rank_label(rank.index).ok_or_else(|| InferenceError::MissingClassLabel {
    class_id: rank.index,
  })?;
  let suit_label = suit_label(suit.index).ok_or_else(|| InferenceError::MissingClassLabel {
    class_id: suit.index,
  })?;

  Ok(CardCornerPrediction {
    rank: rank_label.to_string(),
    suit: suit_label.to_string(),
    rank_confidence: rank.confidence,
    suit_confidence: suit.confidence,
  })
}

fn named_output<'a>(outputs: &'a [F32Tensor], name: &str) -> InferenceResult<&'a F32Tensor> {
  outputs
    .iter()
    .find(|output| output.name == name)
    .ok_or_else(|| InferenceError::Backend {
      message: format!("missing ONNX output: {name}"),
    })
}

#[cfg(test)]
mod tests {
  use auv_inference_ort::F32Tensor;
  use image::{Rgb, RgbImage};

  use crate::card_corner::{
    CardCornerPrediction, card_corner_input_tensor, prediction_from_logits, rank_label, suit_label,
  };

  #[test]
  fn rank_and_suit_labels_match_training_order() {
    assert_eq!(rank_label(0), Some("A"));
    assert_eq!(rank_label(9), Some("10"));
    assert_eq!(rank_label(12), Some("K"));
    assert_eq!(rank_label(13), None);

    assert_eq!(suit_label(0), Some("spades"));
    assert_eq!(suit_label(1), Some("hearts"));
    assert_eq!(suit_label(2), Some("clubs"));
    assert_eq!(suit_label(3), Some("diamonds"));
    assert_eq!(suit_label(4), None);
  }

  #[test]
  fn input_tensor_resizes_and_normalizes_rgb_to_chw() {
    let mut image = RgbImage::new(1, 1);
    image.put_pixel(0, 0, Rgb([64, 128, 255]));

    let tensor = card_corner_input_tensor(&image, 2);

    assert_eq!(tensor.name, "images");
    assert_eq!(tensor.shape, vec![1, 3, 2, 2]);
    assert_eq!(tensor.data.len(), 12);
    assert!((tensor.data[0] - 64.0 / 255.0).abs() < 1e-6);
    assert!((tensor.data[4] - 128.0 / 255.0).abs() < 1e-6);
    assert!((tensor.data[8] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn prediction_from_logits_uses_softmax_confidence() {
    let prediction = prediction_from_logits(
      &F32Tensor {
        name: "rank_logits".to_string(),
        shape: vec![1, 13],
        data: vec![
          0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ],
      },
      &F32Tensor {
        name: "suit_logits".to_string(),
        shape: vec![1, 4],
        data: vec![0.0, 0.0, 9.0, 0.0],
      },
    )
    .unwrap();

    assert_eq!(
      prediction,
      CardCornerPrediction {
        rank: "2".to_string(),
        suit: "clubs".to_string(),
        rank_confidence: prediction.rank_confidence,
        suit_confidence: prediction.suit_confidence,
      }
    );
    assert!(prediction.rank_confidence > 0.99);
    assert!(prediction.suit_confidence > 0.99);
  }
}
