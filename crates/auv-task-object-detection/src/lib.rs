pub mod render;
pub mod types;

pub use auv_inference_common::{BoundingBox, ImageSize};
pub use render::render_annotated_image;
pub use types::{Detection, DetectionOptions, DetectionResult};
