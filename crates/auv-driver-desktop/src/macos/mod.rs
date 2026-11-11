//! macOS desktop discovery.
pub mod display;

/// The existing xcap display discovery backend.
#[cfg(feature = "macos-xcap")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Xcap;
#[cfg(feature = "macos-xcap")]
impl auv_driver_desktop_common::DisplayDriver for Xcap {
  fn list_displays(&mut self) -> auv_driver_common::DriverResult<auv_driver_common::ObservedDisplays> {
    display::list_displays()
  }
}
