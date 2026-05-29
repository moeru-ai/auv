// File: src/driver/macos/control/pointer.rs
use std::thread;
use std::time::Duration;

use super::super::support::{
  artifacts::{build_text_artifact, sanitize_file_component},
  call::{
    app_identifier, optional_i64, optional_positive_u64, parse_mouse_button, required_f64,
    resolve_scroll_deltas,
  },
  display::{enumerate_displays, render_display_note},
  geometry::resolve_display_point,
};
use super::super::{DriverCall, DriverResponse};
use super::common::{activate_app_if_needed, parse_input_policy, resolve_click_interval_ms};
use crate::model::AuvResult;

pub(crate) fn click_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let (button_name, button_code) = parse_mouse_button(call)?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y)
    .ok_or_else(|| format!("logical point ({x:.3}, {y:.3}) is outside all connected displays"))?;

  activate_app_if_needed(&app_identifier(call).unwrap_or_default())?;
  auv_driver_macos::native::pointer::click_point(
    x,
    y,
    button_code,
    click_count,
    click_interval_ms,
  )?;
  if settle_ms > 0 {
    thread::sleep(Duration::from_millis(settle_ms));
  }

  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!("button={button_name}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=restored-to-original".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "click-point",
    "txt",
    &format!(
      "click-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Clicked a macOS logical point through Quartz and recorded its coordinate contract.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Clicked {} at global logical point ({x:.3}, {y:.3}) on display #{}.",
      button_name, resolution.display.display_id
    ),
    backend: Some("macos.swift.quartz-click".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("button={button_name}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!("settleMs={settle_ms}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
      "cursorAfter=restored-to-original".to_string(),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn scroll_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let (delta_x, delta_y, normalized_scroll) = resolve_scroll_deltas(call)?;
  let input_policy = parse_input_policy(call)?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y)
    .ok_or_else(|| format!("logical point ({x:.3}, {y:.3}) is outside all connected displays"))?;

  if input_policy == auv_driver::InputPolicy::ForegroundPreferred {
    activate_app_if_needed(&app_identifier(call).unwrap_or_default())?;
  }
  let scroll_outcome = crate::driver::macos::typed::session::scroll_point_bridge(
    x,
    y,
    delta_x,
    delta_y,
    input_policy,
    0,
  )?;

  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!("deltaX={delta_x:.0}"),
    format!("deltaY={delta_y:.0}"),
    format!("normalizedScroll={normalized_scroll}"),
    format!("inputPolicy={}", scroll_outcome.input_policy),
    format!("inputBridge={}", scroll_outcome.input_bridge),
    format!("selectedPath={}", scroll_outcome.selected_path),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=restored-to-original".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "scroll-point",
    "txt",
    &format!(
      "scroll-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Scrolled at a macOS logical point through Quartz and recorded its coordinate contract.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Scrolled at global logical point ({x:.3}, {y:.3}) on display #{} with {}.",
      resolution.display.display_id, normalized_scroll
    ),
    backend: Some("macos.typed.input.scroll".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("deltaX={delta_x:.0}"),
      format!("deltaY={delta_y:.0}"),
      format!("normalizedScroll={normalized_scroll}"),
      format!("inputPolicy={}", scroll_outcome.input_policy),
      format!("inputBridge={}", scroll_outcome.input_bridge),
      format!("selectedPath={}", scroll_outcome.selected_path),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
      "cursorAfter=restored-to-original".to_string(),
    ],
    artifacts: vec![artifact],
  })
}
