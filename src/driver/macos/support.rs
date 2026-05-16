use std::io::Write;

use super::*;

pub(super) fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  let report = run_swift_script(ENUMERATE_DISPLAYS_SCRIPT)?;
  parse_display_snapshot(&report)
}

pub(super) fn capture_screenshot_file(label: &str) -> AuvResult<PathBuf> {
  let temporary_path = screenshot_temp_path(label);
  let args = vec!["-x".to_string(), temporary_path.display().to_string()];
  run_command(SCREEN_CAPTURE_BINARY, &args)?;

  if !temporary_path.exists() {
    return Err(format!(
      "screencapture reported success but no image was created at {}",
      temporary_path.display()
    ));
  }

  Ok(temporary_path)
}

pub(super) fn maybe_activate_target_app_for_observation(
  call: &DriverCall,
) -> AuvResult<Option<String>> {
  let Some(app) = app_identifier(call) else {
    return Ok(None);
  };
  if app.is_empty() || !optional_bool(call, "activate_target_before_capture")?.unwrap_or(false) {
    return Ok(None);
  }

  activate_target_app(&app)?;
  Ok(Some(app))
}

pub(super) fn build_observe_windows_script(limit: i64, app_filter: &str) -> String {
  OBSERVE_WINDOWS_SCRIPT_TEMPLATE
    .replace("__LIMIT__", &limit.to_string())
    .replace("__APP_FILTER__", &swift_string_literal(app_filter))
}

pub(super) fn build_observe_window_tree_script(
  app: &str,
  max_depth: i64,
  max_children: i64,
) -> String {
  OBSERVE_WINDOW_TREE_SCRIPT_TEMPLATE
    .replace("__APP_QUERY__", &swift_string_literal(app))
    .replace("__MAX_DEPTH__", &max_depth.to_string())
    .replace("__MAX_CHILDREN__", &max_children.to_string())
}

pub(super) fn build_ocr_find_text_script(
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

pub(super) fn build_find_visual_rows_script(
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

pub(super) fn build_click_point_script(
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

pub(super) fn build_scroll_point_script(x: f64, y: f64, delta_x: f64, delta_y: f64) -> String {
  SCROLL_POINT_SCRIPT_TEMPLATE
    .replace("__X__", &format!("{x:.3}"))
    .replace("__Y__", &format!("{y:.3}"))
    .replace("__DELTA_X__", &format!("{:.0}", delta_x.round()))
    .replace("__DELTA_Y__", &format!("{:.0}", delta_y.round()))
}

pub(super) fn build_restore_clipboard_script(snapshot_payload: &str) -> String {
  RESTORE_CLIPBOARD_SCRIPT_TEMPLATE.replace("__PAYLOAD__", &swift_string_literal(snapshot_payload))
}

pub(super) fn build_set_clipboard_text_script(text: &str) -> String {
  SET_CLIPBOARD_TEXT_SCRIPT_TEMPLATE.replace("__TEXT__", &swift_string_literal(text))
}

pub(super) fn probe_automation_to_system_events() -> String {
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

pub(super) fn parse_display_snapshot(report: &str) -> AuvResult<ObservedDisplaySnapshot> {
  let captured_at = report_value(report, "capturedAt=")
    .unwrap_or("")
    .to_string();
  let displays = report
    .lines()
    .filter(|line| line.starts_with("display\t"))
    .map(parse_display_line)
    .collect::<AuvResult<Vec<_>>>()?;

  if displays.is_empty() {
    return Err("display probe returned no connected displays".to_string());
  }

  if let Some(raw_count) = report_value(report, "displayCount=") {
    let parsed_count = raw_count
      .parse::<usize>()
      .map_err(|error| format!("invalid displayCount value {}: {}", raw_count, error))?;
    if parsed_count != displays.len() {
      return Err(format!(
        "display probe reported {} displays but parsed {}",
        parsed_count,
        displays.len()
      ));
    }
  }

  Ok(ObservedDisplaySnapshot {
    combined_bounds: compute_combined_bounds(&displays),
    displays,
    captured_at,
  })
}

pub(super) fn parse_display_line(line: &str) -> AuvResult<ObservedDisplay> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 15 {
    return Err(format!(
      "invalid display report line; expected 15 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedDisplay {
    display_id: parse_u32(columns[1], "displayId")?,
    is_main: parse_bool_flag(columns[2], "isMain")?,
    is_built_in: parse_bool_flag(columns[3], "isBuiltIn")?,
    bounds: ObservedRect {
      x: parse_i64(columns[4], "bounds.x")?,
      y: parse_i64(columns[5], "bounds.y")?,
      width: parse_i64(columns[6], "bounds.width")?,
      height: parse_i64(columns[7], "bounds.height")?,
    },
    visible_bounds: ObservedRect {
      x: parse_i64(columns[8], "visibleBounds.x")?,
      y: parse_i64(columns[9], "visibleBounds.y")?,
      width: parse_i64(columns[10], "visibleBounds.width")?,
      height: parse_i64(columns[11], "visibleBounds.height")?,
    },
    scale_factor: parse_f64(columns[12], "scaleFactor")?,
    pixel_width: parse_i64(columns[13], "pixelWidth")?,
    pixel_height: parse_i64(columns[14], "pixelHeight")?,
  })
}

pub(super) fn parse_window_line(line: &str) -> AuvResult<ObservedWindow> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 9 {
    return Err(format!(
      "invalid window report line; expected 9 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedWindow {
    app_name: columns[1].to_string(),
    owner_pid: parse_i64(columns[2], "window.ownerPid")?,
    layer: parse_i64(columns[3], "window.layer")?,
    title: columns[4].to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[5], "window.bounds.x")?,
      y: parse_i64(columns[6], "window.bounds.y")?,
      width: parse_i64(columns[7], "window.bounds.width")?,
      height: parse_i64(columns[8], "window.bounds.height")?,
    },
  })
}

pub(super) fn parse_ocr_text_snapshot(report: &str) -> AuvResult<OcrTextSnapshot> {
  let recognized_at = report_value(report, "recognizedAt=")
    .unwrap_or("")
    .to_string();
  let image_path = PathBuf::from(report_value(report, "imagePath=").unwrap_or(""));
  let image_width = parse_i64(
    report_value(report, "imageWidth=").unwrap_or("0"),
    "ocr.imageWidth",
  )?;
  let image_height = parse_i64(
    report_value(report, "imageHeight=").unwrap_or("0"),
    "ocr.imageHeight",
  )?;
  let query = report_value(report, "query=").unwrap_or("").to_string();
  let exact = parse_bool_flag(
    report_value(report, "exact=").unwrap_or("false"),
    "ocr.exact",
  )?;
  let case_sensitive = parse_bool_flag(
    report_value(report, "caseSensitive=").unwrap_or("false"),
    "ocr.caseSensitive",
  )?;
  let matches = report
    .lines()
    .filter(|line| line.starts_with("match\t"))
    .map(parse_ocr_text_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(OcrTextSnapshot {
    recognized_at,
    image_path,
    image_width,
    image_height,
    query,
    exact,
    case_sensitive,
    matches,
  })
}

pub(super) fn parse_ocr_text_line(line: &str) -> AuvResult<OcrTextMatch> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 8 {
    return Err(format!(
      "invalid OCR report line; expected 8 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(OcrTextMatch {
    match_index: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid ocr.matchIndex value {}: {}", columns[1], error))?,
    text: columns[2].to_string(),
    confidence: parse_f64(columns[3], "ocr.confidence")?,
    bounds: ObservedRect {
      x: parse_i64(columns[4], "ocr.bounds.x")?,
      y: parse_i64(columns[5], "ocr.bounds.y")?,
      width: parse_i64(columns[6], "ocr.bounds.width")?,
      height: parse_i64(columns[7], "ocr.bounds.height")?,
    },
  })
}

pub(super) fn parse_visual_rows_snapshot(report: &str) -> AuvResult<DetectedScreenRows> {
  let rows = report
    .lines()
    .filter(|line| line.starts_with("row\t"))
    .map(parse_visual_row_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(DetectedScreenRows {
    strategy: report_value(report, "rowStrategy=")
      .unwrap_or("visual-bands")
      .to_string(),
    raw_match_count: 0,
    filtered_match_count: 0,
    rows,
    report: report.to_string(),
  })
}

fn parse_visual_row_line(line: &str) -> AuvResult<ObservedOcrRow> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 7 {
    return Err(format!(
      "invalid visual-row report line; expected 7 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedOcrRow {
    row_index: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid visualRow.index value {}: {}", columns[1], error))?,
    source: "visual-bands".to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[2], "visualRow.bounds.x")?,
      y: parse_i64(columns[3], "visualRow.bounds.y")?,
      width: parse_i64(columns[4], "visualRow.bounds.width")?,
      height: parse_i64(columns[5], "visualRow.bounds.height")?,
    },
    text_fragments: vec![],
  })
}

pub(super) fn parse_ocr_region_constraint(
  call: &DriverCall,
  image_width: i64,
  image_height: i64,
) -> AuvResult<Option<ObservedRect>> {
  let left_ratio = optional_f64(call, "region_left_ratio")?;
  let top_ratio = optional_f64(call, "region_top_ratio")?;
  let right_ratio = optional_f64(call, "region_right_ratio")?;
  let bottom_ratio = optional_f64(call, "region_bottom_ratio")?;

  match (left_ratio, top_ratio, right_ratio, bottom_ratio) {
    (None, None, None, None) => Ok(None),
    (Some(left), Some(top), Some(right), Some(bottom)) => {
      for (label, value) in [
        ("region_left_ratio", left),
        ("region_top_ratio", top),
        ("region_right_ratio", right),
        ("region_bottom_ratio", bottom),
      ] {
        if !(0.0..=1.0).contains(&value) {
          return Err(format!(
            "invalid --{} value {:.3}: expected a ratio within 0.0..=1.0",
            label, value
          ));
        }
      }
      if left >= right {
        return Err(format!(
          "invalid OCR region: left ratio {:.3} must be smaller than right ratio {:.3}",
          left, right
        ));
      }
      if top >= bottom {
        return Err(format!(
          "invalid OCR region: top ratio {:.3} must be smaller than bottom ratio {:.3}",
          top, bottom
        ));
      }

      Ok(Some(ObservedRect {
        x: (left * image_width as f64).round() as i64,
        y: (top * image_height as f64).round() as i64,
        width: ((right - left) * image_width as f64).round() as i64,
        height: ((bottom - top) * image_height as f64).round() as i64,
      }))
    }
    _ => Err(
      "OCR region ratio mode requires --region_left_ratio, --region_top_ratio, --region_right_ratio, and --region_bottom_ratio together"
        .to_string(),
    ),
  }
}

pub(super) fn filter_ocr_matches<'a>(
  matches: &'a [OcrTextMatch],
  min_confidence: f64,
  region: Option<&ObservedRect>,
) -> Vec<&'a OcrTextMatch> {
  matches
    .iter()
    .filter(|matched| matched.confidence >= min_confidence)
    .filter(|matched| {
      region.is_none_or(|region| {
        let (center_x, center_y) = ocr_match_center(matched);
        center_x >= region.x as f64
          && center_y >= region.y as f64
          && center_x < (region.x + region.width) as f64
          && center_y < (region.y + region.height) as f64
      })
    })
    .collect()
}

pub(super) fn detect_screen_rows(
  image_path: &Path,
  min_confidence: f64,
  max_observations: i64,
  region: Option<&ObservedRect>,
) -> AuvResult<DetectedScreenRows> {
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    image_path,
    "",
    false,
    false,
    max_observations,
    region,
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region);
  let rows = group_ocr_matches_into_rows(&filtered_matches);
  if !rows.is_empty() {
    return Ok(DetectedScreenRows {
      strategy: "ocr-text".to_string(),
      raw_match_count: ocr_snapshot.matches.len(),
      filtered_match_count: filtered_matches.len(),
      rows,
      report: ocr_report,
    });
  }

  let visual_report = run_swift_script(&build_find_visual_rows_script(image_path, region))?;
  parse_visual_rows_snapshot(&visual_report)
}

pub(super) fn group_ocr_matches_into_rows(matches: &[&OcrTextMatch]) -> Vec<ObservedOcrRow> {
  let mut sorted = matches.to_vec();
  sorted.sort_by(|left, right| {
    let (_, left_center_y) = ocr_match_center(left);
    let (_, right_center_y) = ocr_match_center(right);
    left_center_y
      .partial_cmp(&right_center_y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| left.bounds.x.cmp(&right.bounds.x))
  });

  let mut rows = Vec::<ObservedOcrRow>::new();
  for matched in sorted {
    let (_, center_y) = ocr_match_center(matched);
    if let Some(existing) = rows.last_mut() {
      let existing_center_y = existing.bounds.y as f64 + (existing.bounds.height as f64 / 2.0);
      let vertical_threshold =
        ((existing.bounds.height.max(matched.bounds.height)) as f64 * 1.5).max(36.0);
      if (center_y - existing_center_y).abs() <= vertical_threshold {
        existing.bounds = union_rects(&existing.bounds, &matched.bounds);
        if !existing
          .text_fragments
          .iter()
          .any(|value| value == &matched.text)
        {
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

pub(super) fn render_ocr_row_note(row: &ObservedOcrRow) -> String {
  if row.text_fragments.is_empty() {
    return format!(
      "row[{}] source={} bounds={}",
      row.row_index,
      row.source,
      render_rect_compact(&row.bounds)
    );
  }

  let preview = row
    .text_fragments
    .iter()
    .take(3)
    .cloned()
    .collect::<Vec<_>>()
    .join(" | ");
  format!(
    "row[{}] source={} bounds={} text={}",
    row.row_index,
    row.source,
    render_rect_compact(&row.bounds),
    preview
  )
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

pub(super) fn render_ocr_region_note(region: &ObservedRect) -> String {
  format!("ocrRegion={}", render_rect_compact(region))
}

pub(super) fn parse_bool_flag(raw: &str, label: &str) -> AuvResult<bool> {
  match raw {
    "1" | "true" => Ok(true),
    "0" | "false" => Ok(false),
    other => Err(format!("invalid {} value {}: expected 0/1", label, other)),
  }
}

pub(super) fn parse_i64(raw: &str, label: &str) -> AuvResult<i64> {
  raw
    .parse::<i64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub(super) fn parse_u32(raw: &str, label: &str) -> AuvResult<u32> {
  raw
    .parse::<u32>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub(super) fn parse_f64(raw: &str, label: &str) -> AuvResult<f64> {
  let value = raw
    .parse::<f64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))?;
  if !value.is_finite() {
    return Err(format!(
      "invalid {} value {}: expected a finite number",
      label, raw
    ));
  }
  Ok(value)
}

pub(super) fn compute_combined_bounds(displays: &[ObservedDisplay]) -> ObservedRect {
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

pub(super) fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name
    || app_identifier.contains(&app_name)
    || app_name.contains(&app_identifier)
}

pub(super) fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

pub(super) fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  (
    matched.bounds.x as f64 + (matched.bounds.width as f64 / 2.0),
    matched.bounds.y as f64 + (matched.bounds.height as f64 / 2.0),
  )
}

pub(super) fn project_main_screenshot_point(
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

pub(super) fn resolve_window_point(
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

pub(super) fn resolve_display_point(
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

pub(super) fn render_display_snapshot_report(snapshot: &ObservedDisplaySnapshot) -> String {
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("displayCount={}", snapshot.displays.len()),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];
  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }
  lines.join("\n") + "\n"
}

pub(super) fn render_point_identification_report(
  snapshot: &ObservedDisplaySnapshot,
  x: f64,
  y: f64,
  resolution: Option<&ObservedPointResolution>,
) -> String {
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("queryPoint={x:.3},{y:.3}"),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];

  if let Some(resolution) = resolution {
    lines.push(format!("result=display#{}", resolution.display.display_id));
    lines.push(format!(
      "localPoint={:.3},{:.3}",
      resolution.local_x, resolution.local_y
    ));
    lines.push(format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ));
  } else {
    lines.push("result=outside".to_string());
  }

  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }

  lines.join("\n") + "\n"
}

pub(super) fn parse_observed_ax_tree(report: &str) -> AuvResult<ObservedAxTreeSnapshot> {
  let observed_at = report_value(report, "observedAt=")
    .unwrap_or("")
    .to_string();
  let app_name = report_value(report, "appName=").unwrap_or("").to_string();
  let bundle_id = report_value(report, "bundleId=").unwrap_or("").to_string();
  let window_title = report_value(report, "windowTitle=")
    .unwrap_or("")
    .to_string();
  let nodes = report
    .lines()
    .filter(|line| line.starts_with("node\t"))
    .map(parse_observed_ax_node_line)
    .collect::<AuvResult<Vec<_>>>()?;

  if nodes.is_empty() {
    return Err("AX tree report contained no nodes".to_string());
  }

  Ok(ObservedAxTreeSnapshot {
    observed_at,
    app_name,
    bundle_id,
    window_title,
    nodes,
  })
}

pub(super) fn parse_observed_ax_node_line(line: &str) -> AuvResult<ObservedAxNode> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 15 {
    return Err(format!(
      "invalid AX node report line; expected 15 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedAxNode {
    depth: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid AX node depth {}: {}", columns[1], error))?,
    path: columns[2].to_string(),
    role: columns[3].to_string(),
    subrole: columns[4].to_string(),
    title: columns[5].to_string(),
    description: columns[6].to_string(),
    help: columns[7].to_string(),
    identifier: columns[8].to_string(),
    placeholder: columns[9].to_string(),
    value: columns[10].to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[11], "ax.bounds.x")?,
      y: parse_i64(columns[12], "ax.bounds.y")?,
      width: parse_i64(columns[13], "ax.bounds.width")?,
      height: parse_i64(columns[14], "ax.bounds.height")?,
    },
  })
}

pub(super) fn find_best_ax_node<'a>(
  snapshot: &'a ObservedAxTreeSnapshot,
  query: &str,
) -> Option<&'a ObservedAxNode> {
  let query = query.trim().to_lowercase();
  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter_map(|node| score_ax_node_match(node, &query).map(|score| (score, node)))
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
}

pub(super) fn find_now_playing_ax_node<'a>(
  snapshot: &'a ObservedAxTreeSnapshot,
  expected_title: &str,
  expected_artist: Option<&str>,
  scope_path_prefix: Option<&str>,
) -> Option<&'a ObservedAxNode> {
  let expected_title = expected_title.trim().to_lowercase();
  if expected_title.is_empty() {
    return None;
  }
  let expected_artist = expected_artist
    .map(|value| value.trim().to_lowercase())
    .filter(|value| !value.is_empty());
  let scope_path_prefix = scope_path_prefix
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());

  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| {
      scope_path_prefix
        .as_ref()
        .is_none_or(|prefix| node.path.starts_with(prefix))
    })
    .filter_map(|node| {
      score_now_playing_ax_node_match(node, &expected_title, expected_artist.as_deref())
        .map(|score| (score, node))
    })
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
}

pub(super) fn ax_node_search_text(node: &ObservedAxNode) -> String {
  let searchable = [
    node.title.as_str(),
    node.description.as_str(),
    node.help.as_str(),
    node.identifier.as_str(),
    node.placeholder.as_str(),
    node.value.as_str(),
  ]
  .into_iter()
  .filter_map(|value| {
    let trimmed = value.trim();
    if trimmed.is_empty() {
      None
    } else {
      Some(trimmed)
    }
  })
  .collect::<Vec<_>>()
  .join(" ");
  normalize_ax_text(&searchable)
}

fn normalize_ax_text(value: &str) -> String {
  value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>()
    .to_lowercase()
}

fn score_now_playing_ax_node_match(
  node: &ObservedAxNode,
  expected_title: &str,
  expected_artist: Option<&str>,
) -> Option<i64> {
  let searchable = ax_node_search_text(node);
  if !searchable.contains(expected_title) {
    return None;
  }
  if let Some(expected_artist) = expected_artist {
    if !searchable.contains(expected_artist) {
      return None;
    }
  }

  let mut score = 100 - node.depth as i64;
  if node.title.to_lowercase().contains(expected_title) {
    score += 40;
  }
  if let Some(expected_artist) = expected_artist {
    if node.title.to_lowercase().contains(expected_artist) {
      score += 20;
    }
  }
  if node.role == "AXUnknown" || node.role == "AXStaticText" {
    score += 10;
  }
  if node.subrole == "AXStaticText" || node.subrole == "AXTextField" {
    score += 6;
  }

  Some(score)
}

pub(super) fn no_matching_ax_node_error(
  snapshot: &ObservedAxTreeSnapshot,
  query: &str,
  expected_kind: &str,
) -> String {
  if snapshot.nodes.len() <= 1 {
    return format!(
      "no matching {expected_kind} node found for query {query}; observed only {} AX node(s), so the target UI may need to be revealed before retrying",
      snapshot.nodes.len()
    );
  }
  format!("no matching {expected_kind} node found for query {query}")
}

pub(super) fn score_ax_node_match(node: &ObservedAxNode, query: &str) -> Option<i64> {
  if query.is_empty() {
    return None;
  }

  let fields = [
    ("title", node.title.as_str()),
    ("description", node.description.as_str()),
    ("help", node.help.as_str()),
    ("identifier", node.identifier.as_str()),
    ("placeholder", node.placeholder.as_str()),
    ("value", node.value.as_str()),
  ];

  let mut score = 0i64;
  for (label, raw_value) in fields {
    let value = raw_value.trim().to_lowercase();
    if value.is_empty() || !value.contains(query) {
      continue;
    }

    score += match label {
      "title" => 80,
      "description" => 72,
      "placeholder" => 64,
      "help" => 56,
      "identifier" => 40,
      _ => 24,
    };
    if value == query {
      score += 20;
    }
  }

  if score == 0 {
    return None;
  }

  if node.role == "AXTextField" || node.subrole == "AXSearchField" {
    score += 24;
  }
  if node.role == "AXButton" || node.role == "AXLink" {
    score += 18;
  }
  if node.role == "AXUnknown" {
    score += 8;
  }

  Some(score - node.depth as i64)
}

pub(super) fn ax_node_center(node: &ObservedAxNode) -> (f64, f64) {
  (
    node.bounds.x as f64 + (node.bounds.width as f64 / 2.0),
    node.bounds.y as f64 + (node.bounds.height as f64 / 2.0),
  )
}

pub(super) fn render_ax_interaction_report(
  kind: &str,
  snapshot: &ObservedAxTreeSnapshot,
  node: &ObservedAxNode,
  query: &str,
) -> String {
  [
    format!("kind={kind}"),
    format!("observedAt={}", snapshot.observed_at),
    format!("appName={}", snapshot.app_name),
    format!("bundleId={}", snapshot.bundle_id),
    format!("windowTitle={}", snapshot.window_title),
    format!("query={query}"),
    format!("matchedPath={}", node.path),
    format!("matchedRole={}", node.role),
    format!("matchedSubrole={}", node.subrole),
    format!("matchedTitle={}", node.title),
    format!("matchedDescription={}", node.description),
    format!("matchedHelp={}", node.help),
    format!("matchedIdentifier={}", node.identifier),
    format!("matchedPlaceholder={}", node.placeholder),
    format!("matchedValue={}", node.value),
    format!("matchedBounds={}", render_rect_compact(&node.bounds)),
  ]
  .join("\n")
    + "\n"
}

pub(super) fn render_display_report_line(display: &ObservedDisplay) -> String {
  format!(
    "display\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}\t{}",
    display.display_id,
    if display.is_main { 1 } else { 0 },
    if display.is_built_in { 1 } else { 0 },
    display.bounds.x,
    display.bounds.y,
    display.bounds.width,
    display.bounds.height,
    display.visible_bounds.x,
    display.visible_bounds.y,
    display.visible_bounds.width,
    display.visible_bounds.height,
    display.scale_factor,
    display.pixel_width,
    display.pixel_height
  )
}

pub(super) fn render_display_note(display: &ObservedDisplay) -> String {
  format!(
    "display#{} main={} builtIn={} bounds={} scaleFactor={:.3} pixels={}x{}",
    display.display_id,
    display.is_main,
    display.is_built_in,
    render_rect_compact(&display.bounds),
    display.scale_factor,
    display.pixel_width,
    display.pixel_height
  )
}

pub(super) fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

pub(super) fn assess_coordinate_readiness(
  snapshot: &ObservedDisplaySnapshot,
  screenshot: &ScreenshotDimensions,
) -> AuvResult<CoordinateReadinessAssessment> {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  let matches_main_logical = main_display.bounds.width == screenshot.width
    && main_display.bounds.height == screenshot.height;
  let matches_main_physical =
    main_display.pixel_width == screenshot.width && main_display.pixel_height == screenshot.height;
  let matches_combined_logical = snapshot.combined_bounds.width == screenshot.width
    && snapshot.combined_bounds.height == screenshot.height;
  let likely_retina_backing_mismatch =
    matches_main_physical && !matches_main_logical && main_display.scale_factor > 1.0;
  let ready_for_logical_input = matches_main_logical || matches_combined_logical;
  let reason = if ready_for_logical_input {
    if matches_main_logical && matches_combined_logical {
      "screenshot dimensions match both the main display and the combined logical bounds"
        .to_string()
    } else if matches_main_logical {
      "screenshot dimensions match the main display logical bounds".to_string()
    } else {
      "screenshot dimensions match the combined logical desktop bounds".to_string()
    }
  } else if likely_retina_backing_mismatch {
    format!(
      "screenshot dimensions match main display physical pixels while logical input uses {}x{} points; align Retina/backing-scale assumptions before real input",
      main_display.bounds.width, main_display.bounds.height
    )
  } else {
    format!(
      "screenshot {}x{} does not match main logical {}x{}, main physical {}x{}, or combined logical {}x{}",
      screenshot.width,
      screenshot.height,
      main_display.bounds.width,
      main_display.bounds.height,
      main_display.pixel_width,
      main_display.pixel_height,
      snapshot.combined_bounds.width,
      snapshot.combined_bounds.height
    )
  };

  Ok(CoordinateReadinessAssessment {
    ready_for_logical_input,
    matches_main_logical,
    matches_main_physical,
    matches_combined_logical,
    likely_retina_backing_mismatch,
    reason,
  })
}

pub(super) fn render_coordinate_readiness_report(
  snapshot: &ObservedDisplaySnapshot,
  screenshot: &ScreenshotDimensions,
  assessment: &CoordinateReadinessAssessment,
) -> String {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first());
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("displayCount={}", snapshot.displays.len()),
    format!(
      "screenshotPixels={}x{}",
      screenshot.width, screenshot.height
    ),
    format!(
      "combinedLogicalBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
    format!(
      "readyForLogicalInput={}",
      assessment.ready_for_logical_input
    ),
    format!("matchesMainLogical={}", assessment.matches_main_logical),
    format!("matchesMainPhysical={}", assessment.matches_main_physical),
    format!(
      "matchesCombinedLogical={}",
      assessment.matches_combined_logical
    ),
    format!(
      "likelyRetinaBackingMismatch={}",
      assessment.likely_retina_backing_mismatch
    ),
    format!("reason={}", assessment.reason),
  ];
  if let Some(main_display) = main_display {
    lines.push(format!("mainDisplayId={}", main_display.display_id));
    lines.push(format!(
      "mainDisplayLogicalSize={}x{}",
      main_display.bounds.width, main_display.bounds.height
    ));
    lines.push(format!(
      "mainDisplayPixelSize={}x{}",
      main_display.pixel_width, main_display.pixel_height
    ));
    lines.push(format!(
      "mainDisplayScaleFactor={:.3}",
      main_display.scale_factor
    ));
  }
  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }
  lines.join("\n") + "\n"
}

pub(super) fn read_png_dimensions(path: &Path) -> AuvResult<ScreenshotDimensions> {
  let mut file = fs::File::open(path)
    .map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?;
  let mut header = [0u8; 24];
  file
    .read_exact(&mut header)
    .map_err(|error| format!("failed to read PNG header {}: {error}", path.display()))?;

  const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
  if header[..8] != PNG_SIGNATURE {
    return Err(format!(
      "screenshot {} is not a PNG produced by screencapture",
      path.display()
    ));
  }
  if &header[12..16] != b"IHDR" {
    return Err(format!(
      "screenshot {} is missing a PNG IHDR chunk",
      path.display()
    ));
  }

  let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]) as i64;
  let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]) as i64;
  Ok(ScreenshotDimensions { width, height })
}

pub(super) fn run_swift_script(source: &str) -> AuvResult<String> {
  let script_path = temp_file_path("swift-script", "swift");
  fs::write(&script_path, source).map_err(|error| {
    format!(
      "failed to write Swift script {}: {error}",
      script_path.display()
    )
  })?;

  let result = run_swift_script_with_fallback(&script_path);
  let _ = fs::remove_file(&script_path);
  result
}

pub(super) fn run_swift_script_with_fallback(script_path: &PathBuf) -> AuvResult<String> {
  let xcrun_args = vec!["swift".to_string(), script_path.display().to_string()];

  match run_command(XCRUN_BINARY, &xcrun_args) {
    Ok(output) => Ok(output.stdout),
    Err(error) if error.contains("failed to spawn xcrun") => {
      let swift_args = vec![script_path.display().to_string()];
      Ok(run_command("swift", &swift_args)?.stdout)
    }
    Err(error) => Err(error),
  }
}

pub(super) fn run_command(binary: &str, args: &[String]) -> AuvResult<CommandOutput> {
  let output = Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| match error.kind() {
      ErrorKind::NotFound => format!("failed to spawn {}: command not found", binary),
      _ => format!("failed to spawn {}: {}", binary, error),
    })?;

  let stdout = String::from_utf8_lossy(&output.stdout).to_string();
  let stderr = String::from_utf8_lossy(&output.stderr).to_string();

  if !output.status.success() {
    let trimmed_stderr = stderr.trim();
    return Err(format!(
      "{} exited with status {}: {}",
      binary,
      output.status,
      if trimmed_stderr.is_empty() {
        "no stderr output"
      } else {
        trimmed_stderr
      }
    ));
  }

  Ok(CommandOutput { stdout })
}

pub(super) fn temp_file_path(label: &str, extension: &str) -> PathBuf {
  env::temp_dir().join(format!(
    "auv-{}-{}-{}.{}",
    sanitize_file_component(label),
    now_millis(),
    std::process::id(),
    extension
  ))
}

pub(super) fn build_text_artifact(
  kind: &str,
  extension: &str,
  label: &str,
  content: String,
  note: &str,
) -> AuvResult<ProducedArtifact> {
  let source_path = temp_file_path(label, extension);
  fs::write(&source_path, content).map_err(|error| {
    format!(
      "failed to write artifact source {}: {error}",
      source_path.display()
    )
  })?;

  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{}.{}", sanitize_file_component(label), extension),
    note: Some(note.to_string()),
  })
}

pub(super) fn screenshot_temp_path(label: &str) -> PathBuf {
  temp_file_path(label, "png")
}

pub(super) fn render_capture_contract_report(
  snapshot: Option<&ObservedDisplaySnapshot>,
  dimensions: &ScreenshotDimensions,
  path: &Path,
) -> String {
  let mut lines = vec![
    format!("screenshotPath={}", path.display()),
    format!(
      "screenshotPixels={}x{}",
      dimensions.width, dimensions.height
    ),
    "coordinateContract=debug.captureScreen emits main-display physical screenshot pixels"
      .to_string(),
  ];
  if let Some(snapshot) = snapshot {
    lines.push(format!("capturedAt={}", snapshot.captured_at));
    lines.push(format!(
      "combinedLogicalBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ));
    if let Some(main_display) = snapshot
      .displays
      .iter()
      .find(|display| display.is_main)
      .or_else(|| snapshot.displays.first())
    {
      lines.push(format!("mainDisplayId={}", main_display.display_id));
      lines.push(format!(
        "mainDisplayLogicalSize={}x{}",
        main_display.bounds.width, main_display.bounds.height
      ));
      lines.push(format!(
        "mainDisplayPixelSize={}x{}",
        main_display.pixel_width, main_display.pixel_height
      ));
      lines.push(format!(
        "mainDisplayScaleFactor={:.3}",
        main_display.scale_factor
      ));
    }
  } else {
    lines.push("displaySnapshot=unavailable".to_string());
  }
  lines.join("\n") + "\n"
}

pub(super) fn require_macos() -> AuvResult<()> {
  if env::consts::OS != "macos" {
    return Err("macos.desktop is only available on macOS".to_string());
  }

  Ok(())
}

pub(super) fn app_identifier(call: &DriverCall) -> Option<String> {
  optional_string(call, "app").or_else(|| {
    call
      .target
      .application_id
      .clone()
      .filter(|value| !value.trim().is_empty())
  })
}

pub(super) fn optional_string(call: &DriverCall, key: &str) -> Option<String> {
  call.inputs.get(key).cloned()
}

pub(super) fn optional_non_empty_string(call: &DriverCall, key: &str) -> Option<String> {
  optional_string(call, key)
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

pub(super) fn required_non_empty_string(call: &DriverCall, key: &str) -> AuvResult<String> {
  let value = optional_non_empty_string(call, key)
    .ok_or_else(|| format!("operation requires --{} <text>", key))?;
  Ok(value)
}

pub(super) fn required_f64(call: &DriverCall, key: &str) -> AuvResult<f64> {
  optional_f64(call, key)?.ok_or_else(|| format!("operation requires --{} <number>", key))
}

pub(super) fn optional_f64(call: &DriverCall, key: &str) -> AuvResult<Option<f64>> {
  match call.inputs.get(key) {
    Some(value) => {
      let parsed = value
        .parse::<f64>()
        .map_err(|error| format!("invalid --{} value {}: {}", key, value, error))?;
      if !parsed.is_finite() {
        return Err(format!(
          "invalid --{} value {}: expected a finite number",
          key, value
        ));
      }
      Ok(Some(parsed))
    }
    None => Ok(None),
  }
}

pub(super) fn optional_i64(call: &DriverCall, key: &str) -> AuvResult<Option<i64>> {
  match call.inputs.get(key) {
    Some(value) => value
      .parse::<i64>()
      .map(Some)
      .map_err(|error| format!("invalid --{} value {}: {}", key, value, error)),
    None => Ok(None),
  }
}

pub(super) fn optional_bool(call: &DriverCall, key: &str) -> AuvResult<Option<bool>> {
  match optional_non_empty_string(call, key) {
    Some(value) => match value.to_ascii_lowercase().as_str() {
      "1" | "true" | "yes" | "on" => Ok(Some(true)),
      "0" | "false" | "no" | "off" => Ok(Some(false)),
      _ => Err(format!(
        "invalid --{} value {}: expected true/false or 1/0",
        key, value
      )),
    },
    None => Ok(None),
  }
}

pub(super) fn optional_positive_u64(call: &DriverCall, key: &str) -> AuvResult<Option<u64>> {
  match optional_i64(call, key)? {
    Some(value) if value < 0 => Err(format!(
      "invalid --{} value {}: expected a non-negative integer",
      key, value
    )),
    Some(value) => Ok(Some(value as u64)),
    None => Ok(None),
  }
}

pub(super) fn parse_mouse_button(call: &DriverCall) -> AuvResult<(&'static str, i32)> {
  match optional_string(call, "button")
    .unwrap_or_else(|| "left".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "left" => Ok(("left", 0)),
    "right" => Ok(("right", 1)),
    "middle" => Ok(("middle", 2)),
    other => Err(format!(
      "invalid --button value {}; expected left, right, or middle",
      other
    )),
  }
}

pub(super) fn resolve_scroll_deltas(call: &DriverCall) -> AuvResult<(f64, f64, String)> {
  let explicit_delta_x = optional_f64(call, "delta_x")?;
  let explicit_delta_y = optional_f64(call, "delta_y")?;
  if explicit_delta_x.is_some() || explicit_delta_y.is_some() {
    let delta_x = explicit_delta_x.unwrap_or(0.0);
    let delta_y = explicit_delta_y.unwrap_or(0.0);
    return Ok((
      delta_x,
      delta_y,
      format!("delta_x={:.0},delta_y={:.0}", delta_x, delta_y),
    ));
  }

  let direction = required_non_empty_string(call, "direction")?.to_ascii_lowercase();
  let pages = optional_f64(call, "pages")?.unwrap_or(1.0);
  if !pages.is_finite() || pages <= 0.0 {
    return Err(format!(
      "invalid --pages value {:.3}: expected a positive finite number",
      pages
    ));
  }
  let magnitude = (pages * 480.0).round();
  let (delta_x, delta_y) = match direction.as_str() {
    "up" => (0.0, magnitude),
    "down" => (0.0, -magnitude),
    "left" => (magnitude, 0.0),
    "right" => (-magnitude, 0.0),
    other => {
      return Err(format!(
        "invalid --direction value {}; expected up, down, left, or right",
        other
      ));
    }
  };

  Ok((
    delta_x,
    delta_y,
    format!("direction={direction},pages={pages:.3}"),
  ))
}

pub(super) fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .map(str::trim)
}

pub(super) fn activate_target_app(app: &str) -> AuvResult<()> {
  let command = if looks_like_bundle_identifier(app) {
    format!(
      "tell application id {} to activate",
      osascript_string_literal(app)
    )
  } else {
    format!(
      "tell application {} to activate",
      osascript_string_literal(app)
    )
  };
  let args = vec!["-e".to_string(), command];
  run_command(OSASCRIPT_BINARY, &args).map(|_| ())
}

pub(super) struct ClipboardLock {
  path: PathBuf,
}

impl Drop for ClipboardLock {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

pub(super) fn acquire_clipboard_lock(timeout_ms: u64) -> AuvResult<ClipboardLock> {
  let path = env::temp_dir().join("auv-macos-clipboard.lock");
  let started_at = now_millis();

  loop {
    match fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&path)
    {
      Ok(mut file) => {
        let _ = writeln!(file, "pid={}", std::process::id());
        let _ = writeln!(file, "acquiredAt={}", started_at);
        return Ok(ClipboardLock { path });
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => {
        if now_millis().saturating_sub(started_at) > timeout_ms as u128 {
          return Err(format!(
            "timed out waiting for the global macOS clipboard lock after {} ms",
            timeout_ms
          ));
        }
        thread::sleep(Duration::from_millis(50));
      }
      Err(error) => {
        return Err(format!(
          "failed to acquire the global macOS clipboard lock {}: {error}",
          path.display()
        ));
      }
    }
  }
}

pub(super) fn capture_clipboard_snapshot() -> AuvResult<String> {
  Ok(
    run_swift_script(CAPTURE_CLIPBOARD_SCRIPT)?
      .trim()
      .to_string(),
  )
}

pub(super) fn restore_clipboard_snapshot(snapshot_payload: &str) -> AuvResult<()> {
  run_swift_script(&build_restore_clipboard_script(snapshot_payload)).map(|_| ())
}

pub(super) fn set_clipboard_text(text: &str) -> AuvResult<()> {
  run_swift_script(&build_set_clipboard_text_script(text)).map(|_| ())
}

pub(super) fn type_text_via_system_events(
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
  submit_settle_ms: u64,
) -> AuvResult<()> {
  let mut lines = vec!["tell application \"System Events\"".to_string()];
  if replace_existing {
    lines.push("keystroke \"a\" using {command down}".to_string());
    lines.push("delay 0.05".to_string());
    lines.push("key code 51".to_string());
    lines.push("delay 0.05".to_string());
  }
  lines.push(format!("keystroke {}", osascript_string_literal(text)));
  if let Some(submit_key) = submit_key {
    let key_code = special_key_code(submit_key)?;
    lines.push("delay 0.05".to_string());
    lines.push(format!("key code {key_code}"));
  }
  lines.push("end tell".to_string());
  run_osascript_lines(&lines)?;
  if submit_settle_ms > 0 {
    thread::sleep(Duration::from_millis(submit_settle_ms));
  }
  Ok(())
}

pub(super) fn paste_text_preserving_clipboard(
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
  submit_settle_ms: u64,
) -> AuvResult<()> {
  let _clipboard_lock = acquire_clipboard_lock(5_000)?;
  let clipboard_snapshot = capture_clipboard_snapshot()?;
  let action_result = (|| {
    set_clipboard_text(text)?;
    let mut lines = vec!["tell application \"System Events\"".to_string()];
    if replace_existing {
      lines.push("keystroke \"a\" using {command down}".to_string());
      lines.push("delay 0.05".to_string());
      lines.push("key code 51".to_string());
      lines.push("delay 0.05".to_string());
    }
    lines.push("keystroke \"v\" using {command down}".to_string());
    if let Some(submit_key) = submit_key {
      let key_code = special_key_code(submit_key)?;
      lines.push("delay 0.05".to_string());
      lines.push(format!("key code {key_code}"));
    }
    lines.push("end tell".to_string());
    run_osascript_lines(&lines)?;
    if submit_settle_ms > 0 {
      thread::sleep(Duration::from_millis(submit_settle_ms));
    }
    Ok(())
  })();
  let restore_result = restore_clipboard_snapshot(&clipboard_snapshot);

  match (action_result, restore_result) {
    (Ok(()), Ok(())) => Ok(()),
    (Err(action_error), Ok(())) => Err(action_error),
    (Ok(()), Err(restore_error)) => Err(format!(
      "restored pasted text action but failed to restore clipboard: {restore_error}"
    )),
    (Err(action_error), Err(restore_error)) => Err(format!(
      "{action_error}; additionally failed to restore clipboard: {restore_error}"
    )),
  }
}

pub(super) fn special_key_code(raw: &str) -> AuvResult<u32> {
  match raw.trim().to_ascii_lowercase().as_str() {
    "return" => Ok(36),
    "enter" => Ok(76),
    "tab" => Ok(48),
    "escape" | "esc" => Ok(53),
    "space" => Ok(49),
    other => Err(format!(
      "invalid submit key {}; supported values are return, enter, tab, escape, and space",
      other
    )),
  }
}

pub(super) fn run_osascript_lines(lines: &[String]) -> AuvResult<CommandOutput> {
  let mut args = Vec::with_capacity(lines.len() * 2);
  for line in lines {
    args.push("-e".to_string());
    args.push(line.clone());
  }
  run_command(OSASCRIPT_BINARY, &args)
}

pub(super) fn send_key_input(key: &str, settle_ms: u64) -> AuvResult<()> {
  if key.contains('+') {
    send_shortcut(key)?;
  } else if let Ok(key_code) = special_key_code(key) {
    run_osascript_lines(&[
      "tell application \"System Events\"".to_string(),
      format!("key code {key_code}"),
      "end tell".to_string(),
    ])?;
  } else if key.chars().count() == 1 {
    run_osascript_lines(&[format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(key)
    )])?;
  } else {
    return Err(format!(
      "invalid key {}; use a special key like Return, a shortcut like cmd+f, or debug.typeText for multi-character text",
      key
    ));
  }

  if settle_ms > 0 {
    thread::sleep(Duration::from_millis(settle_ms));
  }
  Ok(())
}

pub(super) fn send_shortcut(shortcut: &str) -> AuvResult<()> {
  let parsed = parse_shortcut(shortcut)?;
  let line = if parsed.modifiers.is_empty() {
    format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(&parsed.key)
    )
  } else {
    format!(
      "tell application \"System Events\" to keystroke {} using {{{}}}",
      osascript_string_literal(&parsed.key),
      parsed.modifiers.join(", ")
    )
  };
  run_osascript_lines(&[line]).map(|_| ())
}

#[derive(Debug)]
pub(super) struct ParsedShortcut {
  pub(super) key: String,
  pub(super) modifiers: Vec<&'static str>,
}

pub(super) fn parse_shortcut(shortcut: &str) -> AuvResult<ParsedShortcut> {
  let raw_parts = shortcut
    .split('+')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>();
  if raw_parts.len() < 2 {
    return Err(format!(
      "invalid shortcut {}; expected a form like cmd+f or cmd+shift+p",
      shortcut
    ));
  }

  let key = raw_parts
    .last()
    .map(|value| value.to_ascii_lowercase())
    .ok_or_else(|| format!("invalid shortcut {}; missing key", shortcut))?;
  if key.chars().count() != 1 {
    return Err(format!(
      "invalid shortcut {}; only single-character keys are currently supported",
      shortcut
    ));
  }

  let mut modifiers = Vec::new();
  for raw_modifier in &raw_parts[..raw_parts.len() - 1] {
    let modifier = match raw_modifier.to_ascii_lowercase().as_str() {
      "cmd" | "command" => "command down",
      "shift" => "shift down",
      "alt" | "option" => "option down",
      "ctrl" | "control" => "control down",
      other => {
        return Err(format!(
          "invalid shortcut {}; unsupported modifier {}",
          shortcut, other
        ));
      }
    };
    if !modifiers.contains(&modifier) {
      modifiers.push(modifier);
    }
  }

  Ok(ParsedShortcut { key, modifiers })
}

pub(super) fn render_type_text_report(
  app: &str,
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
) -> String {
  let mut lines = vec![
    format!("typedAt={}", now_millis()),
    format!("app={app}"),
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
  ];
  if let Some(submit_key) = submit_key {
    lines.push(format!("submitKey={submit_key}"));
  }
  lines.join("\n")
}

pub(super) fn render_activate_app_report(app: &str, settle_ms: u64) -> String {
  [
    format!("activatedAt={}", now_millis()),
    format!("app={app}"),
    format!("settleMs={settle_ms}"),
  ]
  .join("\n")
}

pub(super) fn looks_like_bundle_identifier(raw: &str) -> bool {
  raw.contains('.')
    && raw
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

pub(super) fn osascript_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

pub(super) fn launch_host_process() -> String {
  env::args()
    .next()
    .map(PathBuf::from)
    .as_ref()
    .and_then(|value| value.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("auv-cli")
    .to_string()
}

pub(super) fn swift_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

pub(super) fn sanitize_file_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();

  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

pub(crate) fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy artifact from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub(crate) fn sanitized_artifact_name(raw: &str) -> String {
  sanitize_file_component(raw)
}

pub(super) struct CommandOutput {
  pub(super) stdout: String,
}
