mod accessibility;
mod application;
mod descriptor;
mod driver;
mod readiness;
mod session;

// TODO(driver-crates): These modules are temporarily public so the root
// command adapter can build while command-facing code migrates to typed
// session APIs. Do not treat them as stable crate API.
#[doc(hidden)]
pub mod capture;
#[doc(hidden)]
pub mod constants;
#[doc(hidden)]
pub mod observe;
#[doc(hidden)]
pub mod support;
#[doc(hidden)]
pub mod types;

// TODO(driver-crates): This is a temporary compatibility surface for the root
// crate while legacy macOS command code is moved behind typed session APIs.
#[doc(hidden)]
pub mod native;

pub use accessibility::{DEFAULT_AX_MAX_CHILDREN, DEFAULT_AX_MAX_DEPTH};
pub use application::ApplicationControl;
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
pub use auv_driver_common::{AxFocusResult, AxTextRead};
pub use descriptor::{MacosDriverDescriptor, macos_driver_descriptor};
pub use driver::{MacosDriver, MacosDriverSession};
pub use readiness::assess_readiness;
pub use session::{AccessibilityApi, ClipboardApi, InputApi, PermissionApi, VisionApi, WindowApi};
pub use types::{ObservedAxNode, ObservedAxTreeSnapshot};
