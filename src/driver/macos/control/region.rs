use std::collections::BTreeMap;

use super::super::*;

pub(crate) fn observe_window_region(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-region-observe".to_string());
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
  let region = region_ratios_from_call(call)?;
  let capture = super::window_ocr::capture_resolved_window_observation(call, &label)?;
  let ocr_region = region.to_observed_rect(capture.dimensions.width, capture.dimensions.height)?;
  let detection = detect_screen_rows(
    capture.screenshot_path.as_path(),
    min_confidence,
    max_observations,
    Some(&ocr_region),
  )?;
  let rows = detection.rows;
  let json = render_observe_window_region_json(
    &rows,
    &ocr_region,
    &capture.dimensions,
    &capture.screenshot_path,
  )?;
  let json_artifact = build_text_artifact(
    "window-region-observation",
    "json",
    &format!("{}-rows", sanitize_file_component(&label)),
    json,
    "Machine-readable OCR row observation for a window region.",
  )?;
  // WORKAROUND: Window-region observation records the screenshot and OCR-region
  // bounds, but does not yet emit a full capture contract artifact. Remove this
  // once window capture contract staging is shared with scan artifacts.
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: capture.screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Window screenshot captured for region observation.".to_string()),
  };

  Ok(DriverResponse {
    summary: format!(
      "Observed {} OCR row(s) in resolved window region.",
      rows.len()
    ),
    backend: Some("macos.vision.observe-window-region".to_string()),
    signals: crate::driver::macos::observe::row_detection_signals(rows.len()),
    notes: vec![
      format!("scope={}", capture.scope),
      format!("windowRef={}", capture.capture_source),
      format!("region={}", render_rect_compact(&ocr_region)),
      format!("rows.count={}", rows.len()),
      format!("strategy={}", detection.strategy),
      format!("minConfidence={min_confidence:.3}"),
      format!("maxObservations={max_observations}"),
      format!(
        "screenshotPixels={}x{}",
        capture.dimensions.width, capture.dimensions.height
      ),
    ],
    artifacts: vec![screenshot_artifact, json_artifact],
  })
}

pub(crate) fn scroll_window_region(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).ok_or_else(|| {
    "scroll_window_region requires --target <application-id> or --app".to_string()
  })?;
  let raw_direction = optional_string(call, "direction").unwrap_or_else(|| "down".to_string());
  let direction = normalize_scroll_direction(&raw_direction)?.to_string();
  let amount = optional_f64(call, "amount")?.unwrap_or(6.0).max(1.0);
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(250);
  let region = region_ratios_from_call(call)?;
  activate_target_app(&app)?;
  let snapshot = super::super::observe::observe_windows_snapshot(24, &app)?;
  let xcap_displays = super::super::capture::xcap_backend::list_displays()?;
  let display_snapshot = enumerate_displays()?;
  let selector = parse_app_selector(&app)?;
  let resolved_app = resolve_app_ref(&snapshot, &selector)?;
  let candidate = resolve_window_candidate(
    &snapshot,
    &resolved_app,
    &xcap_displays,
    &parse_window_selection(call)?,
  )?;
  let window = &candidate.window_ref;
  let x =
    window.bounds.x as f64 + window.bounds.width as f64 * ((region.left + region.right) / 2.0);
  let y =
    window.bounds.y as f64 + window.bounds.height as f64 * ((region.top + region.bottom) / 2.0);
  let resolution = resolve_display_point(&display_snapshot, x, y).ok_or_else(|| {
    format!("resolved scroll point ({x:.3}, {y:.3}) is outside all connected displays")
  })?;
  let (delta_x, delta_y) = scan_scroll_delta(&direction, amount)?;
  crate::driver::macos::native::pointer::scroll_point(x, y, delta_x, delta_y)?;
  if settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(settle_ms));
  }

  let report = [
    "coordinateSpace=global-logical".to_string(),
    format!("applicationId={app}"),
    format!("appSelector={}", resolved_app.selector.raw),
    format!("matchStrategy={}", resolved_app.match_strategy),
    format!(
      "resolvedAppBundleId={}",
      resolved_app
        .resolved_bundle_id
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!("resolvedAppName={}", resolved_app.resolved_app_name),
    format!("windowId={}", window.window_number),
    format!("windowTitle={}", window.title),
    format!("windowBounds={}", render_rect_compact(&window.bounds)),
    format!(
      "regionRatios={:.3},{:.3},{:.3},{:.3}",
      region.left, region.top, region.right, region.bottom
    ),
    format!("scrollPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "displayBounds={}",
      render_rect_compact(&resolution.display.bounds)
    ),
    format!("candidateIndex={}", candidate.candidate_index),
    format!("selectionReason={}", candidate.selection_reason),
    format!(
      "isFullyContainedInDisplay={}",
      candidate.is_fully_contained_in_display
    ),
    format!(
      "displayRef={}",
      candidate
        .display_ref
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!(
      "nativeDisplayId={}",
      candidate
        .native_display_id
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!("candidateArea={}", candidate.area),
    format!("direction={direction}"),
    format!("amount={amount:.3}"),
    format!("deltaX={delta_x:.0}"),
    format!("deltaY={delta_y:.0}"),
    format!("settleMs={settle_ms}"),
  ]
  .join("\n");
  let artifact = build_text_artifact(
    "window-region-scroll",
    "txt",
    "window-region-scroll",
    report,
    "Scrolled at the center of a resolved window region.",
  )?;

  Ok(DriverResponse {
    summary: format!("Scrolled window region {direction} by amount {amount:.3}."),
    backend: Some("macos.swift.quartz-scroll-window-region".to_string()),
    signals: BTreeMap::from([
      ("scroll.direction".to_string(), direction),
      ("scroll.amount".to_string(), format!("{amount:.3}")),
    ]),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("windowId={}", window.window_number),
      format!("windowBounds={}", render_rect_compact(&window.bounds)),
      format!(
        "regionRatios={:.3},{:.3},{:.3},{:.3}",
        region.left, region.top, region.right, region.bottom
      ),
      format!("scrollPoint={x:.3},{y:.3}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      format!("displayId={}", resolution.display.display_id),
      format!("candidateIndex={}", candidate.candidate_index),
      format!("selectionReason={}", candidate.selection_reason),
      format!("deltaX={delta_x:.0}"),
      format!("deltaY={delta_y:.0}"),
      format!("settleMs={settle_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

fn scan_scroll_delta(direction: &str, amount: f64) -> AuvResult<(f64, f64)> {
  match normalize_scroll_direction(direction)? {
    "down" => Ok((0.0, -amount)),
    "up" => Ok((0.0, amount)),
    "right" => Ok((-amount, 0.0)),
    "left" => Ok((amount, 0.0)),
    _ => unreachable!("normalize_scroll_direction only returns supported directions"),
  }
}

fn normalize_scroll_direction(direction: &str) -> AuvResult<&'static str> {
  match direction.trim().to_ascii_lowercase().as_str() {
    "down" => Ok("down"),
    "up" => Ok("up"),
    "right" => Ok("right"),
    "left" => Ok("left"),
    other => Err(format!(
      "invalid scroll direction {other:?}; expected down, up, left, or right"
    )),
  }
}

#[derive(Clone, Copy, Debug)]
struct RegionRatios {
  left: f64,
  top: f64,
  right: f64,
  bottom: f64,
}

impl RegionRatios {
  fn to_observed_rect(self, width: i64, height: i64) -> AuvResult<ObservedRect> {
    if width <= 0 || height <= 0 {
      return Err(format!(
        "invalid screenshot dimensions {}x{}: expected positive width and height",
        width, height
      ));
    }
    let (left_px, right_px) = ratio_edges_to_pixels(self.left, self.right, width);
    let (top_px, bottom_px) = ratio_edges_to_pixels(self.top, self.bottom, height);

    Ok(ObservedRect {
      x: left_px,
      y: top_px,
      width: right_px - left_px,
      height: bottom_px - top_px,
    })
  }
}

fn ratio_edges_to_pixels(left: f64, right: f64, size: i64) -> (i64, i64) {
  let size_f = size as f64;
  let left_px = (left * size_f).floor() as i64;
  let left_px = left_px.clamp(0, size - 1);
  let right_px = (right * size_f).ceil() as i64;
  let right_px = right_px.clamp(left_px + 1, size);

  (left_px, right_px)
}

fn region_ratios_from_call(call: &DriverCall) -> AuvResult<RegionRatios> {
  let left = optional_f64(call, "region_left_ratio")?.unwrap_or(0.0);
  let top = optional_f64(call, "region_top_ratio")?.unwrap_or(0.0);
  let right = optional_f64(call, "region_right_ratio")?.unwrap_or(1.0);
  let bottom = optional_f64(call, "region_bottom_ratio")?.unwrap_or(1.0);
  validate_region_ratios(left, top, right, bottom)?;
  Ok(RegionRatios {
    left,
    top,
    right,
    bottom,
  })
}

fn validate_region_ratios(left: f64, top: f64, right: f64, bottom: f64) -> AuvResult<()> {
  if !(0.0 <= left && left < right && right <= 1.0) {
    return Err("invalid region x ratios: expected 0.0 <= left < right <= 1.0".to_string());
  }
  if !(0.0 <= top && top < bottom && bottom <= 1.0) {
    return Err("invalid region y ratios: expected 0.0 <= top < bottom <= 1.0".to_string());
  }
  Ok(())
}

fn render_observe_window_region_json(
  rows: &[ObservedOcrRow],
  region: &ObservedRect,
  dimensions: &ScreenshotDimensions,
  screenshot_path: &std::path::Path,
) -> AuvResult<String> {
  let rows = rows
    .iter()
    .map(|row| {
      serde_json::json!({
        "row_index": row.row_index,
        "source": row.source,
        "text": row.text_fragments.join(" | "),
        "text_fragments": row.text_fragments,
        "bounds": {
          "x": row.bounds.x,
          "y": row.bounds.y,
          "width": row.bounds.width,
          "height": row.bounds.height,
        },
      })
    })
    .collect::<Vec<_>>();
  serde_json::to_string_pretty(&serde_json::json!({
    "extractor": "ocr-row",
    "coordinate_space": "window_screenshot_pixels",
    "screenshot_path": screenshot_path.display().to_string(),
    "screenshot_width": dimensions.width,
    "screenshot_height": dimensions.height,
    "region": {
      "x": region.x,
      "y": region.y,
      "width": region.width,
      "height": region.height,
    },
    "rows": rows,
  }))
  .map(|mut rendered| {
    rendered.push('\n');
    rendered
  })
  .map_err(|error| format!("failed to render window region observation json: {error}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_region_ratios_rejects_inverted_region() {
    let error = validate_region_ratios(0.8, 0.2, 0.4, 0.9).expect_err("region should fail");
    assert!(error.contains("expected 0.0 <= left < right <= 1.0"));
  }

  #[test]
  fn validate_region_ratios_accepts_normal_region() {
    validate_region_ratios(0.1, 0.2, 0.9, 0.8).expect("region should pass");
  }

  #[test]
  fn scan_scroll_delta_defaults_to_vertical_down() {
    let (delta_x, delta_y) = scan_scroll_delta("down", 6.0).expect("delta");
    assert_eq!(delta_x, 0.0);
    assert!(delta_y < 0.0);
  }

  #[test]
  fn scan_scroll_delta_normalizes_direction_case() {
    let (delta_x, delta_y) = scan_scroll_delta("DOWN", 6.0).expect("delta");
    assert_eq!((delta_x, delta_y), (0.0, -6.0));
  }

  #[test]
  fn scan_scroll_delta_maps_all_cardinal_directions() {
    assert_eq!(scan_scroll_delta("up", 4.0).expect("up"), (0.0, 4.0));
    assert_eq!(scan_scroll_delta("down", 4.0).expect("down"), (0.0, -4.0));
    assert_eq!(scan_scroll_delta("left", 4.0).expect("left"), (4.0, 0.0));
    assert_eq!(scan_scroll_delta("right", 4.0).expect("right"), (-4.0, 0.0));
  }

  #[test]
  fn scan_scroll_delta_rejects_unknown_direction() {
    let error = scan_scroll_delta("diagonal", 4.0).expect_err("direction should fail");
    assert!(error.contains("expected down, up, left, or right"));
  }

  #[test]
  fn region_ratios_to_observed_rect_clamps_near_right_edge() {
    let region = RegionRatios {
      left: 0.996,
      top: 0.996,
      right: 1.0,
      bottom: 1.0,
    }
    .to_observed_rect(100, 100)
    .expect("valid dimensions should convert");

    assert_eq!(
      region,
      ObservedRect {
        x: 99,
        y: 99,
        width: 1,
        height: 1,
      }
    );
    assert!(region.x < 100);
    assert!(region.y < 100);
    assert!(region.x + region.width <= 100);
    assert!(region.y + region.height <= 100);
  }

  #[test]
  fn render_observe_window_region_json_includes_coordinate_semantics() {
    let json = render_observe_window_region_json(
      &[],
      &ObservedRect {
        x: 99,
        y: 10,
        width: 1,
        height: 80,
      },
      &ScreenshotDimensions {
        width: 100,
        height: 200,
      },
      std::path::Path::new("/tmp/window.png"),
    )
    .expect("json should render");
    let value: serde_json::Value = serde_json::from_str(&json).expect("json should parse");

    assert_eq!(
      value["coordinate_space"],
      serde_json::Value::String("window_screenshot_pixels".to_string())
    );
    assert_eq!(value["screenshot_width"], serde_json::Value::from(100));
    assert_eq!(value["screenshot_height"], serde_json::Value::from(200));
  }
}
