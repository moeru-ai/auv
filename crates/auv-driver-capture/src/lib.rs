//! Capture contracts and the local capture provider.
//!
//! TODO(capture-platform-extraction): macOS and Windows native implementations
//! remain in their existing driver crates until their platform extraction.
//! This facade currently exposes a standalone provider only on Linux.

pub use auv_driver_capture_common::*;

#[cfg(target_os = "linux")]
pub use auv_driver_capture_linux::Capturer;
