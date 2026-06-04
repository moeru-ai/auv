use image::RgbImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImageSize {
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageFrame {
  pub image: RgbImage,
}

impl ImageFrame {
  pub fn new(image: RgbImage) -> Self {
    Self { image }
  }

  pub fn size(&self) -> ImageSize {
    ImageSize {
      width: self.image.width(),
      height: self.image.height(),
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct BoundingBox {
  pub x1: f32,
  pub y1: f32,
  pub x2: f32,
  pub y2: f32,
}

impl BoundingBox {
  pub fn width(&self) -> f32 {
    self.x2 - self.x1
  }

  pub fn height(&self) -> f32 {
    self.y2 - self.y1
  }

  pub fn area(&self) -> f32 {
    self.width().max(0.0) * self.height().max(0.0)
  }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  pub bbox: BoundingBox,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionSet {
  pub model_id: ModelId,
  pub image_size: ImageSize,
  pub detections: Vec<Detection>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionOptions {
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
}

impl Default for DetectionOptions {
  fn default() -> Self {
    Self {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
    }
  }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
}
