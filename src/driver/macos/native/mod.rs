// TODO(driver-crates): temporary root-native compatibility while legacy macOS
// command code migrates to typed `auv-driver-macos` session APIs.

pub(crate) mod ax_tree {
  pub(crate) use auv_driver_macos::native::ax_tree::*;
}

pub(crate) mod clipboard {
  pub(crate) use auv_driver_macos::native::clipboard::*;
}

pub(crate) mod ocr {
  pub(crate) use auv_driver_macos::native::ocr::*;
}

pub(crate) mod permission {
  pub(crate) use auv_driver_macos::native::permission::*;
}

pub(crate) mod pointer {
  pub(crate) use auv_driver_macos::native::pointer::*;
}

pub(crate) mod window {
  pub(crate) use auv_driver_macos::native::window::*;
}

pub(crate) mod overlay {
  use crate::model::AuvResult;

  fn unsupported() -> AuvResult<()> {
    Err(
      "macOS native overlay is temporarily unsupported while overlay moves to auv-overlay-macos"
        .to_string(),
    )
  }

  pub(crate) fn show_cursor(_x: f64, _y: f64, _label: &str) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn show_dual_cursor(
    _x: f64,
    _y: f64,
    _label: &str,
    _user_label: &str,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn set_cursor(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _variant: &str,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn move_cursor(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _variant: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn move_dual_cursor(
    _x: f64,
    _y: f64,
    _label: &str,
    _user_label: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn flash_cursor(_x: f64, _y: f64, _label: &str, _duration_ms: u64) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn flash_cursor_id(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn hide_cursor_id(_cursor_id: &str) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn hide_cursor() -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn pump_events(_duration_ms: u64) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn shutdown() -> AuvResult<()> {
    unsupported()
  }
}
