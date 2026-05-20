use std::collections::BTreeMap;

use super::*;

const DEFAULT_PREVIEW_MS: u64 = 250;

pub(crate) fn overlay_show_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label = optional_non_empty_string(call, "label").unwrap_or_else(|| "AUV".to_string());
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  crate::driver::macos::native::overlay::show_cursor(x, y, &label)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    crate::driver::macos::native::overlay::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "show_cursor".to_string()),
    ("event", "shown".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("windowShape", "small-floating".to_string()),
    ("lifecycle", "in-process".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-show-cursor",
    report,
    "Recorded an experimental macOS overlay cursor show command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Showed experimental AUV overlay cursor at global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-poc".to_string()),
    signals: native_overlay_signals(controller_pid, "shown"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "windowShape=small-floating".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("label={label}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_hide_cursor(_call: &DriverCall) -> AuvResult<DriverResponse> {
  crate::driver::macos::native::overlay::hide_cursor()?;
  let controller_pid = std::process::id();
  let report = overlay_report([
    ("operation", "hide_cursor".to_string()),
    ("event", "hidden".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("lifecycle", "in-process".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-hide-cursor",
    report,
    "Recorded an experimental macOS overlay cursor hide command.",
  )?;

  Ok(DriverResponse {
    summary: "Hid the experimental AUV overlay cursor in the current process.".to_string(),
    backend: Some("macos.swift.overlay-ffi-poc".to_string()),
    signals: native_overlay_signals(controller_pid, "hidden"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
    ],
    artifacts: vec![artifact],
  })
}

/// Debug-only wrapped click: shows overlay cursor, clicks, then hides overlay.
///
/// Does NOT modify `debug.clickPoint` behavior.  Flicker acceptability must be
/// confirmed by manual observation before this is considered for any non-debug path.
pub(crate) fn overlay_click_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label = optional_non_empty_string(call, "label").unwrap_or_else(|| "AUV".to_string());
  let preview_ms = optional_positive_u64(call, "preview_ms")?.unwrap_or(DEFAULT_PREVIEW_MS);
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = optional_positive_u64(call, "click_interval_ms")?
    .unwrap_or(80)
    .min(1000);
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let (button_name, button_code) = parse_mouse_button(call)?;

  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y)
    .ok_or_else(|| format!("logical point ({x:.3}, {y:.3}) is outside all connected displays"))?;

  let app = app_identifier(call).unwrap_or_default();
  if !app.is_empty() {
    activate_target_app(&app)?;
  }

  // 1. Show overlay cursor.
  crate::driver::macos::native::overlay::show_cursor(x, y, &label)?;
  let controller_pid = std::process::id();
  let show_event = "shown".to_string();

  // 2. Hold overlay for preview visibility before the click.
  if preview_ms > 0 {
    crate::driver::macos::native::overlay::pump_events(preview_ms)?;
  }

  // 3. Click. Native pointer bridge handles warp-to-target + CGEvent + warp-restore internally.
  let click_result = crate::driver::macos::native::pointer::click_point(
    x,
    y,
    button_code,
    click_count,
    click_interval_ms,
  );

  if settle_ms > 0 {
    crate::driver::macos::native::overlay::pump_events(settle_ms)?;
  }

  // 4. Hide overlay cursor regardless of click success.
  let hide_event =
    match crate::driver::macos::native::overlay::hide_cursor().map(|_| "hidden".to_string()) {
      Ok(event) => event,
      Err(_) => "hide_failed".to_string(),
    };

  // Propagate click errors after overlay cleanup.
  click_result?;

  let report = [
    "operation=overlay_click_point".to_string(),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!("label={label}"),
    format!("previewMs={preview_ms}"),
    format!("button={button_name}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
    format!("controllerPid={controller_pid}"),
    format!("showEvent={show_event}"),
    format!("hideEvent={hide_event}"),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=restored-to-original".to_string(),
    "cursorDisturbance=warp-visible".to_string(),
    "overlayPresentation=visual-only".to_string(),
    "experimental=true".to_string(),
  ]
  .join("\n")
    + "\n";

  let artifact = build_text_artifact(
    "overlay-click-point",
    "txt",
    &format!(
      "overlay-click-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}")),
    ),
    report,
    "Clicked a macOS logical point with an experimental visual overlay cursor.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Overlay-clicked {button_name} at global logical point ({x:.3}, {y:.3}) on display #{} with label \"{label}\".",
      resolution.display.display_id
    ),
    backend: Some("macos.swift.quartz-click+overlay-ffi".to_string()),
    signals: BTreeMap::from([
      (
        "overlayEvent".to_string(),
        format!("{show_event}+{hide_event}"),
      ),
      ("controllerPid".to_string(), controller_pid.to_string()),
      ("cursorDisturbance".to_string(), "warp-visible".to_string()),
      ("experimental".to_string(), "true".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      format!("label={label}"),
      format!("previewMs={preview_ms}"),
      format!("button={button_name}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!("settleMs={settle_ms}"),
      format!("controllerPid={controller_pid}"),
      render_display_note(&resolution.display),
      "cursorAfter=restored-to-original".to_string(),
      "cursorDisturbance=warp-visible".to_string(),
      "overlayPresentation=visual-only".to_string(),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_shutdown(_call: &DriverCall) -> AuvResult<DriverResponse> {
  crate::driver::macos::native::overlay::shutdown()?;
  let controller_pid = std::process::id();

  Ok(DriverResponse {
    summary: "Shut down the experimental AUV overlay daemon in the current process.".to_string(),
    backend: Some("macos.swift.overlay-ffi-poc".to_string()),
    signals: native_overlay_signals(controller_pid, "shutdown"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
    ],
    artifacts: vec![],
  })
}

/// Outcome of an `[with_overlay_cursor]` invocation — surfaces the overlay
/// lifecycle so callers can include it in signals/notes/reports.
pub(crate) struct OverlayWrapperOutcome {
  pub(crate) show_event: String,
  pub(crate) hide_event: String,
  pub(crate) controller_pid: u32,
}

/// Show the overlay cursor at `(x, y)`, run `body`, then hide the overlay —
/// guaranteed even on body failure. Used to wrap a non-cursor-touching action
/// (e.g. AX press) in a visible overlay marker so the user can see where the
/// driver is acting without the real cursor being warped.
pub(crate) fn with_overlay_cursor<R, F>(
  x: f64,
  y: f64,
  label: &str,
  body: F,
) -> AuvResult<(R, OverlayWrapperOutcome)>
where
  F: FnOnce() -> AuvResult<R>,
{
  crate::driver::macos::native::overlay::show_cursor(x, y, label)?;
  let show_event = "shown".to_string();
  let controller_pid = std::process::id();

  let body_result = body();

  let hide_event =
    match crate::driver::macos::native::overlay::hide_cursor().map(|_| "hidden".to_string()) {
      Ok(event) => event,
      Err(_) => "hide_failed".to_string(),
    };

  let outcome = OverlayWrapperOutcome {
    show_event,
    hide_event,
    controller_pid,
  };

  body_result.map(|value| (value, outcome))
}

fn native_overlay_signals(controller_pid: u32, event: &str) -> BTreeMap<String, String> {
  BTreeMap::from([
    ("overlayEvent".to_string(), event.to_string()),
    ("controllerPid".to_string(), controller_pid.to_string()),
    ("visualOnly".to_string(), "true".to_string()),
    ("crossProcessPersistence".to_string(), "false".to_string()),
  ])
}

fn overlay_report<const N: usize>(entries: [(&str, String); N]) -> String {
  entries
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n"
}
