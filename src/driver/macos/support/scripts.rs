use std::path::Path;

use super::super::*;

pub(crate) fn build_observe_windows_script(limit: i64, app_filter: &str) -> String {
  OBSERVE_WINDOWS_SCRIPT_TEMPLATE
    .replace("__LIMIT__", &limit.to_string())
    .replace("__APP_FILTER__", &swift_string_literal(app_filter))
}

pub(crate) fn build_observe_window_tree_script(
  app: &str,
  max_depth: i64,
  max_children: i64,
) -> String {
  OBSERVE_WINDOW_TREE_SCRIPT_TEMPLATE
    .replace("__APP_QUERY__", &swift_string_literal(app))
    .replace("__MAX_DEPTH__", &max_depth.to_string())
    .replace("__MAX_CHILDREN__", &max_children.to_string())
}

pub(crate) fn build_ocr_find_text_script(
  image_path: &Path,
  query: &str,
  exact: bool,
  case_sensitive: bool,
  max_observations: i64,
  crop_region: Option<&ObservedRect>,
) -> String {
  OCR_FIND_TEXT_SCRIPT_TEMPLATE
    .replace(
      "__IMAGE_PATH__",
      &swift_string_literal(&image_path.display().to_string()),
    )
    .replace("__QUERY__", &swift_string_literal(query))
    .replace("__EXACT__", if exact { "true" } else { "false" })
    .replace(
      "__CASE_SENSITIVE__",
      if case_sensitive { "true" } else { "false" },
    )
    .replace("__MAX_OBSERVATIONS__", &max_observations.to_string())
    .replace(
      "__CROP_ENABLED__",
      if crop_region.is_some() {
        "true"
      } else {
        "false"
      },
    )
    .replace(
      "__CROP_X__",
      &crop_region.map(|value| value.x).unwrap_or(0).to_string(),
    )
    .replace(
      "__CROP_Y__",
      &crop_region.map(|value| value.y).unwrap_or(0).to_string(),
    )
    .replace(
      "__CROP_WIDTH__",
      &crop_region
        .map(|value| value.width)
        .unwrap_or(0)
        .to_string(),
    )
    .replace(
      "__CROP_HEIGHT__",
      &crop_region
        .map(|value| value.height)
        .unwrap_or(0)
        .to_string(),
    )
}

pub(crate) fn build_find_visual_rows_script(
  image_path: &Path,
  crop_region: Option<&ObservedRect>,
) -> String {
  FIND_VISUAL_ROWS_SCRIPT_TEMPLATE
    .replace(
      "__IMAGE_PATH__",
      &swift_string_literal(&image_path.display().to_string()),
    )
    .replace(
      "__CROP_ENABLED__",
      if crop_region.is_some() {
        "true"
      } else {
        "false"
      },
    )
    .replace(
      "__CROP_X__",
      &crop_region.map(|value| value.x).unwrap_or(0).to_string(),
    )
    .replace(
      "__CROP_Y__",
      &crop_region.map(|value| value.y).unwrap_or(0).to_string(),
    )
    .replace(
      "__CROP_WIDTH__",
      &crop_region
        .map(|value| value.width)
        .unwrap_or(0)
        .to_string(),
    )
    .replace(
      "__CROP_HEIGHT__",
      &crop_region
        .map(|value| value.height)
        .unwrap_or(0)
        .to_string(),
    )
}

pub(crate) fn build_click_point_script(
  x: f64,
  y: f64,
  button_code: i32,
  click_count: i64,
) -> String {
  CLICK_POINT_SCRIPT_TEMPLATE
    .replace("__X__", &format!("{x:.3}"))
    .replace("__Y__", &format!("{y:.3}"))
    .replace("__BUTTON__", &button_code.to_string())
    .replace("__CLICK_COUNT__", &click_count.to_string())
}

pub(crate) fn build_scroll_point_script(x: f64, y: f64, delta_x: f64, delta_y: f64) -> String {
  SCROLL_POINT_SCRIPT_TEMPLATE
    .replace("__X__", &format!("{x:.3}"))
    .replace("__Y__", &format!("{y:.3}"))
    .replace("__DELTA_X__", &format!("{:.0}", delta_x.round()))
    .replace("__DELTA_Y__", &format!("{:.0}", delta_y.round()))
}

pub(crate) fn build_restore_clipboard_script(snapshot_payload: &str) -> String {
  RESTORE_CLIPBOARD_SCRIPT_TEMPLATE.replace("__PAYLOAD__", &swift_string_literal(snapshot_payload))
}

pub(crate) fn build_set_clipboard_text_script(text: &str) -> String {
  SET_CLIPBOARD_TEXT_SCRIPT_TEMPLATE.replace("__TEXT__", &swift_string_literal(text))
}

pub(crate) fn probe_automation_to_system_events() -> String {
  let args = vec![
    "-e".to_string(),
    "tell application \"System Events\"".to_string(),
    "-e".to_string(),
    "return name of first application process whose frontmost is true".to_string(),
    "-e".to_string(),
    "end tell".to_string(),
  ];

  match run_command(OSASCRIPT_BINARY, &args) {
    Ok(_) => "granted".to_string(),
    Err(_) => "missing".to_string(),
  }
}
