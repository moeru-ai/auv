use crate::types::{
  AuvResult, ObservedDisplaySnapshot, ObservedOcrRow, ObservedPointResolution, ObservedRect, ObservedWindow, OcrTextMatch,
};

pub fn looks_like_bundle_identifier(raw: &str) -> bool {
  raw.contains('.') && raw.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

pub fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name || app_identifier.contains(&app_name) || app_name.contains(&app_identifier)
}

pub fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

pub fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

pub fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  (matched.bounds.x as f64 + (matched.bounds.width as f64 / 2.0), matched.bounds.y as f64 + (matched.bounds.height as f64 / 2.0))
}

pub fn group_ocr_matches_into_rows(matches: &[&OcrTextMatch]) -> Vec<ObservedOcrRow> {
  let mut sorted = matches.to_vec();
  sorted.sort_by(|left, right| {
    let (_, left_center_y) = ocr_match_center(left);
    let (_, right_center_y) = ocr_match_center(right);
    left_center_y.partial_cmp(&right_center_y).unwrap_or(std::cmp::Ordering::Equal).then_with(|| left.bounds.x.cmp(&right.bounds.x))
  });

  let mut rows = Vec::<ObservedOcrRow>::new();
  for matched in sorted {
    let (_, center_y) = ocr_match_center(matched);
    if let Some(existing) = rows.last_mut() {
      let existing_center_y = existing.bounds.y as f64 + (existing.bounds.height as f64 / 2.0);
      let vertical_threshold = ((existing.bounds.height.max(matched.bounds.height) as f64) * 1.5).max(36.0);
      if (center_y - existing_center_y).abs() <= vertical_threshold {
        existing.bounds = union_rects(&existing.bounds, &matched.bounds);
        if !existing.text_fragments.iter().any(|value| value == &matched.text) {
          existing.text_fragments.push(matched.text.clone());
        }
        continue;
      }
    }

    rows.push(ObservedOcrRow {
      row_index: rows.len(),
      source: "ocr-text".to_string(),
      bounds: matched.bounds.clone(),
      text_fragments: vec![matched.text.clone()],
    });
  }

  for (index, row) in rows.iter_mut().enumerate() {
    row.row_index = index;
  }
  rows
}

fn union_rects(left: &ObservedRect, right: &ObservedRect) -> ObservedRect {
  let min_x = left.x.min(right.x);
  let min_y = left.y.min(right.y);
  let max_x = (left.x + left.width).max(right.x + right.width);
  let max_y = (left.y + left.height).max(right.y + right.height);
  ObservedRect {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  }
}

pub fn project_main_screenshot_point(snapshot: &ObservedDisplaySnapshot, screenshot_x: f64, screenshot_y: f64) -> AuvResult<(f64, f64)> {
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

pub fn resolve_display_point(snapshot: &ObservedDisplaySnapshot, x: f64, y: f64) -> Option<ObservedPointResolution> {
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
