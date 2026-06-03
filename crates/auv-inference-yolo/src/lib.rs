pub mod decode;
pub mod detector;
pub mod error;
pub mod letterbox;
pub mod nms;
pub mod render;
pub mod types;

pub use detector::YoloDetector;
pub use error::{YoloError, YoloResult};
pub use render::render_annotated_image;
pub use types::{
  BoundingBox, Detection, DetectionOptions, DetectionSet, ImageFrame, ImageSize, ModelId,
  YoloFamily, YoloModelConfig,
};

// TODO(auv-inference-yolo-followups): module bodies are intentionally deferred
// until the owner-approved decode, detector, letterbox, nms, and render slices.
