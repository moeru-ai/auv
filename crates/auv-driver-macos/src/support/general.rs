use crate::types::{
  AuvResult, ObservedDisplaySnapshot, ObservedPointResolution, ObservedRect, ObservedWindow,
  OcrTextMatch,
};

pub fn looks_like_bundle_identifier(raw: &str) -> bool {
  raw.contains('.')
    && raw
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

pub fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name
    || app_identifier.contains(&app_name)
    || app_name.contains(&app_identifier)
}

pub fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

pub fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

pub fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  (
    matched.bounds.x as f64 + (matched.bounds.width as f64 / 2.0),
    matched.bounds.y as f64 + (matched.bounds.height as f64 / 2.0),
  )
}

pub fn project_main_screenshot_point(
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

pub fn resolve_display_point(
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
