use super::super::*;
use super::common::{ClickPointCallOptions, build_click_point_call, resolve_click_interval_ms};
use super::pointer::click_point;

pub(super) fn click_screen_text_signals(text: &str) -> std::collections::BTreeMap<String, String> {
  std::collections::BTreeMap::from([("click.resolved_text".to_string(), text.to_string())])
}

pub(super) fn click_screen_row_signals(
  clicked_row_index: usize,
  detected_row_count: usize,
) -> std::collections::BTreeMap<String, String> {
  std::collections::BTreeMap::from([
    (
      "rows.clicked_index".to_string(),
      clicked_row_index.to_string(),
    ),
    ("rows.count".to_string(), detected_row_count.to_string()),
  ])
}

pub(crate) fn click_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-click-{}", sanitize_file_component(&query));
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let (screenshot_path, capture_contract) =
    crate::driver::macos::capture::xcap_backend::capture_main_display_to_path(&label)?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let match_index = optional_i64(call, "match_index")?.unwrap_or(0).max(0) as usize;
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    screenshot_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref());
  let matched = filtered_matches.get(match_index).copied().ok_or_else(|| {
    format!(
      "no filtered OCR text match at index {} for query {} (found {} after filtering from {})",
      match_index,
      query,
      filtered_matches.len(),
      ocr_snapshot.matches.len()
    )
  })?;
  let anchor_offset_x = optional_f64(call, "anchor_offset_x")?.unwrap_or(0.0);
  let anchor_offset_y = optional_f64(call, "anchor_offset_y")?.unwrap_or(0.0);
  let (match_center_x, match_center_y) = ocr_match_center(matched);
  let screenshot_center_x = match_center_x + anchor_offset_x;
  let screenshot_center_y = match_center_y + anchor_offset_y;
  let (logical_x, logical_y) =
    crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture_contract,
      screenshot_center_x,
      screenshot_center_y,
    )?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
    ClickPointCallOptions {
      x: logical_x,
      y: logical_y,
      button: &button_label,
      click_count,
      click_interval_ms: Some(click_interval_ms),
      settle_ms: Some(settle_ms),
      app: None,
    },
  );
  let _ = click_point(&nested_call)?;

  let report_artifact = build_text_artifact(
    "screen-text-click",
    "txt",
    &format!("screen-text-click-{}", sanitize_file_component(&query)),
    [
      format!("query={query}"),
      format!("matchIndex={match_index}"),
      format!("filteredMatchCount={}", filtered_matches.len()),
      format!("minConfidence={min_confidence:.3}"),
      format!("matchText={}", matched.text),
      format!("matchBounds={}", render_rect_compact(&matched.bounds)),
      format!("matchConfidence={:.3}", matched.confidence),
      format!("anchorOffset={anchor_offset_x:.3},{anchor_offset_y:.3}"),
      format!("screenshotCenter={screenshot_center_x:.3},{screenshot_center_y:.3}"),
      format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!("settleMs={settle_ms}"),
    ]
    .join("\n"),
    "Clicked an OCR text anchor projected from screenshot pixels to logical coordinates.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR click-anchor detection.".to_string()),
  };
  let mut notes = vec![
    format!("query={query}"),
    format!("matchIndex={match_index}"),
    format!("filteredMatchCount={}", filtered_matches.len()),
    format!("matchText={}", matched.text),
    format!("matchBounds={}", render_rect_compact(&matched.bounds)),
    format!("minConfidence={min_confidence:.3}"),
    format!("anchorOffset={anchor_offset_x:.3},{anchor_offset_y:.3}"),
    format!("screenshotCenter={screenshot_center_x:.3},{screenshot_center_y:.3}"),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("button={button_label}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
  ];
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked OCR text anchor {} for query {} at logical point ({logical_x:.3}, {logical_y:.3}).",
      matched.text, query
    ),
    backend: Some("macos.vision.click-screen-text".to_string()),
    signals: click_screen_text_signals(&matched.text),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(crate) fn click_screen_row(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "screen-row-click".to_string());
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let (screenshot_path, capture_contract) =
    crate::driver::macos::capture::xcap_backend::capture_main_display_to_path(&label)?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(128)
    .clamp(1, 512);
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let row_index = optional_i64(call, "row_index")?.unwrap_or(1).clamp(1, 64) as usize - 1;
  let row_anchor_x_ratio = optional_f64(call, "row_anchor_x_ratio")?.unwrap_or(0.25);
  let row_anchor_y_ratio = optional_f64(call, "row_anchor_y_ratio")?.unwrap_or(0.5);
  let row_anchor_mode =
    optional_string(call, "row_anchor_mode").unwrap_or_else(|| "title_band".to_string());
  for (label, value) in [
    ("row_anchor_x_ratio", row_anchor_x_ratio),
    ("row_anchor_y_ratio", row_anchor_y_ratio),
  ] {
    if !(0.0..=1.0).contains(&value) {
      return Err(format!(
        "invalid --{} value {:.3}: expected a ratio within 0.0..=1.0",
        label, value
      ));
    }
  }
  match row_anchor_mode.as_str() {
    "title_band" | "row_ratio" => {}
    other => {
      return Err(format!(
        "invalid --row_anchor_mode value {}: expected title_band or row_ratio",
        other
      ));
    }
  }

  let detection = detect_screen_rows(
    screenshot_path.as_path(),
    min_confidence,
    max_observations,
    region.as_ref(),
  )?;
  let rows = detection.rows;
  let row = rows.get(row_index).ok_or_else(|| {
    format!(
      "no visible row at index {} (detected {} row(s) with strategy {})",
      row_index + 1,
      rows.len(),
      detection.strategy
    )
  })?;

  let screenshot_center_x = match row_anchor_mode.as_str() {
    "row_ratio" => row.bounds.x as f64 + (row.bounds.width as f64 * row_anchor_x_ratio),
    "title_band" => {
      let region_left = region
        .as_ref()
        .map(|value| value.x as f64 + (value.width as f64 * 0.16))
        .unwrap_or(row.bounds.x as f64 + (row.bounds.width as f64 * 0.16));
      let cover_offset = row.bounds.x as f64 + (row.bounds.height as f64 * 1.05) + 18.0;
      cover_offset
        .max(region_left)
        .min((row.bounds.x + row.bounds.width - 24).max(row.bounds.x) as f64)
    }
    _ => unreachable!(),
  };
  let screenshot_center_y = row.bounds.y as f64 + (row.bounds.height as f64 * row_anchor_y_ratio);
  let (logical_x, logical_y) =
    crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture_contract,
      screenshot_center_x,
      screenshot_center_y,
    )?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
    ClickPointCallOptions {
      x: logical_x,
      y: logical_y,
      button: &button_label,
      click_count,
      click_interval_ms: Some(click_interval_ms),
      settle_ms: Some(settle_ms),
      app: None,
    },
  );
  let _ = click_point(&nested_call)?;

  let report_artifact = build_text_artifact(
    "screen-row-click",
    "txt",
    &format!("screen-row-click-{}", sanitize_file_component(&label)),
    [
      format!("rowStrategy={}", detection.strategy),
      format!("rowIndex={}", row_index + 1),
      format!("detectedRowCount={}", rows.len()),
      format!("matchCount={}", detection.raw_match_count),
      format!("filteredMatchCount={}", detection.filtered_match_count),
      format!("minConfidence={min_confidence:.3}"),
      format!("rowBounds={}", render_rect_compact(&row.bounds)),
      format!("rowSource={}", row.source),
      format!("rowText={}", row.text_fragments.join(" | ")),
      format!("rowAnchorMode={row_anchor_mode}"),
      format!("rowAnchorRatio={row_anchor_x_ratio:.3},{row_anchor_y_ratio:.3}"),
      format!("screenshotCenter={screenshot_center_x:.3},{screenshot_center_y:.3}"),
      format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!("settleMs={settle_ms}"),
    ]
    .join("\n"),
    "Detected a visible row (OCR first, then visual-band fallback), projected a row-derived anchor point, and clicked it.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for visible OCR row detection before row click.".to_string()),
  };
  let mut notes = vec![
    format!("rowStrategy={}", detection.strategy),
    format!("rowIndex={}", row_index + 1),
    format!("detectedRowCount={}", rows.len()),
    format!("rowSource={}", row.source),
    format!("rowBounds={}", render_rect_compact(&row.bounds)),
    format!("rowText={}", row.text_fragments.join(" | ")),
    format!("rowAnchorMode={row_anchor_mode}"),
    format!("rowAnchorRatio={row_anchor_x_ratio:.3},{row_anchor_y_ratio:.3}"),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("button={button_label}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
  ];
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked visible row {} with strategy {} at logical point ({logical_x:.3}, {logical_y:.3}).",
      row_index + 1,
      detection.strategy
    ),
    backend: Some(format!(
      "macos.vision.click-screen-row.{}",
      detection.strategy
    )),
    signals: click_screen_row_signals(row_index + 1, rows.len()),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

#[cfg(test)]
mod tests {
  use super::{click_screen_row_signals, click_screen_text_signals};

  #[test]
  fn click_screen_text_signals_exposes_resolved_text() {
    let signals = click_screen_text_signals("Play Now");

    assert_eq!(
      signals.get("click.resolved_text"),
      Some(&"Play Now".to_string())
    );
  }

  #[test]
  fn click_screen_row_signals_expose_clicked_index_and_count() {
    let signals = click_screen_row_signals(2, 5);

    assert_eq!(signals.get("rows.clicked_index"), Some(&"2".to_string()));
    assert_eq!(signals.get("rows.count"), Some(&"5".to_string()));
  }
}
