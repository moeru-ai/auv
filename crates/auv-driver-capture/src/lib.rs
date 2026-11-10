//! Native capture implementations selected explicitly through Cargo features.
//! Contracts are owned by `auv-driver-capture-common`; enabling a backend never
//! changes the backend selection or fallback policy of an existing session.

pub use auv_driver_capture_common::{
  Activation, Capture, CaptureOptions, CaptureResolution, DisplayCapture, DisplayCaptureDriver, ImageView, RegionCapture,
  RegionCaptureDriver, WindowCaptureDriver,
};

#[cfg(all(
  target_os = "linux",
  any(feature = "linux-screencast", feature = "linux-screenshot")
))]
pub mod linux;

#[cfg(any(feature = "macos-xcap", feature = "macos-screencapturekit"))]
pub mod macos;

#[cfg(any(
  feature = "windows-xcap",
  feature = "windows-printwindow",
  feature = "windows-wgc"
))]
pub mod windows;
