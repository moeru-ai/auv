//! The existing xcap macOS display discovery and permission errors.
#![cfg_attr(
  not(all(target_os = "macos", feature = "macos-xcap")),
  allow(unused_imports)
)]
use auv_driver_common::error::backend;
use auv_driver_common::{CoordinateSpace, Display, DriverError, DriverResult, ObservedDisplays, Rect};
pub use auv_driver_desktop_common::display::{
  DisplayTarget, resolve_for_region as resolve_display_for_global_region, resolve_target as resolve_display_target,
};

pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Ok(ObservedDisplays {
    displays: list_display_targets()?.into_iter().map(|target| target.display).collect(),
  })
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
pub fn list_display_targets() -> DriverResult<Vec<DisplayTarget>> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  display_targets_from_monitors(&monitors)
}

#[cfg(not(all(target_os = "macos", feature = "macos-xcap")))]
pub fn list_display_targets() -> DriverResult<Vec<DisplayTarget>> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(all(target_os = "macos", feature = "macos-xcap"))]
pub fn display_targets_from_monitors(monitors: &[xcap::Monitor]) -> DriverResult<Vec<DisplayTarget>> {
  if monitors.is_empty() {
    return Err(DriverError::PermissionDenied {
      permission: "screen_recording",
      message: None,
      recovery: Some(
        "xcap returned no displays; run `auv doctor --json` and grant Screen Recording to the terminal or agent process that launches AUV"
          .to_string(),
      ),
    });
  }
  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor.x().map_err(|error| backend(format!("failed to read display x: {error}")))? as f64;
      let y = monitor.y().map_err(|error| backend(format!("failed to read display y: {error}")))? as f64;
      let width = monitor.width().map_err(|error| backend(format!("failed to read display width: {error}")))? as f64;
      let height = monitor.height().map_err(|error| backend(format!("failed to read display height: {error}")))? as f64;
      let scale_factor = monitor.scale_factor().map_err(|error| backend(format!("failed to read display scale: {error}")))? as f64;
      let is_builtin = monitor.is_builtin().map_err(|error| backend(format!("failed to read display built-in flag: {error}")))?;
      let native_id = monitor.id().map_err(|error| backend(format!("failed to read display id: {error}")))?.to_string();
      Ok(DisplayTarget {
        index,
        display: Display {
          id: native_id,
          name: Some(format!("display_{index}")),
          frame: Rect::new(x, y, width, height),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor,
          is_primary: monitor.is_primary().map_err(|error| backend(format!("failed to read display primary flag: {error}")))?,
          is_builtin: Some(is_builtin),
        },
      })
    })
    .collect()
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
