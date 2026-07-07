pub mod render;
pub mod types;

#[cfg(feature = "ultralytics")]
pub mod ultralytics;

pub use auv_inference_common::{BoundingBox, ImageSize};
pub use render::render_annotated_image;
pub use types::{Detection, DetectionOptions, DetectionResult};

#[cfg(feature = "ultralytics")]
pub use ultralytics::{UltralyticsObjectDetector, UltralyticsObjectDetectorConfig, detection_result_from_prediction};
