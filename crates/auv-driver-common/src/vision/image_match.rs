use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

#[derive(Clone, Debug, PartialEq)]
pub struct ImageMatchOptions {
  pub threshold: f32,
  pub region: Option<Rect>,
}

impl Default for ImageMatchOptions {
  fn default() -> Self {
    Self {
      threshold: 0.9,
      region: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageMatch {
  pub bounds: Rect,
  pub score: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageMatchResult {
  pub matches: Vec<ImageMatch>,
}
