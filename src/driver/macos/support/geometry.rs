use super::super::*;
use super::call::optional_f64;

pub(crate) fn compute_combined_bounds(displays: &[ObservedDisplay]) -> ObservedRect {
  let min_x = displays
    .iter()
    .map(|display| display.bounds.x)
    .min()
    .unwrap_or(0);
  let min_y = displays
    .iter()
    .map(|display| display.bounds.y)
    .min()
    .unwrap_or(0);
  let max_x = displays
    .iter()
    .map(|display| display.bounds.x + display.bounds.width)
    .max()
    .unwrap_or(0);
  let max_y = displays
    .iter()
    .map(|display| display.bounds.y + display.bounds.height)
    .max()
    .unwrap_or(0);

  ObservedRect {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  }
}

pub(crate) fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name
    || app_identifier.contains(&app_name)
    || app_name.contains(&app_identifier)
}

pub(crate) fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

pub(crate) fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  (
    matched.bounds.x as f64 + (matched.bounds.width as f64 / 2.0),
    matched.bounds.y as f64 + (matched.bounds.height as f64 / 2.0),
  )
}

pub(crate) fn project_main_screenshot_point(
  snapshot: &ObservedDisplaySnapshot,
  screenshot_x: f64,
  screenshot_y: f64,
) -> AuvResult<(f64, f64)> {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  if screenshot_x < 0.0
    || screenshot_y < 0.0
    || screenshot_x >= main_display.pixel_width as f64
    || screenshot_y >= main_display.pixel_height as f64
  {
    return Err(format!(
      "screenshot pixel point ({screenshot_x:.3}, {screenshot_y:.3}) is outside main display physical bounds {}x{}",
      main_display.pixel_width, main_display.pixel_height
    ));
  }
  Ok((
    main_display.bounds.x as f64 + (screenshot_x / main_display.scale_factor),
    main_display.bounds.y as f64 + (screenshot_y / main_display.scale_factor),
  ))
}

pub(crate) fn resolve_window_point(
  call: &DriverCall,
  window: &ObservedWindow,
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
  let display = snapshot.displays.iter().find(|display| {
    let left = display.bounds.x as f64;
    let top = display.bounds.y as f64;
    let right = left + display.bounds.width as f64;
    let bottom = top + display.bounds.height as f64;
    x >= left && x < right && y >= top && y < bottom
  })?;
  let local_x = x - display.bounds.x as f64;
  let local_y = y - display.bounds.y as f64;

  Some(ObservedPointResolution {
    display: display.clone(),
    local_x,
    local_y,
    backing_pixel_x: (local_x * display.scale_factor).round() as i64,
    backing_pixel_y: (local_y * display.scale_factor).round() as i64,
  })
}

pub(crate) fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}
