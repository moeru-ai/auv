#[cfg(target_os = "linux")]
pub(crate) mod keymap;
#[cfg(target_os = "linux")]
pub(crate) mod uinput;

#[cfg(target_os = "linux")]
pub mod portal;

use auv_driver_common::error::DriverResult;
use auv_driver_common::input::Scroll;

// NOTICE(scroll-pixels-per-wheel-notch): one wheel notch (uinput REL_WHEEL or
// portal NotifyPointerAxisDiscrete) scrolled Chromium by 120 CSS px on GNOME
// Wayland (`neko-gpu-1`, 2026-10-06), so AUV maps 120 logical pixels to one
// notch. See `docs/ai/references/driver/2026-10-06-scroll-delta-contract.md`.
// TODO(linux-portal-hi-res-wheel): the RemoteDesktop portal (v2) has no
// high-resolution discrete axis, so portal deltas below one notch accumulate
// within one input session. uinput uses REL_WHEEL_HI_RES instead.
pub(crate) const SCROLL_PIXELS_PER_WHEEL_NOTCH: f64 = 120.0;

/// High-resolution wheel units per notch (`REL_WHEEL_HI_RES`, kernel convention).
pub(crate) const HI_RES_UNITS_PER_WHEEL_NOTCH: f64 = 120.0;

/// Converts AUV logical-pixel scroll deltas into whole `(horizontal, vertical)`
/// wheel notches, positive toward later content (right/down), plus the
/// fractional remainder carried to the next scroll in the same input session.
pub(crate) fn wheel_notches(remainder: (f64, f64), scroll: Scroll) -> DriverResult<((i32, i32), (f64, f64))> {
  wheel_steps(remainder, scroll, 1.0 / SCROLL_PIXELS_PER_WHEEL_NOTCH)
}

/// Converts AUV logical-pixel scroll deltas into whole `(horizontal, vertical)`
/// high-resolution wheel units, positive toward later content, plus the
/// fractional remainder carried within one input session.
pub(crate) fn wheel_hi_res_units(remainder: (f64, f64), scroll: Scroll) -> DriverResult<((i32, i32), (f64, f64))> {
  let per_pixel = HI_RES_UNITS_PER_WHEEL_NOTCH / SCROLL_PIXELS_PER_WHEEL_NOTCH;
  wheel_steps(remainder, scroll, per_pixel)
}

fn wheel_steps(remainder: (f64, f64), scroll: Scroll, steps_per_pixel: f64) -> DriverResult<((i32, i32), (f64, f64))> {
  if !scroll.delta_x.is_finite() || !scroll.delta_y.is_finite() {
    return Err(crate::error::invalid_input("scroll delta must be finite"));
  }
  let x = remainder.0 + scroll.delta_x * steps_per_pixel;
  let y = remainder.1 + scroll.delta_y * steps_per_pixel;
  if x.abs() > f64::from(i32::MAX) || y.abs() > f64::from(i32::MAX) {
    return Err(crate::error::invalid_input("scroll delta exceeds the wheel range"));
  }
  Ok(((x.trunc() as i32, y.trunc() as i32), (x.fract(), y.fract())))
}

#[cfg(test)]
mod wheel_tests {
  use super::*;

  #[test]
  fn wheel_notches_map_120_logical_pixels_to_one_notch_toward_later_content() {
    // ROOT CAUSE:
    //
    // If a caller sent 120 logical pixels on Linux, uinput emitted 8 notches
    // (15 px per notch) and the portal sent a continuous finger-scroll axis
    // that GNOME scaled by 12 and continued kinetically.
    //
    // Before the fix, Chromium scrolled about 960-5700 px for a 120 px request.
    // The fix maps 120 logical pixels to one discrete notch on both backends.
    assert_eq!(wheel_notches((0.0, 0.0), Scroll::new(0.0, 120.0)).unwrap(), ((0, 1), (0.0, 0.0)));
    assert_eq!(wheel_notches((0.0, 0.0), Scroll::new(240.0, -360.0)).unwrap(), ((2, -3), (0.0, 0.0)));
    let (notches, remainder) = wheel_notches((0.0, 0.0), Scroll::new(0.0, 60.0)).unwrap();
    assert_eq!(notches, (0, 0));
    assert_eq!(wheel_notches(remainder, Scroll::new(0.0, 60.0)).unwrap(), ((0, 1), (0.0, 0.0)));
    assert!(wheel_notches((0.0, 0.0), Scroll::new(f64::NAN, 0.0)).is_err());
  }

  #[test]
  fn wheel_hi_res_units_map_one_logical_pixel_to_one_unit_and_carry_fractions() {
    assert_eq!(wheel_hi_res_units((0.0, 0.0), Scroll::new(-30.0, 45.0)).unwrap(), ((-30, 45), (0.0, 0.0)));
    let (units, remainder) = wheel_hi_res_units((0.0, 0.0), Scroll::new(0.0, 0.6)).unwrap();
    assert_eq!(units, (0, 0));
    assert_eq!(wheel_hi_res_units(remainder, Scroll::new(0.0, 0.6)).unwrap().0, (0, 1));
  }
}

#[cfg(not(target_os = "linux"))]
pub mod portal {
  use auv_driver_common::error::{DriverError, DriverResult};
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::{Click, Scroll};

  // NOTICE(linux-portal-nonlinux-stub): the real portal sessions depend on Linux-only
  // crates (`zbus`, `pipewire`) wired under target-specific Cargo dependencies. Keep a
  // narrow unsupported stub on non-Linux targets so cross-target analysis can compile
  // the crate without pretending those capabilities exist.

  #[derive(Debug, Default)]
  pub struct PortalClipboard;

  impl PortalClipboard {
    pub fn open(_app_id: Option<&String>) -> DriverResult<ClipboardSession> {
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
    pub fn open(_restore_tokens: Option<&RestoreTokenStore>, _app_id: Option<&String>) -> DriverResult<InputSession> {
      Err(DriverError::unsupported("linux.portal.input"))
    }
  }

  #[derive(Clone, Debug, Default)]
  pub struct RestoreTokenStore;

  impl RestoreTokenStore {
    pub(crate) fn new(_root: std::path::PathBuf) -> Self {
      Self
    }
  }

  #[derive(Debug, Default)]
  pub struct InputSession;

  impl InputSession {
    pub fn button(&mut self, _button: auv_driver_common::MouseButton, _down: bool) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn move_to(&mut self, _point: Point) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn click_at(
      &mut self,
      _point: Point,
      _button: auv_driver_common::MouseButton,
      _click: Click,
      _modifiers: &[i32],
    ) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn scroll(&mut self, _scroll: Scroll) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn scroll_at(&mut self, _point: Point, _scroll: Scroll) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn key_press(&mut self, _keysym: i32) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn key_transition(&mut self, _keysym: i32, _down: bool) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }

    pub fn key_chord(&mut self, _modifiers: &[i32], _key: i32) -> DriverResult<()> {
      Err(DriverError::unsupported("linux.portal.input"))
    }
  }

  #[derive(Debug, Default)]
  pub struct ScreenCastSession;

  impl ScreenCastSession {
    pub fn open_monitor(_restore_tokens: Option<&RestoreTokenStore>, _app_id: Option<&String>) -> DriverResult<Self> {
      Err(DriverError::unsupported("linux.portal.screencast"))
    }
  }
}
