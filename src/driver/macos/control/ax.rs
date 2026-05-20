use std::thread;
use std::time::Duration;

use super::super::overlay::with_overlay_cursor;
use super::super::*;
use super::common::{
  DEFAULT_CLICK_INTERVAL_MS, activate_app_if_needed, build_ax_click_notes,
  send_reveal_shortcut_if_needed,
};
use super::window_ocr::{
  capture_resolved_window_observation, click_window_text, logical_point_for_match,
};

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
    backend: Some("macos.observe.ax-tree-click-focus".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
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
  let overlay_label = optional_non_empty_string(call, "label").unwrap_or_else(|| "AUV".to_string());
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
        thread::sleep(Duration::from_millis(preview_ms));
      }
      let action = crate::driver::macos::native::ax_tree::perform_ax_path_action(
        snapshot.pid,
        &matched.path,
        &matched.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        thread::sleep(Duration::from_millis(settle_ms));
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
    if overlay { "visual-only" } else { "off" },
  );
  if let Some(outcome) = &overlay_outcome {
    report.push_str(&format!("overlayShowEvent={}\n", outcome.show_event));
    report.push_str(&format!("overlayHideEvent={}\n", outcome.hide_event));
    report.push_str(&format!("daemonPid={}\n", outcome.daemon_pid));
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

  let mut notes = build_ax_click_notes(&query, matched, center_x, center_y);
  notes.push("pressMechanism=ax-action".to_string());
  notes.push("cursorDisturbance=none".to_string());
  notes.push(format!("performedAction={performed_action}"));
  if !available_actions.is_empty() {
    notes.push(format!("availableActions={available_actions}"));
  }
  notes.push(format!("activatedApp={activate}"));
  if let Some(outcome) = &overlay_outcome {
    notes.push("overlayPresentation=visual-only".to_string());
    notes.push(format!("overlayShowEvent={}", outcome.show_event));
    notes.push(format!("overlayHideEvent={}", outcome.hide_event));
    notes.push(format!("daemonPid={}", outcome.daemon_pid));
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
    signals.insert("daemonPid".to_string(), outcome.daemon_pid.to_string());
  }

  let backend = if overlay {
    "macos.ax.perform-action+overlay-daemon"
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
    artifacts: vec![artifact],
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
    backend: Some("macos.observe.ax-tree-click-press".to_string()),
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
  let overlay_label = optional_non_empty_string(call, "label").unwrap_or_else(|| "AUV".to_string());
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
        thread::sleep(Duration::from_millis(preview_ms));
      }
      let action = crate::driver::macos::native::ax_tree::perform_ax_path_action(
        ax_capture.pid as i32,
        &ax_node.path,
        &ax_node.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        thread::sleep(Duration::from_millis(settle_ms));
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
    if overlay { "visual-only" } else { "off" }
  ));
  if let Some(outcome) = &overlay_outcome {
    report.push_str(&format!("overlayShowEvent={}\n", outcome.show_event));
    report.push_str(&format!("overlayHideEvent={}\n", outcome.hide_event));
    report.push_str(&format!("daemonPid={}\n", outcome.daemon_pid));
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
    notes.push("overlayPresentation=visual-only".to_string());
    notes.push(format!("overlayShowEvent={}", outcome.show_event));
    notes.push(format!("overlayHideEvent={}", outcome.hide_event));
    notes.push(format!("daemonPid={}", outcome.daemon_pid));
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
    signals.insert("daemonPid".to_string(), outcome.daemon_pid.to_string());
  }

  let backend = if overlay {
    "macos.ax.click-window-text+overlay-daemon"
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
    artifacts: vec![ocr_artifact, report_artifact],
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

fn mark_smart_press_response(
  mut response: DriverResponse,
  query: &str,
  strategy: &str,
  fallback_used: bool,
  primary_error: Option<String>,
) -> AuvResult<DriverResponse> {
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

fn report_value(raw: &str) -> String {
  raw
    .replace('\\', "\\\\")
    .replace('\n', "\\n")
    .replace('\r', "\\r")
    .replace('\t', "\\t")
}
