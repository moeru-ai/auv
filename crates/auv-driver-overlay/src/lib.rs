//! Platform-selecting overlay facade.
//!
//! Enable `macos` to route [`show`] and [`remove`] through the AppKit adapter.
//! Renderer-independent types are re-exported from `auv-driver-overlay-common`.

mod error;
mod theme;

pub use theme::theme_from_env;

pub use auv_driver_overlay_common::*;
pub use error::{OverlayError, OverlayResult};

/// Shows an overlay using the rendering process's `AUV_OVERLAY_THEME` defaults.
///
/// Host theme fields override supplied layer styles. Use [`show_with_theme`] to
/// select an explicit per-call theme without consulting the environment.
pub fn show(overlay: &Overlay, options: ShowOptions) -> OverlayResult<()> {
  let theme = theme_from_env()?;
  match theme {
    Some(theme) => show_with_theme(overlay, options, &theme),
    None => render(overlay, options),
  }
}

/// Shows an overlay with a typed per-call theme instead of environment defaults.
/// An empty theme preserves all layer styles. This is a local Rust API; remote
/// callers can use [`OverlayTheme::apply`] before sending their existing layers.
pub fn show_with_theme(overlay: &Overlay, options: ShowOptions, theme: &OverlayTheme) -> OverlayResult<()> {
  let overlay = theme.apply(overlay).map_err(|message| OverlayError::InvalidTheme { message })?;
  render(&overlay, options)
}

fn render(overlay: &Overlay, options: ShowOptions) -> OverlayResult<()> {
  #[cfg(all(target_os = "macos", feature = "macos"))]
  {
    return auv_driver_overlay_macos::render(overlay, options).map_err(OverlayError::backend);
  }

  #[cfg(all(target_os = "windows", feature = "windows"))]
  {
    return auv_driver_overlay_windows::render(overlay, options).map_err(OverlayError::backend);
  }

  #[cfg(not(any(
    all(target_os = "macos", feature = "macos"),
    all(target_os = "windows", feature = "windows")
  )))]
  {
    let _ = (overlay, options);
    Err(OverlayError::Unavailable {
      reason: "no overlay platform adapter is enabled for this target".to_string(),
    })
  }
}

/// Removes all layers owned by the enabled platform adapter.
pub fn remove() -> OverlayResult<()> {
  #[cfg(all(target_os = "macos", feature = "macos"))]
  {
    return auv_driver_overlay_macos::remove().map_err(OverlayError::backend);
  }

  #[cfg(all(target_os = "windows", feature = "windows"))]
  {
    return auv_driver_overlay_windows::remove().map_err(OverlayError::backend);
  }

  #[cfg(not(any(
    all(target_os = "macos", feature = "macos"),
    all(target_os = "windows", feature = "windows")
  )))]
  {
    Err(OverlayError::Unavailable {
      reason: "no overlay platform adapter is enabled for this target".to_string(),
    })
  }
}
