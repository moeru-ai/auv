//! Linux desktop discovery through Wayland xdg-output.
pub mod display;

/// The existing Wayland display discovery backend.
#[cfg(feature = "linux-wayland")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Wayland;
#[cfg(feature = "linux-wayland")]
impl auv_driver_desktop_common::DisplayDriver for Wayland {
  fn list_displays(&mut self) -> auv_driver_common::DriverResult<auv_driver_common::ObservedDisplays> {
    display::list()
  }
}
