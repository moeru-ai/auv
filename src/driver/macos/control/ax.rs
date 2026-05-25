// File: src/driver/macos/control/ax.rs
//! AX-tree driven actions for the macOS driver.
//!
//! Implements action commands that locate targets via an observed AX snapshot
//! and then interact via pointer clicks or AX actions (with optional overlay
//! evidence).
//!
//! Boundary: this is "query AX -> act now" glue. It does not introduce a
//! generic retained UI node runtime; any references are re-resolved per action.

use super::super::overlay::with_overlay_cursor;
use super::super::*;
use super::common::{
  DEFAULT_CLICK_INTERVAL_MS, activate_app_if_needed, build_ax_click_notes,
  send_reveal_shortcut_if_needed,
};
use super::window_ocr::{
  capture_resolved_window_observation, click_window_text, logical_point_for_match,
};
use std::fs;

pub(crate) fn focus_text_input(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);

  activate_app_if_needed(&app)?;
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let snapshot =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "text input-like"))?;
  let (center_x, center_y) = ax_node_center(matched);
  crate::driver::macos::native::pointer::click_point(
    center_x,
    center_y,
    0,
    1,
    DEFAULT_CLICK_INTERVAL_MS,
  )?;

  let report = render_ax_interaction_report("focus-text-input", &snapshot, matched, &query);
  let artifact = build_text_artifact(
    "focus-text-input",
    "txt",
    &format!("focus-text-input-{}", sanitize_file_component(&query)),
    report,
    "Focused a text input by matching the observed AX tree and clicking the resolved bounds.",
  )?;
  let mut notes = build_ax_click_notes(&query, matched, center_x, center_y);
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.placeholder.is_empty() {
    notes.push(format!("matchedPlaceholder={}", matched.placeholder));
  }

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Focused text input in {} using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Focused text input {} in {} using query {}.",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some("macos.desktop.ax-tree-click-focus".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}

pub(crate) fn ax_focus_text_input(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  let activate = optional_bool(call, "activate")?.unwrap_or(true);
  let overlay = optional_bool(call, "overlay")?.unwrap_or(false);
  let overlay_label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let preview_ms =
    optional_positive_u64(call, "preview_ms")?.unwrap_or(if overlay { 250 } else { 0 });
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);

  if activate {
    activate_app_if_needed(&app)?;
  }
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let capture =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?;
  let snapshot = &capture.snapshot;
  if capture.pid <= 0 {
    return Err(format!(
      "native AX tree capture did not return a valid pid for app {:?} (got {}); cannot dispatch AX focus",
      snapshot.app_name, capture.pid
    ));
  }
  let matched = find_best_ax_node(snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(snapshot, &query, "text input-like"))?;
  let (center_x, center_y) = ax_node_center(matched);

  let (focus_result, overlay_outcome) = if overlay {
    let (result, outcome) = with_overlay_cursor(center_x, center_y, &overlay_label, || {
      if preview_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(preview_ms)?;
      }
      let result = crate::driver::macos::native::ax_tree::set_ax_focused_path(
        capture.pid as i32,
        &matched.path,
        &matched.role,
      )?;
      if settle_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(settle_ms)?;
      }
      Ok(result)
    })?;
    (result, Some(outcome))
  } else {
    (
      crate::driver::macos::native::ax_tree::set_ax_focused_path(
        capture.pid as i32,
        &matched.path,
        &matched.role,
      )?,
      None,
    )
  };

  let report = render_ax_interaction_report("ax-focus-text-input", snapshot, matched, &query);
  let mut report = format!(
    "{report}setAttribute={set_attribute}\nwasAlreadyFocused={was_already_focused}\nfocusMechanism=ax-attribute\ncursorDisturbance=none\nactivatedApp={activate}\noverlayPresentation={}\n",
    if overlay {
      "dual-cursor-visual-only"
    } else {
      "off"
    },
    set_attribute = focus_result.set_attribute,
    was_already_focused = focus_result.was_already_focused,
  );
  if let Some(outcome) = &overlay_outcome {
    report.push_str("userCursorSource=current-hardware-cursor\n");
    report.push_str("userCursorTracking=polling-30hz\n");
    report.push_str(&format!("overlayShowEvent={}\n", outcome.show_event));
    report.push_str(&format!("overlayHideEvent={}\n", outcome.hide_event));
    report.push_str(&format!("controllerPid={}\n", outcome.controller_pid));
    report.push_str(&format!("previewMs={preview_ms}\n"));
    report.push_str(&format!("settleMs={settle_ms}\n"));
    report.push_str(&format!("overlayLabel={overlay_label}\n"));
  }
  let artifact = build_text_artifact(
    "ax-focus-text-input",
    "txt",
    &format!("ax-focus-text-input-{}", sanitize_file_component(&query)),
    report,
    "Focused a text input via AXUIElementSetAttributeValue(kAXFocusedAttribute); the real cursor is not moved.",
  )?;
  let capture_label = format!("ax-focus-text-input-{}", sanitize_file_component(&query));
  let (mut overlay_artifacts, overlay_capture_note) = best_effort_ax_overlay_artifacts(
    call,
    &capture_label,
    "ax-focus-text-input",
    &query,
    "ax-attribute",
    "ax-attribute",
    "none",
    overlay.then_some("dual-cursor-visual-only"),
    center_x,
    center_y,
    overlay,
    "auv",
    matched,
  );

  let mut notes = build_ax_click_notes(&query, matched, center_x, center_y);
  notes.push("focusMechanism=ax-attribute".to_string());
  notes.push("cursorDisturbance=none".to_string());
  notes.push(format!("setAttribute={}", focus_result.set_attribute));
  notes.push(format!(
    "wasAlreadyFocused={}",
    focus_result.was_already_focused
  ));
  notes.push(format!("activatedApp={activate}"));
  if let Some(outcome) = &overlay_outcome {
    notes.push("overlayPresentation=dual-cursor-visual-only".to_string());
    notes.push("userCursorSource=current-hardware-cursor".to_string());
    notes.push("userCursorTracking=polling-30hz".to_string());
    notes.push(format!("overlayShowEvent={}", outcome.show_event));
    notes.push(format!("overlayHideEvent={}", outcome.hide_event));
    notes.push(format!("controllerPid={}", outcome.controller_pid));
    notes.push(format!("previewMs={preview_ms}"));
    notes.push(format!("settleMs={settle_ms}"));
    notes.push(format!("overlayLabel={overlay_label}"));
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.placeholder.is_empty() {
    notes.push(format!("matchedPlaceholder={}", matched.placeholder));
  }
  if let Some(note) = overlay_capture_note {
    notes.push(note);
  }

  let mut signals = std::collections::BTreeMap::new();
  signals.insert("focusMechanism".to_string(), "ax-attribute".to_string());
  signals.insert("cursorDisturbance".to_string(), "none".to_string());
  signals.insert(
    "setAttribute".to_string(),
    focus_result.set_attribute.clone(),
  );
  signals.insert(
    "wasAlreadyFocused".to_string(),
    focus_result.was_already_focused.to_string(),
  );
  if let Some(outcome) = &overlay_outcome {
    signals.insert(
      "overlayEvent".to_string(),
      format!("{}+{}", outcome.show_event, outcome.hide_event),
    );
    signals.insert(
      "controllerPid".to_string(),
      outcome.controller_pid.to_string(),
    );
    signals.insert(
      "overlayPresentation".to_string(),
      "dual-cursor-visual-only".to_string(),
    );
    signals.insert("dualCursor".to_string(), "true".to_string());
    signals.insert("userCursorTracking".to_string(), "polling-30hz".to_string());
  }

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Focused text input in {} via AXUIElementSetAttributeValue(kAXFocusedAttribute) using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Focused {} in {} via AXUIElementSetAttributeValue(kAXFocusedAttribute) using query {} (no cursor warp).",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some(
      if overlay {
        "macos.ax.set-focused+overlay-ffi"
      } else {
        "macos.ax.set-focused"
      }
      .to_string(),
    ),
    signals,
    notes,
    artifacts: {
      let mut artifacts = vec![artifact];
      artifacts.append(&mut overlay_artifacts);
      artifacts
    },
  })
}

pub(crate) fn ax_press_button(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  let activate = optional_bool(call, "activate")?.unwrap_or(true);
  let action_name =
    optional_non_empty_string(call, "action").unwrap_or_else(|| "AXPress".to_string());
  let overlay = optional_bool(call, "overlay")?.unwrap_or(false);
  let overlay_label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let preview_ms =
    optional_positive_u64(call, "preview_ms")?.unwrap_or(if overlay { 250 } else { 0 });
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);

  if activate {
    activate_app_if_needed(&app)?;
  }
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let snapshot =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  if snapshot.pid <= 0 {
    return Err(format!(
      "native AX tree capture did not return a valid pid for app {:?} (got {}); cannot dispatch AX action",
      snapshot.app_name, snapshot.pid
    ));
  }
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "AX-pressable"))?;
  let (center_x, center_y) = ax_node_center(matched);

  let (press_action, overlay_outcome) = if overlay {
    let (action, outcome) = with_overlay_cursor(center_x, center_y, &overlay_label, || {
      if preview_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(preview_ms)?;
      }
      let action = crate::driver::macos::native::ax_tree::perform_ax_path_action(
        snapshot.pid,
        &matched.path,
        &matched.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(settle_ms)?;
      }
      Ok(action)
    })?;
    (action, Some(outcome))
  } else {
    (
      crate::driver::macos::native::ax_tree::perform_ax_path_action(
        snapshot.pid,
        &matched.path,
        &matched.role,
        &action_name,
      )?,
      None,
    )
  };
  let performed_action = press_action.performed_action;
  let available_actions = press_action.available_actions;

  let report = render_ax_interaction_report("ax-press-button", &snapshot, matched, &query);
  let mut report = format!(
    "{report}performedAction={performed_action}\navailableActions={available_actions}\npressMechanism=ax-action\ncursorDisturbance=none\nactivatedApp={activate}\noverlayPresentation={}\n",
    if overlay {
      "dual-cursor-visual-only"
    } else {
      "off"
    },
  );
  if let Some(outcome) = &overlay_outcome {
    report.push_str("userCursorSource=current-hardware-cursor\n");
    report.push_str("userCursorTracking=polling-30hz\n");
    report.push_str(&format!("overlayShowEvent={}\n", outcome.show_event));
    report.push_str(&format!("overlayHideEvent={}\n", outcome.hide_event));
    report.push_str(&format!("controllerPid={}\n", outcome.controller_pid));
    report.push_str(&format!("previewMs={preview_ms}\n"));
    report.push_str(&format!("settleMs={settle_ms}\n"));
    report.push_str(&format!("overlayLabel={overlay_label}\n"));
  }
  let artifact = build_text_artifact(
    "ax-press-button",
    "txt",
    &format!("ax-press-button-{}", sanitize_file_component(&query)),
    report,
    "Pressed a control via AXUIElementPerformAction; the real cursor is not moved.",
  )?;
  let capture_label = format!("ax-press-button-{}", sanitize_file_component(&query));
  let (mut overlay_artifacts, overlay_capture_note) = best_effort_ax_overlay_artifacts(
    call,
    &capture_label,
    "ax-press-button",
    &query,
    "ax-action",
    "ax-action",
    "none",
    overlay.then_some("dual-cursor-visual-only"),
    center_x,
    center_y,
    overlay,
    "auv",
    matched,
  );

  let mut notes = build_ax_click_notes(&query, matched, center_x, center_y);
  notes.push("pressMechanism=ax-action".to_string());
  notes.push("cursorDisturbance=none".to_string());
  notes.push(format!("performedAction={performed_action}"));
  if !available_actions.is_empty() {
    notes.push(format!("availableActions={available_actions}"));
  }
  notes.push(format!("activatedApp={activate}"));
  if let Some(outcome) = &overlay_outcome {
    notes.push("overlayPresentation=dual-cursor-visual-only".to_string());
    notes.push("userCursorSource=current-hardware-cursor".to_string());
    notes.push("userCursorTracking=polling-30hz".to_string());
    notes.push(format!("overlayShowEvent={}", outcome.show_event));
    notes.push(format!("overlayHideEvent={}", outcome.hide_event));
    notes.push(format!("controllerPid={}", outcome.controller_pid));
    notes.push(format!("previewMs={preview_ms}"));
    notes.push(format!("settleMs={settle_ms}"));
    notes.push(format!("overlayLabel={overlay_label}"));
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.help.is_empty() {
    notes.push(format!("matchedHelp={}", matched.help));
  }
  if let Some(note) = overlay_capture_note {
    notes.push(note);
  }

  let mut signals = std::collections::BTreeMap::new();
  signals.insert("pressMechanism".to_string(), "ax-action".to_string());
  signals.insert("cursorDisturbance".to_string(), "none".to_string());
  signals.insert("performedAction".to_string(), performed_action.clone());
  if !available_actions.is_empty() {
    signals.insert("availableActions".to_string(), available_actions);
  }
  if let Some(outcome) = &overlay_outcome {
    signals.insert(
      "overlayEvent".to_string(),
      format!("{}+{}", outcome.show_event, outcome.hide_event),
    );
    signals.insert(
      "controllerPid".to_string(),
      outcome.controller_pid.to_string(),
    );
    signals.insert(
      "overlayPresentation".to_string(),
      "dual-cursor-visual-only".to_string(),
    );
    signals.insert("dualCursor".to_string(), "true".to_string());
    signals.insert("userCursorTracking".to_string(), "polling-30hz".to_string());
  }

  let backend = if overlay {
    "macos.ax.perform-action+overlay-ffi"
  } else {
    "macos.ax.perform-action"
  };

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Pressed button-like control in {} via AXUIElementPerformAction using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Pressed {} in {} via AXUIElementPerformAction using query {}.",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some(backend.to_string()),
    signals,
    notes,
    artifacts: {
      let mut artifacts = vec![artifact];
      artifacts.append(&mut overlay_artifacts);
      artifacts
    },
  })
}

pub(crate) fn press_button(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);

  activate_app_if_needed(&app)?;
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let snapshot =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "button-like"))?;
  let (center_x, center_y) = ax_node_center(matched);
  crate::driver::macos::native::pointer::click_point(
    center_x,
    center_y,
    0,
    1,
    DEFAULT_CLICK_INTERVAL_MS,
  )?;

  let report = render_ax_interaction_report("press-button", &snapshot, matched, &query);
  let artifact = build_text_artifact(
    "press-button",
    "txt",
    &format!("press-button-{}", sanitize_file_component(&query)),
    report,
    "Pressed a known control by matching the observed AX tree and clicking the resolved bounds.",
  )?;
  let mut notes = build_ax_click_notes(&query, matched, center_x, center_y);
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.help.is_empty() {
    notes.push(format!("matchedHelp={}", matched.help));
  }

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Pressed button-like control in {} using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Pressed {} in {} using query {}.",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some("macos.desktop.ax-tree-click-press".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}

pub(crate) fn ax_click_window_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let match_index = optional_i64(call, "match_index")?.unwrap_or(0).max(0) as usize;
  let anchor_offset_x = optional_f64(call, "anchor_offset_x")?.unwrap_or(0.0);
  let anchor_offset_y = optional_f64(call, "anchor_offset_y")?.unwrap_or(0.0);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  let action_name =
    optional_non_empty_string(call, "action").unwrap_or_else(|| "AXPress".to_string());
  let overlay = optional_bool(call, "overlay")?.unwrap_or(false);
  let overlay_label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let preview_ms =
    optional_positive_u64(call, "preview_ms")?.unwrap_or(if overlay { 250 } else { 0 });
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);

  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| "operation requires --target <application-id>".to_string())?;

  // 1. OCR locate the text inside the target window.
  let capture_label = format!("ax-click-window-text-{}", sanitize_file_component(&query));
  let capture = capture_resolved_window_observation(call, &capture_label)?;
  let (ocr_snapshot, filtered, ocr_report, _command_report) =
    run_text_match_on_capture(call, &capture, &query)?;
  let matched_ocr = filtered.get(match_index).ok_or_else(|| {
    format!(
      "no OCR text match at index {match_index} for query {query} inside resolved window; observed {} match(es). Inspect `debug.findWindowText`",
      filtered.len()
    )
  })?;
  let (logical_x_base, logical_y_base) = logical_point_for_match(&capture, matched_ocr)?;
  let logical_x = logical_x_base + anchor_offset_x;
  let logical_y = logical_y_base + anchor_offset_y;

  // 2. Observe AX tree and resolve the pressable node under the OCR anchor.
  // Activation already happened inside capture_resolved_window_observation.
  let ax_capture =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?;
  let ax_snapshot = &ax_capture.snapshot;
  if ax_capture.pid <= 0 {
    return Err(format!(
      "native AX tree capture did not return a valid pid for app {:?} (got {}); cannot dispatch AX action",
      ax_snapshot.app_name, ax_capture.pid
    ));
  }
  let ax_node = find_ax_node_at_point(ax_snapshot, logical_x, logical_y).ok_or_else(|| {
    format!(
      "no AX node found at OCR anchor ({logical_x:.3}, {logical_y:.3}) for text {:?}; the visible text may be canvas-rendered or outside the observed window's AX subtree. Try debug.clickWindowText if you accept cursor warp.",
      matched_ocr.text
    )
  })?;
  let (center_x, center_y) = ax_node_center(ax_node);

  // 3. AX press (optionally wrapped by the overlay marker for visual feedback).
  let (press_action, overlay_outcome) = if overlay {
    let (action, outcome) = with_overlay_cursor(center_x, center_y, &overlay_label, || {
      if preview_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(preview_ms)?;
      }
      let action = crate::driver::macos::native::ax_tree::perform_ax_path_action(
        ax_capture.pid as i32,
        &ax_node.path,
        &ax_node.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        crate::driver::macos::native::overlay::pump_events(settle_ms)?;
      }
      Ok(action)
    })?;
    (action, Some(outcome))
  } else {
    (
      crate::driver::macos::native::ax_tree::perform_ax_path_action(
        ax_capture.pid as i32,
        &ax_node.path,
        &ax_node.role,
        &action_name,
      )?,
      None,
    )
  };
  let performed_action = press_action.performed_action;
  let available_actions = press_action.available_actions;

  // 4. Compose the artifact report (OCR side + AX side + press side).
  let mut report =
    render_ax_interaction_report("ax-click-window-text", &ax_snapshot, ax_node, &query);
  report.push_str(&format!("ocrMatchText={}\n", matched_ocr.text));
  report.push_str(&format!(
    "ocrMatchBounds={}\n",
    render_rect_compact(&matched_ocr.bounds)
  ));
  report.push_str(&format!(
    "ocrMatchConfidence={:.3}\n",
    matched_ocr.confidence
  ));
  report.push_str(&format!("ocrMatchIndex={match_index}\n"));
  report.push_str(&format!("ocrFilteredCount={}\n", filtered.len()));
  report.push_str(&format!(
    "ocrSnapshotMatches={}\n",
    ocr_snapshot.matches.len()
  ));
  report.push_str(&format!(
    "ocrAnchorLogicalPoint={logical_x_base:.3},{logical_y_base:.3}\n"
  ));
  report.push_str(&format!(
    "anchorOffset={anchor_offset_x:.3},{anchor_offset_y:.3}\n"
  ));
  report.push_str(&format!(
    "axResolvedLogicalPoint={logical_x:.3},{logical_y:.3}\n"
  ));
  report.push_str(&format!("axNodeCenter={center_x:.3},{center_y:.3}\n"));
  report.push_str(&format!("performedAction={performed_action}\n"));
  report.push_str(&format!("availableActions={available_actions}\n"));
  report.push_str("pressMechanism=ax-action\n");
  report.push_str("cursorDisturbance=none\n");
  report.push_str(&format!(
    "overlayPresentation={}\n",
    if overlay {
      "dual-cursor-visual-only"
    } else {
      "off"
    }
  ));
  if let Some(outcome) = &overlay_outcome {
    report.push_str("userCursorSource=current-hardware-cursor\n");
    report.push_str("userCursorTracking=polling-30hz\n");
    report.push_str(&format!("overlayShowEvent={}\n", outcome.show_event));
    report.push_str(&format!("overlayHideEvent={}\n", outcome.hide_event));
    report.push_str(&format!("controllerPid={}\n", outcome.controller_pid));
    report.push_str(&format!("previewMs={preview_ms}\n"));
    report.push_str(&format!("settleMs={settle_ms}\n"));
    report.push_str(&format!("overlayLabel={overlay_label}\n"));
  }

  let report_artifact = build_text_artifact(
    "ax-click-window-text",
    "txt",
    &format!("ax-click-window-text-{}", sanitize_file_component(&query)),
    report,
    "Located text via Vision OCR, resolved to the AX node at that point, and pressed it via AXUIElementPerformAction without warping the real cursor.",
  )?;
  let ocr_artifact = build_text_artifact(
    "ax-click-window-text",
    "txt",
    &format!(
      "ax-click-window-text-ocr-{}",
      sanitize_file_component(&query)
    ),
    ocr_report,
    "Vision OCR text-anchor report consumed by debug.axClickWindowText.",
  )?;
  let screenshot_artifact = screenshot_artifact(&capture, &capture_label, "ax click window text");
  let mut overlay_artifacts = build_overlay_evidence_artifacts(OverlayEvidenceRequest {
    kind: "ax-click-window-text",
    label: capture_label.clone(),
    screenshot_path: screenshot_artifact.source_path.clone(),
    screenshot_dimensions: capture.dimensions.clone(),
    capture_contract: capture.capture_contract.clone(),
    query: Some(query.clone()),
    strategy: Some("ax-action".to_string()),
    fallback_used: Some(false),
    cursor_disturbance: Some("none".to_string()),
    press_mechanism: Some("ax-action".to_string()),
    overlay_presentation: overlay.then_some("dual-cursor-visual-only".to_string()),
    action_point: logical_to_capture_pixel(&capture.capture_contract, center_x, center_y),
    expected_target: Some(logical_to_capture_pixel(
      &capture.capture_contract,
      logical_x,
      logical_y,
    )),
    ocr_match: Some(OverlayEvidenceMatch {
      text: matched_ocr.text.clone(),
      confidence: matched_ocr.confidence,
      bounds: matched_ocr.bounds.clone(),
    }),
    row: None,
    ax_target: Some(OverlayEvidenceAxTarget {
      role: ax_node.role.clone(),
      label: if ax_node.title.is_empty() {
        query.clone()
      } else {
        ax_node.title.clone()
      },
      bounds: ax_node.bounds.clone(),
    }),
    decision: None,
    include_user_cursor: overlay,
    auv_cursor_variant: "auv",
  })?;

  // 5. Notes + signals.
  let mut notes = build_ax_click_notes(&query, ax_node, center_x, center_y);
  notes.push(format!("ocrMatchText={}", matched_ocr.text));
  notes.push(format!(
    "ocrMatchBounds={}",
    render_rect_compact(&matched_ocr.bounds)
  ));
  notes.push(format!("ocrMatchConfidence={:.3}", matched_ocr.confidence));
  notes.push(format!(
    "ocrAnchorLogicalPoint={logical_x_base:.3},{logical_y_base:.3}"
  ));
  notes.push(format!(
    "anchorOffset={anchor_offset_x:.3},{anchor_offset_y:.3}"
  ));
  notes.push("pressMechanism=ax-action".to_string());
  notes.push("cursorDisturbance=none".to_string());
  notes.push(format!("performedAction={performed_action}"));
  if !available_actions.is_empty() {
    notes.push(format!("availableActions={available_actions}"));
  }
  if let Some(outcome) = &overlay_outcome {
    notes.push("overlayPresentation=dual-cursor-visual-only".to_string());
    notes.push("userCursorSource=current-hardware-cursor".to_string());
    notes.push("userCursorTracking=polling-30hz".to_string());
    notes.push(format!("overlayShowEvent={}", outcome.show_event));
    notes.push(format!("overlayHideEvent={}", outcome.hide_event));
    notes.push(format!("controllerPid={}", outcome.controller_pid));
    notes.push(format!("previewMs={preview_ms}"));
    notes.push(format!("settleMs={settle_ms}"));
    notes.push(format!("overlayLabel={overlay_label}"));
  }

  let mut signals = std::collections::BTreeMap::new();
  signals.insert("pressMechanism".to_string(), "ax-action".to_string());
  signals.insert("cursorDisturbance".to_string(), "none".to_string());
  signals.insert("performedAction".to_string(), performed_action);
  if !available_actions.is_empty() {
    signals.insert("availableActions".to_string(), available_actions);
  }
  signals.insert("click.resolved_text".to_string(), matched_ocr.text.clone());
  if let Some(outcome) = &overlay_outcome {
    signals.insert(
      "overlayEvent".to_string(),
      format!("{}+{}", outcome.show_event, outcome.hide_event),
    );
    signals.insert(
      "controllerPid".to_string(),
      outcome.controller_pid.to_string(),
    );
    signals.insert(
      "overlayPresentation".to_string(),
      "dual-cursor-visual-only".to_string(),
    );
    signals.insert("dualCursor".to_string(), "true".to_string());
    signals.insert("userCursorTracking".to_string(), "polling-30hz".to_string());
  }

  let backend = if overlay {
    "macos.ax.click-window-text+overlay-ffi"
  } else {
    "macos.ax.click-window-text"
  };

  Ok(DriverResponse {
    summary: format!(
      "Pressed AX node at OCR text {:?} inside {} via AXUIElementPerformAction (no cursor warp).",
      matched_ocr.text,
      if ax_snapshot.app_name.is_empty() {
        "target app"
      } else {
        &ax_snapshot.app_name
      }
    ),
    backend: Some(backend.to_string()),
    signals,
    notes,
    artifacts: {
      let mut artifacts = vec![screenshot_artifact, ocr_artifact, report_artifact];
      artifacts.append(&mut overlay_artifacts);
      artifacts
    },
  })
}

pub(crate) fn smart_press(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let allow_pointer_fallback = optional_bool(call, "allow_pointer_fallback")?.unwrap_or(true);
  let overlay_requested = optional_bool(call, "overlay")?;

  let mut smart_call = call.clone();
  smart_call.operation = "smart_press".to_string();
  if overlay_requested.is_none() {
    smart_call
      .inputs
      .insert("overlay".to_string(), "true".to_string());
  }

  match ax_click_window_text(&smart_call) {
    Ok(response) => mark_smart_press_response(response, &query, "ax-action", false, None),
    Err(primary_error) => {
      if !allow_pointer_fallback {
        return Err(format!(
          "smartPress AX strategy failed and pointer fallback is disabled: {primary_error}"
        ));
      }

      match click_window_text(&smart_call) {
        Ok(response) => {
          mark_smart_press_response(response, &query, "pointer-click", true, Some(primary_error))
        }
        Err(fallback_error) => Err(format!(
          "smartPress AX strategy failed: {primary_error}; pointer fallback also failed: {fallback_error}"
        )),
      }
    }
  }
}

fn best_effort_ax_overlay_artifacts(
  call: &DriverCall,
  capture_label: &str,
  kind: &'static str,
  query: &str,
  strategy: &str,
  press_mechanism: &str,
  cursor_disturbance: &str,
  overlay_presentation: Option<&str>,
  center_x: f64,
  center_y: f64,
  include_user_cursor: bool,
  auv_cursor_variant: &'static str,
  matched: &ObservedAxNode,
) -> (Vec<ProducedArtifact>, Option<String>) {
  let capture = match capture_resolved_window_observation(call, capture_label) {
    Ok(capture) => capture,
    Err(error) => {
      return (
        Vec::new(),
        Some(format!(
          "overlayEvidenceCaptureError={}",
          report_value(&error)
        )),
      );
    }
  };

  let screenshot_artifact = screenshot_artifact(&capture, capture_label, "ax overlay evidence");
  let overlay_artifacts = match build_overlay_evidence_artifacts(OverlayEvidenceRequest {
    kind,
    label: capture_label.to_string(),
    screenshot_path: screenshot_artifact.source_path.clone(),
    screenshot_dimensions: capture.dimensions.clone(),
    capture_contract: capture.capture_contract.clone(),
    query: Some(query.to_string()),
    strategy: Some(strategy.to_string()),
    fallback_used: Some(false),
    cursor_disturbance: Some(cursor_disturbance.to_string()),
    press_mechanism: Some(press_mechanism.to_string()),
    overlay_presentation: overlay_presentation.map(str::to_string),
    action_point: logical_to_capture_pixel(&capture.capture_contract, center_x, center_y),
    expected_target: Some(logical_to_capture_pixel(
      &capture.capture_contract,
      center_x,
      center_y,
    )),
    ocr_match: None,
    row: None,
    ax_target: Some(OverlayEvidenceAxTarget {
      role: matched.role.clone(),
      label: if matched.title.is_empty() {
        query.to_string()
      } else {
        matched.title.clone()
      },
      bounds: matched.bounds.clone(),
    }),
    decision: None,
    include_user_cursor,
    auv_cursor_variant,
  }) {
    Ok(artifacts) => artifacts,
    Err(error) => {
      return (
        vec![screenshot_artifact],
        Some(format!(
          "overlayEvidenceBuildError={}",
          report_value(&error)
        )),
      );
    }
  };

  let mut artifacts = vec![screenshot_artifact];
  artifacts.extend(overlay_artifacts);
  (artifacts, None)
}

fn mark_smart_press_response(
  mut response: DriverResponse,
  query: &str,
  strategy: &str,
  fallback_used: bool,
  primary_error: Option<String>,
) -> AuvResult<DriverResponse> {
  augment_smart_press_overlay_annotation(
    &mut response,
    strategy,
    fallback_used,
    primary_error.as_deref(),
  )?;
  response
    .signals
    .insert("smartPress.strategy".to_string(), strategy.to_string());
  response.signals.insert(
    "smartPress.fallbackUsed".to_string(),
    fallback_used.to_string(),
  );
  response.notes.push("smartPress=true".to_string());
  response
    .notes
    .push(format!("smartPressStrategy={strategy}"));
  response
    .notes
    .push(format!("smartPressFallbackUsed={fallback_used}"));
  if let Some(error) = primary_error.as_deref() {
    response
      .notes
      .push(format!("smartPressPrimaryError={}", report_value(error)));
  }

  let mut report = vec![
    "operation=smart_press".to_string(),
    format!("query={query}"),
    format!("strategy={strategy}"),
    format!("fallbackUsed={fallback_used}"),
  ];
  if let Some(error) = primary_error.as_deref() {
    report.push(format!("primaryError={}", report_value(error)));
  }
  let artifact = build_text_artifact(
    "smart-press",
    "txt",
    &format!("smart-press-{}", sanitize_file_component(query)),
    report.join("\n") + "\n",
    "Recorded the selected smartPress strategy and fallback decision.",
  )?;
  response.artifacts.push(artifact);

  response.backend = Some(format!("macos.smart-press.{strategy}"));
  response.summary = format!("Smart press used {strategy}: {}", response.summary);
  Ok(response)
}

fn augment_smart_press_overlay_annotation(
  response: &mut DriverResponse,
  selected_strategy: &str,
  fallback_used: bool,
  primary_error: Option<&str>,
) -> AuvResult<()> {
  let Some(annotation_artifact) = response
    .artifacts
    .iter()
    .find(|artifact| artifact.kind == "click.overlay.annotation")
  else {
    return Ok(());
  };

  let annotation_raw = fs::read_to_string(&annotation_artifact.source_path).map_err(|error| {
    format!(
      "failed to read smartPress overlay annotation {}: {error}",
      annotation_artifact.source_path.display()
    )
  })?;
  let mut payload: serde_json::Value = serde_json::from_str(&annotation_raw).map_err(|error| {
    format!(
      "failed to parse smartPress overlay annotation {}: {error}",
      annotation_artifact.source_path.display()
    )
  })?;

  payload["decision"] = serde_json::json!({
    "operation": "smart_press",
    "primary_strategy": "ax-action",
    "selected_strategy": selected_strategy,
    "fallback_used": fallback_used,
    "primary_error": primary_error,
  });
  payload["fallback_used"] = serde_json::Value::Bool(fallback_used);
  payload["strategy"] = serde_json::Value::String("smart-press".to_string());
  payload["press_mechanism"] = serde_json::Value::String(selected_strategy.to_string());

  fs::write(
    &annotation_artifact.source_path,
    serde_json::to_string_pretty(&payload)
      .map_err(|error| format!("failed to encode smartPress overlay annotation: {error}"))?
      + "\n",
  )
  .map_err(|error| {
    format!(
      "failed to write smartPress overlay annotation {}: {error}",
      annotation_artifact.source_path.display()
    )
  })?;

  Ok(())
}

fn report_value(raw: &str) -> String {
  raw
    .replace('\\', "\\\\")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
    .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::driver::macos::support::temp_file_path;
  use crate::model::ProducedArtifact;

  #[test]
  fn augment_smart_press_overlay_annotation_records_decision() {
    let annotation_path = temp_file_path("smart-press-overlay-annotation-test", "json");
    fs::write(
      &annotation_path,
      serde_json::json!({
        "version": "v1alpha1",
        "kind": "window-text-click",
        "strategy": "ocr-text",
        "fallback_used": null,
        "press_mechanism": "pointer-click",
      })
      .to_string(),
    )
    .expect("annotation fixture should write");
    let mut response = DriverResponse {
      summary: "ok".to_string(),
      backend: None,
      signals: std::collections::BTreeMap::new(),
      notes: Vec::new(),
      artifacts: vec![ProducedArtifact {
        kind: "click.overlay.annotation".to_string(),
        source_path: annotation_path.clone(),
        preferred_name: "smart-press-overlay-annotation-test.json".to_string(),
        note: None,
      }],
    };

    augment_smart_press_overlay_annotation(
      &mut response,
      "pointer-click",
      true,
      Some("AX target had no matching action"),
    )
    .expect("annotation rewrite should succeed");

    let payload: serde_json::Value = serde_json::from_str(
      &fs::read_to_string(&annotation_path).expect("annotation should remain readable"),
    )
    .expect("annotation should remain valid json");
    assert_eq!(payload["strategy"], "smart-press");
    assert_eq!(payload["fallback_used"], true);
    assert_eq!(payload["press_mechanism"], "pointer-click");
    assert_eq!(payload["decision"]["operation"], "smart_press");
    assert_eq!(payload["decision"]["primary_strategy"], "ax-action");
    assert_eq!(payload["decision"]["selected_strategy"], "pointer-click");
    assert_eq!(
      payload["decision"]["primary_error"],
      "AX target had no matching action"
    );

    let _ = fs::remove_file(annotation_path);
  }
}
