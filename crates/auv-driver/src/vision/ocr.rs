use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognizedText {
  pub text: String,
  pub bounds: Rect,
  pub confidence: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRecognition {
  pub text: String,
  pub regions: Vec<RecognizedText>,
}
