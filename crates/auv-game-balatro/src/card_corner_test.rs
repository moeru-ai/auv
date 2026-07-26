use auv_inference_ort::F32Tensor;
use image::{Rgb, RgbImage};

use crate::card_corner::{CardCornerPrediction, card_corner_input_tensor, prediction_from_logits, rank_label, suit_label};

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
