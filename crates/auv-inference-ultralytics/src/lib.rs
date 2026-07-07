pub mod detector;
pub mod device;

pub use detector::{UltralyticsBoxes, UltralyticsModelConfig, UltralyticsPrediction, UltralyticsResult, UltralyticsSession};
pub use device::InferenceDevice;
