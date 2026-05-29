// File: src/driver/macos/support/geometry.rs
use super::super::{
  DriverCall, ObservedDisplaySnapshot, ObservedPointResolution, ObservedRect, OcrTextMatch,
  WindowRef,
};
use super::call::optional_f64;
use crate::model::AuvResult;
#[cfg(test)]
use auv_driver_macos::types::ObservedWindow;

#[cfg(test)]
pub(crate) fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name
    || app_identifier.contains(&app_name)
    || app_name.contains(&app_identifier)
}

#[cfg(test)]
pub(crate) fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

pub(crate) fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  auv_driver_macos::support::ocr_match_center(matched)
}

pub(crate) fn project_main_screenshot_point(
  snapshot: &ObservedDisplaySnapshot,
  screenshot_x: f64,
  screenshot_y: f64,
) -> AuvResult<(f64, f64)> {
  auv_driver_macos::support::project_main_screenshot_point(snapshot, screenshot_x, screenshot_y)
}

pub(crate) fn resolve_window_point(
  call: &DriverCall,
  window: &WindowRef,
) -> AuvResult<(f64, f64, String)> {
  let offset_x = optional_f64(call, "offset_x")?;
  let offset_y = optional_f64(call, "offset_y")?;
  let relative_x = optional_f64(call, "relative_x")?;
  let relative_y = optional_f64(call, "relative_y")?;

  match (offset_x, offset_y, relative_x, relative_y) {
    (Some(offset_x), Some(offset_y), None, None) => Ok((
      window.bounds.x as f64 + offset_x,
      window.bounds.y as f64 + offset_y,
      format!("windowOffset={offset_x:.3},{offset_y:.3}"),
    )),
    (None, None, Some(relative_x), Some(relative_y)) => {
      if !(0.0..=1.0).contains(&relative_x) || !(0.0..=1.0).contains(&relative_y) {
        return Err(
          "relative window coordinates must be within 0.0..=1.0 for both axes".to_string(),
        );
      }
      Ok((
        window.bounds.x as f64 + (window.bounds.width as f64 * relative_x),
        window.bounds.y as f64 + (window.bounds.height as f64 * relative_y),
        format!("windowRelative={relative_x:.3},{relative_y:.3}"),
      ))
    }
    (Some(_), None, _, _) | (None, Some(_), _, _) => {
      Err("window point offset mode requires both --offset_x and --offset_y".to_string())
    }
    (_, _, Some(_), None) | (_, _, None, Some(_)) => {
      Err("window point relative mode requires both --relative_x and --relative_y".to_string())
    }
    (Some(_), Some(_), Some(_), Some(_)) => {
      Err("use either --offset_x/--offset_y or --relative_x/--relative_y, not both".to_string())
    }
    _ => Err(
      "operation requires either --offset_x/--offset_y or --relative_x/--relative_y".to_string(),
    ),
  }
}

pub(crate) fn resolve_display_point(
  snapshot: &ObservedDisplaySnapshot,
  x: f64,
  y: f64,
) -> Option<ObservedPointResolution> {
  auv_driver_macos::support::resolve_display_point(snapshot, x, y)
}

pub(crate) fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}
