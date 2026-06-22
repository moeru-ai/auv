//! Windows-specific NetEase Cloud Music window identity and resolution.
//!
//! Product selectors live here rather than in `auv-driver-windows`: the driver
//! owns generic Win32/UIA capabilities, while this crate owns the knowledge
//! that NetEase normally runs as `cloudmusic.exe`.

use auv_driver::window::Window;

pub const DEFAULT_PROCESS_NAME: &str = "cloudmusic.exe";
pub const DEFAULT_WINDOW_TITLE: &str = "网易云音乐";
pub const ENGLISH_WINDOW_TITLE: &str = "NetEase Cloud Music";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveOptions {
  pub process_name: String,
  pub title: String,
}

impl Default for ResolveOptions {
  fn default() -> Self {
    Self {
      process_name: DEFAULT_PROCESS_NAME.to_string(),
      title: DEFAULT_WINDOW_TITLE.to_string(),
    }
  }
}

#[cfg(target_os = "windows")]
pub fn resolve_window(options: &ResolveOptions) -> Result<Option<Window>, String> {
  use auv_driver::Driver;
  use auv_driver::error::DriverError;
  use auv_driver::selector::{App, Window as WindowSelector};
  use auv_driver_windows::WindowsDriver;

  let session = WindowsDriver::new()
    .open_local()
    .map_err(|error| format!("failed to open Windows driver: {error}"))?;

  let by_process = WindowSelector::main_visible().owned_by(App::name(options.process_name.clone()));
  match session.window().resolve(by_process) {
    Ok(window) => return Ok(Some(window)),
    Err(DriverError::NotFound { .. }) => {}
    Err(error) => {
      return Err(format!(
        "failed to resolve NetEase window by process name: {error}"
      ));
    }
  }

  let mut titles = vec![
    options.title.as_str(),
    DEFAULT_WINDOW_TITLE,
    ENGLISH_WINDOW_TITLE,
    "CloudMusic",
  ];
  titles.dedup();
  for title in titles {
    match session
      .window()
      .resolve(WindowSelector::title_contains(title))
    {
      Ok(window) => return Ok(Some(window)),
      Err(DriverError::NotFound { .. }) => {}
      Err(error) => {
        return Err(format!(
          "failed to resolve NetEase window by title {title:?}: {error}"
        ));
      }
    }
  }

  Ok(None)
}

#[cfg(not(target_os = "windows"))]
pub fn resolve_window(_options: &ResolveOptions) -> Result<Option<Window>, String> {
  Ok(None)
}
