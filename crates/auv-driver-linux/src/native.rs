#[cfg(target_os = "linux")]
pub mod portal;

#[cfg(not(target_os = "linux"))]
pub mod portal {
  use auv_driver::error::{DriverError, DriverResult};
  use auv_driver::geometry::Point;
  use auv_driver::input::{Click, Scroll};

  // NOTICE(linux-portal-nonlinux-stub): the real portal sessions depend on Linux-only
  // crates (`zbus`, `pipewire`) wired under target-specific Cargo dependencies. Keep a
  // narrow unsupported stub on non-Linux targets so cross-target analysis can compile
  // the crate without pretending those capabilities exist.

  #[derive(Debug, Default)]
  pub struct PortalClipboard;

  impl PortalClipboard {
    pub fn open() -> DriverResult<ClipboardSession> {
      Err(DriverError::unsupported("linux.portal.clipboard"))
    }
  }

  #[derive(Debug, Default)]
  pub struct ClipboardSession;

  impl ClipboardSession {
    pub fn snapshot(&mut self) -> DriverResult<String> {
      Err(DriverError::unsupported("linux.portal.clipboard"))
    }

    pub fn set_text(&mut self, _text: &str) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.clipboard"))
    }
  }

  #[derive(Debug, Default)]
  pub struct PortalInput;

  impl PortalInput {
    pub fn open() -> DriverResult<InputSession> {
      Err(DriverError::unsupported("linux.portal.input"))
    }
  }

  #[derive(Debug, Default)]
  pub struct InputSession;

  impl InputSession {
    pub fn click_at(&mut self, _point: Point, _click: Click) -> DriverResult<Option<String>> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn scroll(&mut self, _scroll: Scroll) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn key_press(&mut self, _keysym: i32) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn key_chord(&mut self, _modifiers: &[i32], _key: i32) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }
  }
}
