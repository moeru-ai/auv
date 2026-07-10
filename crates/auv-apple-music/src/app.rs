//! Apple Music window resolution via the Windows driver.
//!
//! Apple Music ships as a Microsoft Store (MSIX) package on Windows. Its
//! top-level window carries the title "Apple Music" and its process is named
//! `AppleMusic.exe`. This module wraps the driver's window selector into a
//! single typed result so command implementations stay driver-agnostic.

use auv_driver::window::Window;

/// Default process name for Apple Music on Windows (Microsoft Store edition).
pub const APPLE_MUSIC_PROCESS_NAME: &str = "AppleMusic.exe";

/// Default title substring used to identify the Apple Music window.
pub const APPLE_MUSIC_TITLE: &str = "Apple Music";

/// A resolved Apple Music window ready for capture or input.
#[derive(Clone, Debug)]
pub struct AppleMusicWindow {
  pub window: Window,
}

/// Inputs controlling how window resolution is attempted.
#[derive(Clone, Debug)]
pub struct ResolveOptions {
  /// Process name to match. Defaults to [`APPLE_MUSIC_PROCESS_NAME`].
  pub process_name: String,
  /// Window title substring to match. Defaults to [`APPLE_MUSIC_TITLE`].
  pub title: String,
}

impl Default for ResolveOptions {
  fn default() -> Self {
    Self {
      process_name: APPLE_MUSIC_PROCESS_NAME.to_string(),
      title: APPLE_MUSIC_TITLE.to_string(),
    }
  }
}

/// Attempts to resolve the Apple Music window using the Windows driver.
///
/// Returns `Ok(Some(...))` when a matching window is found, `Ok(None)` when
/// none are visible, and `Err(...)` when the driver itself fails.
#[cfg(target_os = "windows")]
pub fn resolve_window(options: &ResolveOptions) -> Result<Option<AppleMusicWindow>, String> {
  use auv_driver::selector::{App, Window as WindowSel, WindowSelector};

  let session = auv_driver::open_local().map_err(|error| error.to_string())?;

  // Prefer matching by process name (app name) so we find the window even if
  // the title has been localised. Fall back to title-only when process name
  // matching returns nothing.
  let by_name_selector = WindowSelector {
    app: Some(App::name(&options.process_name)),
    title: None,
    main_visible: false,
  };

  match session.window().resolve(by_name_selector) {
    Ok(window) => return Ok(Some(AppleMusicWindow { window })),
    Err(auv_driver::error::DriverError::NotFound { .. }) => {}
    Err(e) => return Err(format!("window resolve by process name failed: {e}")),
  }

  // Title fallback: useful when the process name differs across Windows
  // editions or locales.
  let by_title_selector = WindowSel::title_contains(&options.title);
  match session.window().resolve(by_title_selector) {
    Ok(window) => Ok(Some(AppleMusicWindow { window })),
    Err(auv_driver::error::DriverError::NotFound { .. }) => Ok(None),
    Err(e) => Err(format!("window resolve by title failed: {e}")),
  }
}

/// Stub for non-Windows targets — always returns `Ok(None)`.
#[cfg(not(target_os = "windows"))]
pub fn resolve_window(_options: &ResolveOptions) -> Result<Option<AppleMusicWindow>, String> {
  Ok(None)
}
