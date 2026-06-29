//! Linux desktop driver capabilities for AUV.
//!
//! The first Linux slice is intentionally Wayland-friendly and capability
//! oriented: it exposes shared driver/session types, records portal readiness,
//! and validates live desktop capture through `xcap`. RemoteDesktop/libei input
//! delivery is reserved until the portal session lifecycle is wired end to end.

mod accessibility;
mod atspi;
mod capture;
mod descriptor;
mod driver;
mod error;
pub mod input;
pub mod ocr;
mod permission;
mod session;
pub mod vision;
mod window;

pub use descriptor::{LINUX_DESKTOP_CAPABILITIES, LinuxDriverDescriptor, linux_driver_descriptor};
pub use driver::{LinuxDriver, LinuxDriverSession};
pub use ocr::{OcrError, recognize_text_in_rgba};
pub use permission::{LinuxPortalProbe, PortalInterfaceProbe, probe_portals};
pub use session::{AccessibilityApi, DisplayApi, InputApi, PermissionApi, VisionApi, WindowApi};
pub use vision::{OcrMatch, OcrMatches};
