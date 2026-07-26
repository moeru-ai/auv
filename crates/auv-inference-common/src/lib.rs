pub mod error;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use types::{BoundingBox, ImageFrame, ImageSize, ModelConfig, ModelId};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
