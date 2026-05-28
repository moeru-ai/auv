// File: src/driver/macos/control/window.rs
use super::super::support::runtime::activate_target_app;
use super::super::*;
use super::common::{ClickPointCallOptions, build_click_point_call, resolve_click_interval_ms};
use super::pointer::click_point;

pub(crate) fn click_window_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| {
      "operation requires --target <application-id> or --app <application-id>".to_string()
    })?;
  let selector = parse_app_selector(&app)?;
  let selection = parse_window_selection(call)?;
  activate_target_app(&app)?;

  let displays = super::super::capture::xcap_backend::list_displays()?;
  let snapshot = super::super::observe::observe_windows_snapshot(128, &app)?;
  let resolved_app = resolve_app_ref(&snapshot, &selector)?;
  let selected = resolve_window_candidate(&snapshot, &resolved_app, &displays, &selection)?;
  let window = &selected.window_ref;

  let (logical_x, logical_y, coordinate_summary) = resolve_window_point(call, window)?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
    call.run_context.clone(),
    ClickPointCallOptions {
      x: logical_x,
      y: logical_y,
      button: &button_label,
      click_count,
      click_interval_ms: Some(click_interval_ms),
      settle_ms: None,
      app: Some(&app),
    },
  );
  let _ = click_point(&nested_call)?;

  let artifact = build_text_artifact(
    "click-window-point",
    "txt",
    &format!("click-window-point-{}", sanitize_file_component(&app)),
    [
      format!("app={app}"),
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
      format!("windowRef={}", window.window_number),
      format!("windowTitle={}", window.title),
      format!("windowBounds={}", render_rect_compact(&window.bounds)),
      format!("ownerBundleId={}", window.owner_bundle_id),
      format!("ownerPid={}", window.owner_pid),
      format!("candidateIndex={}", selected.candidate_index),
      format!("selectionReason={}", selected.selection_reason),
      format!(
        "isFullyContainedInDisplay={}",
        selected.is_fully_contained_in_display
      ),
      format!("resolvedLogicalPoint={logical_x:.3},{logical_y:.3}"),
      coordinate_summary.clone(),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
    ]
    .join("\n"),
    "Clicked a point relative to a resolved macOS app window.",
  )?;
  let mut notes = vec![
    format!("app={app}"),
    format!("appSelector={}", resolved_app.selector.raw),
    format!("matchStrategy={}", resolved_app.match_strategy),
    format!(
      "resolvedAppBundleId={}",
      resolved_app
        .resolved_bundle_id
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!("windowRef={}", window.window_number),
    format!("windowBounds={}", render_rect_compact(&window.bounds)),
    format!("candidateIndex={}", selected.candidate_index),
    format!("selectionReason={}", selected.selection_reason),
    format!(
      "isFullyContainedInDisplay={}",
      selected.is_fully_contained_in_display
    ),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    coordinate_summary,
    format!("clickIntervalMs={click_interval_ms}"),
  ];
  if !window.owner_bundle_id.is_empty() {
    notes.push(format!("ownerBundleId={}", window.owner_bundle_id));
  }
  if !window.title.is_empty() {
    notes.push(format!("windowTitle={}", window.title));
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked {} window-relative point in {} at global logical point ({logical_x:.3}, {logical_y:.3}).",
      button_label, app
    ),
    backend: Some("macos.desktop.window-relative-click".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}
