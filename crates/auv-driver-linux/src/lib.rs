//! Linux desktop driver capabilities for AUV.
//!
//! The first Linux slice is intentionally Wayland-friendly and capability
//! oriented: it exposes shared driver/session types, records portal readiness,
//! and validates live desktop capture through XDG desktop portal screenshots
//! plus Wayland xdg-output display geometry.
//! Foreground input uses RemoteDesktop Portal or an explicitly selected uinput
//! virtual device backend.

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
pub use driver::{InputBackend, LinuxDriver, LinuxDriverSession};
pub use ocr::{OcrError, recognize_text_in_rgba};
pub use permission::{LinuxPortalProbe, PortalInterfaceProbe, probe_portals};
pub use session::{AccessibilityApi, ClipboardApi, DisplayApi, InputApi, PermissionApi, VisionApi, WindowApi};

#[cfg(target_os = "linux")]
pub use permission::{kde_authorization, set_kde_authorization, verify_portal_identity};
