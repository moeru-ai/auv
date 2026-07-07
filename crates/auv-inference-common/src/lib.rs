pub mod error;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use types::{BoundingBox, ImageFrame, ImageSize, ModelConfig, ModelId};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn image_frame_reports_rgb_image_size() {
    let frame = ImageFrame::new(image::RgbImage::new(12, 7));

    assert_eq!(
      frame.size(),
      ImageSize {
        width: 12,
        height: 7
      }
    );
  }
}
