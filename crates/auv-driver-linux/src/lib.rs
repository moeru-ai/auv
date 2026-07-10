//! Linux desktop driver capabilities for AUV.
//!
//! The first Linux slice is intentionally Wayland-friendly and capability
//! oriented: it exposes shared driver/session types, records portal readiness,
//! and validates live desktop capture through XDG desktop portal screenshots
//! plus Wayland xdg-output display geometry.
//! RemoteDesktop/libei input delivery is reserved until the portal session
//! lifecycle is wired end to end.

mod accessibility;
#[cfg(target_os = "linux")]
mod atspi;
#[cfg(not(target_os = "linux"))]
mod atspi_stub;
mod capture;
mod clipboard;
mod descriptor;
mod driver;
mod error;
pub mod input;
mod native;
pub mod ocr;
mod permission;
mod session;
pub mod vision;
mod window;
#[cfg(not(target_os = "linux"))]
pub(crate) use atspi_stub as atspi;

pub use accessibility::{AxNode, AxTreeSnapshot};
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
pub use descriptor::{LINUX_DESKTOP_CAPABILITIES, LinuxDriverDescriptor, linux_driver_descriptor};
pub use driver::{LinuxDriver, LinuxDriverSession};
pub use ocr::{OcrError, recognize_text_in_rgba};
pub use permission::{LinuxPortalProbe, PortalInterfaceProbe, probe_portals};
pub use session::{AccessibilityApi, ClipboardApi, DisplayApi, InputApi, PermissionApi, VisionApi, WindowApi};
