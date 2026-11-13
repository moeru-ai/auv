//! The existing xcap Windows display discovery.
#![cfg_attr(
  not(all(target_os = "windows", feature = "windows-xcap")),
  allow(unused_imports)
)]
use auv_driver_common::error::{backend, not_found};
use auv_driver_common::{CoordinateSpace, Display, DriverError, DriverResult, ObservedDisplays, Rect};
pub use auv_driver_desktop_common::display::{
  DisplayTarget, resolve_for_region as resolve_display_for_region, resolve_target as resolve_display_target,
};

#[cfg(all(target_os = "windows", feature = "windows-xcap"))]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  Ok(ObservedDisplays {
    displays: display_targets_from_monitors(&monitors)?.into_iter().map(|target| target.display).collect(),
  })
}

#[cfg(not(all(target_os = "windows", feature = "windows-xcap")))]
pub fn list_displays() -> DriverResult<ObservedDisplays> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(all(target_os = "windows", feature = "windows-xcap"))]
pub fn display_targets_from_monitors(monitors: &[xcap::Monitor]) -> DriverResult<Vec<DisplayTarget>> {
  if monitors.is_empty() {
    return Err(not_found("display"));
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
          // NOTICE: xcap exposes `is_builtin` on macOS; on Windows the concept
          // is not meaningful, so the shared `is_builtin` flag stays None.
          is_builtin: None,
        },
      })
    })
    .collect()
}
