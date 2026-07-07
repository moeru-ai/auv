//! Windows driver capabilities for AUV.
//!
//! This crate exposes Windows-native capabilities behind narrow,
//! capability-oriented modules, mirroring the macOS driver crate. The first
//! capability is system OCR backed by `Windows.Media.Ocr`.

pub mod accessibility;
pub mod capture;
pub mod clipboard;
mod descriptor;
mod driver;
mod error;
pub mod input;
pub mod mutation;
pub mod ocr;
pub mod permission;
mod session;
pub mod vision;
pub mod window;

pub use accessibility::{AxNode, AxTreeSnapshot, focus_node, select_node, snapshot_window};
pub use descriptor::{WINDOWS_DESKTOP_CAPABILITIES, WindowsDriverDescriptor, windows_driver_descriptor};
pub use driver::{WindowsDriver, WindowsDriverSession};
pub use ocr::{OcrError, recognize_text_in_rgba};
pub use permission::{WindowsPermissionProbe, probe as probe_permissions};
pub use session::{AccessibilityApi, ClipboardApi, DisplayApi, InputApi, PermissionApi, VisionApi, WindowApi};
pub use vision::{OcrMatch, OcrMatches};
