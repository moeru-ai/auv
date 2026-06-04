pub mod decode;
pub mod detector;
pub mod error;
pub mod letterbox;
pub mod nms;
// NOTICE: The YOLO-local renderer remains temporarily during transition.
// Migration/removal is deferred because `auv-inference-yolo` is scheduled for
// removal/replacement in Task 7 of
// `docs/superpowers/plans/2026-06-04-ultralytics-inference-adapter.md`. Remove
// this duplicate surface after the Ultralytics adapter replacement lands.
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
