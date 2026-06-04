pub mod error;
pub mod render;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use render::render_annotated_image;
pub use types::{
  BoundingBox, ClassLabelSource, Detection, DetectionCoordinateSpace, DetectionEvidenceManifest,
  DetectionOptions, DetectionSet, ImageFrame, ImageSize, ModelConfig, ModelId, ModelRunMetadata,
  ProjectionBasis, SourceImageEvidence, SourceImageRef,
};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detection_options_default_matches_inference_defaults() {
    let options = DetectionOptions::default();

    assert_eq!(options.confidence_threshold, 0.25);
    assert_eq!(options.iou_threshold, 0.45);
    assert_eq!(options.max_detections, 300);
  }

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
