// File: src/driver/macos/overlay/mod.rs
//! Experimental visual-only overlay cursor support.
//!
//! Overlay operations render an AUV cursor (and variants) for preview/replay
//! evidence while actions run. They are visualization tooling for automation
//! process/results and do not change the underlying automation semantics.
//!
//! Boundary: overlays are in-process and best-effort; do not assume cross-run
//! or cross-process persistence.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::*;

const DEFAULT_PREVIEW_MS: u64 = 250;
const DEFAULT_MOVE_MS: u64 = 180;
const DEFAULT_FLASH_MS: u64 = 160;
const MAX_BATCH_OPS: usize = 64;

#[derive(Debug, Deserialize)]
struct OverlayBatchOp {
  #[serde(default, alias = "type", alias = "operation", alias = "kind")]
  op: String,
  #[serde(default)]
  cursor_id: Option<String>,
  #[serde(default)]
  x: Option<f64>,
  #[serde(default)]
  y: Option<f64>,
  #[serde(default)]
  label: Option<String>,
  #[serde(default)]
  variant: Option<String>,
  #[serde(default)]
  duration_ms: Option<u64>,
  #[serde(default)]
  hold_ms: Option<u64>,
}

pub(crate) fn overlay_show_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::show_cursor(x, y, &label)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
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

pub(crate) fn overlay_show_dual_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let user_label =
    optional_non_empty_string(call, "user_label").unwrap_or_else(|| "you".to_string());
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::show_dual_cursor(x, y, &label, &user_label)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "show_dual_cursor".to_string()),
    ("event", "shown".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("auvGlobalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("auvLabel", label.clone()),
    ("userCursorSource", "current-hardware-cursor".to_string()),
    ("userCursorTracking", "polling-30hz".to_string()),
    ("userLabel", user_label.clone()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("windowShape", "two-small-floating-windows".to_string()),
    ("lifecycle", "in-process".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-show-dual-cursor",
    report,
    "Recorded an experimental macOS dual overlay cursor show command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Showed experimental dual overlay cursors: AUV at global logical point ({x:.3}, {y:.3}) and You at the current hardware cursor."
    ),
    backend: Some("macos.swift.overlay-ffi-dual-cursor-poc".to_string()),
    signals: BTreeMap::from([
      ("overlayEvent".to_string(), "shown".to_string()),
      ("controllerPid".to_string(), controller_pid.to_string()),
      ("visualOnly".to_string(), "true".to_string()),
      ("dualCursor".to_string(), "true".to_string()),
      (
        "userCursorSource".to_string(),
        "current-hardware-cursor".to_string(),
      ),
      ("userCursorTracking".to_string(), "polling-30hz".to_string()),
      ("crossProcessPersistence".to_string(), "false".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "dualCursor=true".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "windowShape=two-small-floating-windows".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      "userCursorSource=current-hardware-cursor".to_string(),
      "userCursorTracking=polling-30hz".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("label={label}"),
      format!("userLabel={user_label}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_apply_cursor_batch(call: &DriverCall) -> AuvResult<DriverResponse> {
  let ops_json = optional_non_empty_string(call, "ops_json")
    .or_else(|| optional_non_empty_string(call, "operations_json"))
    .ok_or_else(|| "operation requires --ops_json <json-array>".to_string())?;
  let ops = parse_overlay_batch_ops(&ops_json)?;
  let controller_pid = std::process::id();
  let mut report_lines = vec![
    "operation=apply_cursor_batch".to_string(),
    format!("controllerPid={controller_pid}"),
    format!("opCount={}", ops.len()),
    "coordinateSpace=global-logical".to_string(),
    "visualOnly=true".to_string(),
    "cursorState=id-addressed".to_string(),
    "lifecycle=in-process".to_string(),
    "crossProcessPersistence=false".to_string(),
  ];
  let mut touched_cursor_ids = Vec::new();

  for (index, op) in ops.iter().enumerate() {
    let op_kind = normalized_batch_op(op, index)?;
    match op_kind.as_str() {
      "set" => {
        let cursor_id = batch_cursor_id(op);
        let variant = batch_variant(op, &cursor_id);
        let label = batch_label(op, &cursor_id, &variant);
        let x = required_batch_f64(op.x, index, "x")?;
        let y = required_batch_f64(op.y, index, "y")?;
        auv_overlay_macos::set_cursor(&cursor_id, x, y, &label, &variant)?;
        pump_batch_hold(op)?;
        touched_cursor_ids.push(cursor_id.clone());
        report_lines.push(format!(
          "op[{index}]=set cursorId={cursor_id} variant={variant} point={x:.3},{y:.3} label={label}"
        ));
      }
      "move" => {
        let cursor_id = batch_cursor_id(op);
        let variant = batch_variant(op, &cursor_id);
        let label = batch_label(op, &cursor_id, &variant);
        let x = required_batch_f64(op.x, index, "x")?;
        let y = required_batch_f64(op.y, index, "y")?;
        let duration_ms = op.duration_ms.unwrap_or(DEFAULT_MOVE_MS);
        auv_overlay_macos::move_cursor(&cursor_id, x, y, &label, &variant, duration_ms)?;
        pump_batch_hold(op)?;
        touched_cursor_ids.push(cursor_id.clone());
        report_lines.push(format!(
          "op[{index}]=move cursorId={cursor_id} variant={variant} point={x:.3},{y:.3} label={label} durationMs={duration_ms}"
        ));
      }
      "flash" => {
        let cursor_id = batch_cursor_id(op);
        let label = op
          .label
          .as_deref()
          .map(str::trim)
          .filter(|value| !value.is_empty())
          .unwrap_or("auv · click")
          .to_string();
        let x = required_batch_f64(op.x, index, "x")?;
        let y = required_batch_f64(op.y, index, "y")?;
        let duration_ms = op.duration_ms.unwrap_or(DEFAULT_FLASH_MS);
        auv_overlay_macos::flash_cursor_id(&cursor_id, x, y, &label, duration_ms)?;
        pump_batch_hold(op)?;
        touched_cursor_ids.push(cursor_id.clone());
        report_lines.push(format!(
          "op[{index}]=flash cursorId={cursor_id} point={x:.3},{y:.3} label={label} durationMs={duration_ms}"
        ));
      }
      "hide" => {
        let cursor_id = batch_cursor_id(op);
        auv_overlay_macos::hide_cursor_id(&cursor_id)?;
        pump_batch_hold(op)?;
        touched_cursor_ids.push(cursor_id.clone());
        report_lines.push(format!("op[{index}]=hide cursorId={cursor_id}"));
      }
      "hide_all" => {
        auv_overlay_macos::hide_cursor()?;
        pump_batch_hold(op)?;
        report_lines.push(format!("op[{index}]=hide_all"));
      }
      other => {
        return Err(format!(
          "unsupported overlay batch op[{index}] {other:?}; expected set, move, flash, hide, or hide_all"
        ));
      }
    }
  }

  touched_cursor_ids.sort();
  touched_cursor_ids.dedup();
  report_lines.push(format!("touchedCursorIds={}", touched_cursor_ids.join(",")));
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-apply-cursor-batch",
    report_lines.join("\n") + "\n",
    "Recorded an experimental macOS id-addressed overlay cursor batch command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Applied {} experimental overlay cursor operation(s) in one process.",
      ops.len()
    ),
    backend: Some("macos.swift.overlay-ffi-cursor-state-batch-poc".to_string()),
    signals: BTreeMap::from([
      ("overlayEvent".to_string(), "batch_applied".to_string()),
      ("controllerPid".to_string(), controller_pid.to_string()),
      ("visualOnly".to_string(), "true".to_string()),
      ("cursorState".to_string(), "id-addressed".to_string()),
      ("batch".to_string(), "true".to_string()),
      ("opCount".to_string(), ops.len().to_string()),
      ("touchedCursorIds".to_string(), touched_cursor_ids.join(",")),
      ("crossProcessPersistence".to_string(), "false".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "cursorState=id-addressed".to_string(),
      "batch=true".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("opCount={}", ops.len()),
      format!("touchedCursorIds={}", touched_cursor_ids.join(",")),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_set_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let cursor_id = optional_non_empty_string(call, "cursor_id").unwrap_or_else(|| "auv".to_string());
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let variant = optional_non_empty_string(call, "variant").unwrap_or_else(|| "auv".to_string());
  let label = optional_non_empty_string(call, "label")
    .unwrap_or_else(|| default_cursor_label(&cursor_id, &variant));
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::set_cursor(&cursor_id, x, y, &label, &variant)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "set_cursor".to_string()),
    ("event", "shown".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("cursorId", cursor_id.clone()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("variant", variant.clone()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("lifecycle", "in-process".to_string()),
    ("cursorState", "id-addressed".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    &format!("overlay-set-cursor-{}", sanitize_file_component(&cursor_id)),
    report,
    "Recorded an experimental macOS id-addressed overlay cursor set command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Set experimental overlay cursor {cursor_id} at global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-cursor-state-poc".to_string()),
    signals: cursor_state_signals(controller_pid, "shown", &cursor_id, &variant),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "cursorState=id-addressed".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("cursorId={cursor_id}"),
      format!("label={label}"),
      format!("variant={variant}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_move_cursor_by_id(call: &DriverCall) -> AuvResult<DriverResponse> {
  let cursor_id = optional_non_empty_string(call, "cursor_id").unwrap_or_else(|| "auv".to_string());
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let variant = optional_non_empty_string(call, "variant").unwrap_or_else(|| "auv".to_string());
  let label = optional_non_empty_string(call, "label")
    .unwrap_or_else(|| default_cursor_label(&cursor_id, &variant));
  let duration_ms = optional_positive_u64(call, "duration_ms")?.unwrap_or(DEFAULT_MOVE_MS);
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::move_cursor(&cursor_id, x, y, &label, &variant, duration_ms)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "move_cursor_by_id".to_string()),
    ("event", "moved".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("cursorId", cursor_id.clone()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("variant", variant.clone()),
    ("durationMs", duration_ms.to_string()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("animation", "ease-out-cubic".to_string()),
    ("cursorState", "id-addressed".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    &format!(
      "overlay-move-cursor-{}",
      sanitize_file_component(&cursor_id)
    ),
    report,
    "Recorded an experimental macOS id-addressed overlay cursor move command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Moved experimental overlay cursor {cursor_id} toward global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-cursor-state-poc".to_string()),
    signals: cursor_state_signals(controller_pid, "moved", &cursor_id, &variant),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "cursorState=id-addressed".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "animation=ease-out-cubic".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("cursorId={cursor_id}"),
      format!("label={label}"),
      format!("variant={variant}"),
      format!("durationMs={duration_ms}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_move_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let user_label =
    optional_non_empty_string(call, "user_label").unwrap_or_else(|| "you".to_string());
  let duration_ms = optional_positive_u64(call, "duration_ms")?.unwrap_or(DEFAULT_MOVE_MS);
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::move_dual_cursor(x, y, &label, &user_label, duration_ms)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "move_cursor".to_string()),
    ("event", "moved".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("auvGlobalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("auvLabel", label.clone()),
    ("userCursorSource", "current-hardware-cursor".to_string()),
    ("userCursorTracking", "polling-30hz".to_string()),
    ("userLabel", user_label.clone()),
    ("durationMs", duration_ms.to_string()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("dualCursor", "true".to_string()),
    ("windowShape", "two-small-floating-windows".to_string()),
    ("lifecycle", "in-process".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-move-cursor",
    report,
    "Recorded an experimental macOS dual cursor move animation command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Moved experimental AUV overlay cursor from the current hardware cursor toward global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-dual-cursor-poc".to_string()),
    signals: BTreeMap::from([
      ("overlayEvent".to_string(), "moved".to_string()),
      ("controllerPid".to_string(), controller_pid.to_string()),
      ("visualOnly".to_string(), "true".to_string()),
      ("dualCursor".to_string(), "true".to_string()),
      ("userCursorTracking".to_string(), "polling-30hz".to_string()),
      ("durationMs".to_string(), duration_ms.to_string()),
      ("crossProcessPersistence".to_string(), "false".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "dualCursor=true".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "animation=ease-out-cubic".to_string(),
      "userCursorSource=current-hardware-cursor".to_string(),
      "userCursorTracking=polling-30hz".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("label={label}"),
      format!("userLabel={user_label}"),
      format!("durationMs={duration_ms}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_flash_cursor_by_id(call: &DriverCall) -> AuvResult<DriverResponse> {
  let cursor_id = optional_non_empty_string(call, "cursor_id").unwrap_or_else(|| "auv".to_string());
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label = optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · click".to_string());
  let duration_ms = optional_positive_u64(call, "duration_ms")?.unwrap_or(DEFAULT_FLASH_MS);
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::flash_cursor_id(&cursor_id, x, y, &label, duration_ms)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "flash_cursor_by_id".to_string()),
    ("event", "flashed".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("cursorId", cursor_id.clone()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("durationMs", duration_ms.to_string()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("sprite", "cursor-auv-click".to_string()),
    ("cursorState", "id-addressed".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    &format!(
      "overlay-flash-cursor-{}",
      sanitize_file_component(&cursor_id)
    ),
    report,
    "Recorded an experimental macOS id-addressed overlay cursor flash command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Flashed experimental overlay cursor {cursor_id} at global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-cursor-state-poc".to_string()),
    signals: cursor_state_signals(controller_pid, "flashed", &cursor_id, "auv-click"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "cursorState=id-addressed".to_string(),
      "sprite=cursor-auv-click".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("cursorId={cursor_id}"),
      format!("label={label}"),
      format!("durationMs={duration_ms}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_flash_cursor(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label = optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · click".to_string());
  let duration_ms = optional_positive_u64(call, "duration_ms")?.unwrap_or(DEFAULT_FLASH_MS);
  let hold_ms = optional_positive_u64(call, "hold_ms")?.unwrap_or(0);
  auv_overlay_macos::flash_cursor(x, y, &label, duration_ms)?;
  let controller_pid = std::process::id();

  if hold_ms > 0 {
    auv_overlay_macos::pump_events(hold_ms)?;
  }

  let report = overlay_report([
    ("operation", "flash_cursor".to_string()),
    ("event", "flashed".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("globalLogicalPoint", format!("{x:.3},{y:.3}")),
    ("label", label.clone()),
    ("durationMs", duration_ms.to_string()),
    ("holdMs", hold_ms.to_string()),
    ("coordinateSpace", "global-logical".to_string()),
    ("visualOnly", "true".to_string()),
    ("sprite", "cursor-auv-click".to_string()),
    ("lifecycle", "in-process".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    "overlay-flash-cursor",
    report,
    "Recorded an experimental macOS AUV click-state cursor flash command.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Flashed experimental AUV click cursor at global logical point ({x:.3}, {y:.3})."
    ),
    backend: Some("macos.swift.overlay-ffi-click-sprite-poc".to_string()),
    signals: BTreeMap::from([
      ("overlayEvent".to_string(), "flashed".to_string()),
      ("controllerPid".to_string(), controller_pid.to_string()),
      ("visualOnly".to_string(), "true".to_string()),
      ("sprite".to_string(), "cursor-auv-click".to_string()),
      ("durationMs".to_string(), duration_ms.to_string()),
      ("crossProcessPersistence".to_string(), "false".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "sprite=cursor-auv-click".to_string(),
      "coordinateSpace=global-logical".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("label={label}"),
      format!("durationMs={duration_ms}"),
      format!("holdMs={hold_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_hide_cursor_id(call: &DriverCall) -> AuvResult<DriverResponse> {
  let cursor_id = optional_non_empty_string(call, "cursor_id").unwrap_or_else(|| "auv".to_string());
  auv_overlay_macos::hide_cursor_id(&cursor_id)?;
  let controller_pid = std::process::id();
  let report = overlay_report([
    ("operation", "hide_cursor_id".to_string()),
    ("event", "hidden".to_string()),
    ("controllerPid", controller_pid.to_string()),
    ("cursorId", cursor_id.clone()),
    ("lifecycle", "in-process".to_string()),
    ("cursorState", "id-addressed".to_string()),
    ("crossProcessPersistence", "false".to_string()),
  ]);
  let artifact = build_text_artifact(
    "overlay-cursor",
    "txt",
    &format!(
      "overlay-hide-cursor-{}",
      sanitize_file_component(&cursor_id)
    ),
    report,
    "Recorded an experimental macOS id-addressed overlay cursor hide command.",
  )?;

  Ok(DriverResponse {
    summary: format!("Hid experimental overlay cursor {cursor_id}."),
    backend: Some("macos.swift.overlay-ffi-cursor-state-poc".to_string()),
    signals: cursor_state_signals(controller_pid, "hidden", &cursor_id, "unknown"),
    notes: vec![
      "experimental=true".to_string(),
      "visualOnly=true".to_string(),
      "cursorState=id-addressed".to_string(),
      "lifecycle=in-process".to_string(),
      "crossProcessPersistence=false".to_string(),
      format!("controllerPid={controller_pid}"),
      format!("cursorId={cursor_id}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_hide_cursor(_call: &DriverCall) -> AuvResult<DriverResponse> {
  auv_overlay_macos::hide_cursor()?;
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

/// Wrapped click: shows overlay cursor, clicks, then hides overlay.
///
/// Does NOT modify `debug.clickPoint` behavior. Flicker acceptability must be
/// confirmed by manual observation before this is used on broader command paths.
pub(crate) fn overlay_click_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let move_ms = optional_positive_u64(call, "move_ms")?.unwrap_or(DEFAULT_MOVE_MS);
  let preview_ms = optional_positive_u64(call, "preview_ms")?.unwrap_or(DEFAULT_PREVIEW_MS);
  let flash_ms = optional_positive_u64(call, "flash_ms")?.unwrap_or(DEFAULT_FLASH_MS);
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

  // 1. Animate AUV overlay cursor from the current hardware cursor toward the target.
  auv_overlay_macos::move_dual_cursor(x, y, &label, "you", move_ms)?;
  let controller_pid = std::process::id();
  let show_event = "moved".to_string();

  // 2. Hold overlay for preview visibility before the click.
  if preview_ms > 0 {
    auv_overlay_macos::pump_events(preview_ms)?;
  }

  // 3. Click. Native pointer bridge handles warp-to-target + CGEvent + warp-restore internally.
  let click_result = auv_driver_macos::native::pointer::click_point(
    x,
    y,
    button_code,
    click_count,
    click_interval_ms,
  );

  let flash_result = auv_overlay_macos::flash_cursor(x, y, &label, flash_ms);

  if settle_ms > 0 {
    auv_overlay_macos::pump_events(settle_ms)?;
  }

  // 4. Hide overlay cursor regardless of click success.
  let hide_event = match auv_overlay_macos::hide_cursor().map(|_| "hidden".to_string()) {
    Ok(event) => event,
    Err(_) => "hide_failed".to_string(),
  };

  // Propagate click errors after overlay cleanup.
  click_result?;
  flash_result?;

  let report = [
    "operation=overlay_click_point".to_string(),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!("label={label}"),
    format!("moveMs={move_ms}"),
    format!("previewMs={preview_ms}"),
    format!("flashMs={flash_ms}"),
    format!("button={button_name}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
    format!("controllerPid={controller_pid}"),
    format!("showEvent={show_event}"),
    "flashEvent=flashed".to_string(),
    format!("hideEvent={hide_event}"),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=restored-to-original".to_string(),
    "cursorDisturbance=warp-visible".to_string(),
    "overlayPresentation=dual-cursor-visual-only".to_string(),
    "userCursorSource=current-hardware-cursor".to_string(),
    "userCursorTracking=polling-30hz".to_string(),
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
      ("dualCursor".to_string(), "true".to_string()),
      ("userCursorTracking".to_string(), "polling-30hz".to_string()),
      ("moveMs".to_string(), move_ms.to_string()),
      ("flashMs".to_string(), flash_ms.to_string()),
      ("experimental".to_string(), "true".to_string()),
    ]),
    notes: vec![
      "experimental=true".to_string(),
      format!("label={label}"),
      format!("moveMs={move_ms}"),
      format!("previewMs={preview_ms}"),
      format!("flashMs={flash_ms}"),
      format!("button={button_name}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!("settleMs={settle_ms}"),
      format!("controllerPid={controller_pid}"),
      render_display_note(&resolution.display),
      "cursorAfter=restored-to-original".to_string(),
      "cursorDisturbance=warp-visible".to_string(),
      "overlayPresentation=dual-cursor-visual-only".to_string(),
      "userCursorSource=current-hardware-cursor".to_string(),
      "userCursorTracking=polling-30hz".to_string(),
    ],
    artifacts: vec![artifact],
  })
}

pub(crate) fn overlay_shutdown(_call: &DriverCall) -> AuvResult<DriverResponse> {
  auv_overlay_macos::shutdown()?;
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
  auv_overlay_macos::move_dual_cursor(x, y, label, "you", DEFAULT_MOVE_MS)?;
  let show_event = "moved".to_string();
  let controller_pid = std::process::id();

  let body_result = body();

  let _ = auv_overlay_macos::flash_cursor(x, y, label, DEFAULT_FLASH_MS);

  let hide_event = match auv_overlay_macos::hide_cursor().map(|_| "hidden".to_string()) {
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

fn cursor_state_signals(
  controller_pid: u32,
  event: &str,
  cursor_id: &str,
  variant: &str,
) -> BTreeMap<String, String> {
  BTreeMap::from([
    ("overlayEvent".to_string(), event.to_string()),
    ("controllerPid".to_string(), controller_pid.to_string()),
    ("visualOnly".to_string(), "true".to_string()),
    ("cursorState".to_string(), "id-addressed".to_string()),
    ("cursorId".to_string(), cursor_id.to_string()),
    ("variant".to_string(), variant.to_string()),
    ("crossProcessPersistence".to_string(), "false".to_string()),
  ])
}

fn default_cursor_label(cursor_id: &str, variant: &str) -> String {
  match variant {
    "you" | "user" | "human" => "you".to_string(),
    "auv-click" | "auv_click" | "click" | "auvClick" => "auv · click".to_string(),
    _ if cursor_id == "you" => "you".to_string(),
    _ => "auv · replay".to_string(),
  }
}

fn parse_overlay_batch_ops(raw: &str) -> AuvResult<Vec<OverlayBatchOp>> {
  let ops: Vec<OverlayBatchOp> = serde_json::from_str(raw)
    .map_err(|error| format!("failed to parse --ops_json as overlay cursor batch: {error}"))?;
  if ops.is_empty() {
    return Err("overlay cursor batch requires at least one operation".to_string());
  }
  if ops.len() > MAX_BATCH_OPS {
    return Err(format!(
      "overlay cursor batch accepts at most {MAX_BATCH_OPS} operations; got {}",
      ops.len()
    ));
  }
  Ok(ops)
}

fn normalized_batch_op(op: &OverlayBatchOp, index: usize) -> AuvResult<String> {
  let kind = op.op.trim().to_ascii_lowercase().replace('-', "_");
  if kind.is_empty() {
    return Err(format!(
      "overlay batch op[{index}] requires an op/type/operation field"
    ));
  }
  Ok(match kind.as_str() {
    "set_cursor" => "set".to_string(),
    "move_cursor" | "move_cursor_by_id" => "move".to_string(),
    "flash_cursor" | "flash_cursor_by_id" => "flash".to_string(),
    "hide_cursor" | "hide_cursor_id" => "hide".to_string(),
    "hide_all" | "hide_cursors" | "hide_all_cursors" => "hide_all".to_string(),
    _ => kind,
  })
}

fn batch_cursor_id(op: &OverlayBatchOp) -> String {
  op.cursor_id
    .as_deref()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .unwrap_or("auv")
    .to_string()
}

fn batch_variant(op: &OverlayBatchOp, cursor_id: &str) -> String {
  op.variant
    .as_deref()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .unwrap_or(if cursor_id == "you" { "you" } else { "auv" })
    .to_string()
}

fn batch_label(op: &OverlayBatchOp, cursor_id: &str, variant: &str) -> String {
  op.label
    .as_deref()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToString::to_string)
    .unwrap_or_else(|| default_cursor_label(cursor_id, variant))
}

fn required_batch_f64(value: Option<f64>, index: usize, field: &str) -> AuvResult<f64> {
  let value = value.ok_or_else(|| format!("overlay batch op[{index}] requires field {field:?}"))?;
  if !value.is_finite() {
    return Err(format!(
      "overlay batch op[{index}] field {field:?} must be finite"
    ));
  }
  Ok(value)
}

fn pump_batch_hold(op: &OverlayBatchOp) -> AuvResult<()> {
  if let Some(hold_ms) = op.hold_ms
    && hold_ms > 0
  {
    auv_overlay_macos::pump_events(hold_ms)?;
  }
  Ok(())
}

fn overlay_report<const N: usize>(entries: [(&str, String); N]) -> String {
  entries
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect::<Vec<_>>()
    .join("\n")
    + "\n"
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_overlay_batch_ops_accepts_operation_aliases() {
    let ops = parse_overlay_batch_ops(
      r#"[
        {"type":"set","cursor_id":"you","x":10,"y":20,"variant":"you"},
        {"operation":"move-cursor","cursor_id":"agent-1","x":30,"y":40,"duration_ms":5},
        {"kind":"flash_cursor_by_id","cursor_id":"agent-1","x":30,"y":40},
        {"op":"hide_all"}
      ]"#,
    )
    .expect("batch should parse");

    assert_eq!(ops.len(), 4);
    assert_eq!(normalized_batch_op(&ops[0], 0).unwrap(), "set");
    assert_eq!(normalized_batch_op(&ops[1], 1).unwrap(), "move");
    assert_eq!(normalized_batch_op(&ops[2], 2).unwrap(), "flash");
    assert_eq!(normalized_batch_op(&ops[3], 3).unwrap(), "hide_all");
  }

  #[test]
  fn parse_overlay_batch_ops_rejects_empty_batches() {
    let error = parse_overlay_batch_ops("[]").expect_err("empty batch should fail");
    assert!(error.contains("at least one operation"));
  }

  #[test]
  fn batch_defaults_use_cursor_id_and_variant() {
    let op = OverlayBatchOp {
      op: "set".to_string(),
      cursor_id: Some("you".to_string()),
      x: Some(1.0),
      y: Some(2.0),
      label: None,
      variant: None,
      duration_ms: None,
      hold_ms: None,
    };
    let cursor_id = batch_cursor_id(&op);
    let variant = batch_variant(&op, &cursor_id);
    let label = batch_label(&op, &cursor_id, &variant);

    assert_eq!(cursor_id, "you");
    assert_eq!(variant, "you");
    assert_eq!(label, "you");
  }
}
