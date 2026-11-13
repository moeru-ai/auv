//! Windows desktop discovery, target probes, and thread attachment resources.
#[cfg(feature = "windows-win32")]
pub mod attachment;
pub mod display;
#[cfg(feature = "windows-win32")]
pub mod target;

/// The existing xcap display discovery backend.
#[cfg(feature = "windows-xcap")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Xcap;
#[cfg(feature = "windows-xcap")]
impl auv_driver_desktop_common::DisplayDriver for Xcap {
  fn list_displays(&mut self) -> auv_driver_common::DriverResult<auv_driver_common::ObservedDisplays> {
    display::list_displays()
  }
}
