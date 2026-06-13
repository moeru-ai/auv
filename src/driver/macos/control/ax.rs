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
use super::super::support::{
  artifacts::{build_text_artifact, sanitize_file_component},
  ax::{
    ax_node_center, find_ax_node_at_point, find_best_ax_node, no_matching_ax_node_error,
    render_ax_interaction_report,
  },
  call::{
    app_identifier, optional_bool, optional_f64, optional_i64, optional_non_empty_string,
    optional_positive_u64, required_non_empty_string,
  },
  geometry::render_rect_compact,
  ocr_commands::{run_text_match_on_capture, screenshot_artifact},
  overlay_evidence::{
    OverlayEvidenceAxTarget, OverlayEvidenceMatch, OverlayEvidenceRequest,
    build_overlay_evidence_artifacts, logical_to_capture_pixel,
  },
};
use super::super::{DriverCall, DriverResponse, ObservedAxNode, ProducedArtifact};
use super::action_resolver::{ActionResolverDecision, ResolvedActionMethod};
use super::common::{
  DEFAULT_CLICK_INTERVAL_MS, activate_app_if_needed, build_ax_click_notes,
  send_reveal_shortcut_if_needed,
};
use super::window_ocr::{
  capture_resolved_window_observation, click_window_text, logical_point_for_match,
};
use crate::contract::{Candidate, TargetGrounding};
use crate::model::AuvResult;
use serde::Deserialize;
use std::fs;

pub(crate) fn focus_text_input(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = optional_non_empty_string(call, "query");
  let candidate_json = optional_non_empty_string(call, "candidate");
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  if query.is_none() && candidate_json.is_none() {
    return Err("operation requires --query <text> or --candidate <json>".to_string());
  }

  activate_app_if_needed(&app)?;
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let snapshot =
    auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let target = resolve_focus_text_target(&snapshot, candidate_json.as_deref(), query.as_deref())?;
  let matched = target.matched;
  let (center_x, center_y) = ax_node_center(matched);
  auv_driver_macos::native::pointer::click_point(
    center_x,
    center_y,
    0,
    1,
    DEFAULT_CLICK_INTERVAL_MS,
  )?;

  let report = render_ax_interaction_report("focus-text-input", &snapshot, matched, &target.query);
  let artifact = build_text_artifact(
    "focus-text-input",
    "txt",
    &format!(
      "focus-text-input-{}",
      sanitize_file_component(&target.query)
    ),
    report,
    "Focused a text input by matching the observed AX tree and clicking the resolved bounds.",
  )?;
  let mut notes = build_ax_click_notes(&target.query, matched, center_x, center_y);
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.placeholder.is_empty() {
    notes.push(format!("matchedPlaceholder={}", matched.placeholder));
  }
  let mut signals = std::collections::BTreeMap::new();
  if let Some(candidate_local_id) = target.consumed_candidate_local_id.as_deref() {
    notes.push(format!("consumedCandidateLocalId={candidate_local_id}"));
    signals.insert(
      "focusTextInput.consumer".to_string(),
      "contract-candidate".to_string(),
    );
    signals.insert(
      "focusTextInput.candidateLocalId".to_string(),
      candidate_local_id.to_string(),
    );
  } else {
    signals.insert("focusTextInput.consumer".to_string(), "query".to_string());
  }
  if let Some(window_number) = target.unverified_window_number {
    signals.insert(
      "focusTextInput.windowNumberPrecondition".to_string(),
      "declared_but_unverified".to_string(),
    );
    notes.push(format!("windowNumberDeclaredButUnverified={window_number}"));
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
        target.query,
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
        target.query
      )
    },
    backend: Some("macos.desktop.ax-tree-click-focus".to_string()),
    signals,
    notes,
    artifacts: vec![artifact],
  })
}

#[derive(Clone, Debug)]
struct FocusTextTarget<'a> {
  query: String,
  matched: &'a ObservedAxNode,
  consumed_candidate_local_id: Option<String>,
  unverified_window_number: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct FocusTextCandidateObservation {
  #[serde(default)]
  query: Option<FocusTextCandidateObservationQuery>,
  #[serde(default)]
  bounds: Option<FocusTextObservedBounds>,
}

#[derive(Clone, Debug, Deserialize)]
struct FocusTextCandidateObservationQuery {
  #[serde(default)]
  ax: Option<FocusTextCandidateAxSelector>,
}

#[derive(Clone, Debug, Deserialize)]
struct FocusTextCandidateAxSelector {
  #[serde(default)]
  role: Option<String>,
  #[serde(default)]
  label: Option<String>,
  #[serde(default)]
  path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct FocusTextObservedBounds {
  x: i64,
  y: i64,
  width: i64,
  height: i64,
}

#[derive(Clone, Debug)]
struct ResolvedFocusTextCandidate {
  candidate_local_id: String,
  query: String,
  role: Option<String>,
  label: Option<String>,
  path: Option<String>,
  observed_bounds: Option<FocusTextObservedBounds>,
  expected_window: Option<crate::contract::WindowRefPrecondition>,
}

fn resolve_focus_text_target<'a>(
  snapshot: &'a auv_driver_macos::types::ObservedAxTreeSnapshot,
  candidate_json: Option<&str>,
  query: Option<&str>,
) -> AuvResult<FocusTextTarget<'a>> {
  if let Some(candidate_json) = candidate_json {
    let candidate = parse_focus_text_candidate(candidate_json)?;
    let matched = resolve_focus_text_candidate(snapshot, &candidate).ok_or_else(|| {
      format!(
        "no matching text input-like node found for promoted candidate {}",
        candidate.candidate_local_id
      )
    })?;
    let unverified_window_number = unverified_window_number(candidate.expected_window.as_ref());
    return Ok(FocusTextTarget {
      query: candidate.query,
      matched,
      consumed_candidate_local_id: Some(candidate.candidate_local_id),
      unverified_window_number,
    });
  }

  let query =
    query.ok_or_else(|| "operation requires --query <text> or --candidate <json>".to_string())?;
  let matched = find_best_ax_node(snapshot, query)
    .ok_or_else(|| no_matching_ax_node_error(snapshot, query, "text input-like"))?;
  Ok(FocusTextTarget {
    query: query.to_string(),
    matched,
    consumed_candidate_local_id: None,
    unverified_window_number: None,
  })
}

fn parse_focus_text_candidate(raw_candidate: &str) -> AuvResult<ResolvedFocusTextCandidate> {
  let candidate: Candidate = serde_json::from_str(raw_candidate)
    .map_err(|error| format!("invalid --candidate JSON: {error}"))?;
  if candidate.target_spec.grounding != TargetGrounding::AxNode {
    return Err(format!(
      "focus_text_input only accepts AxNode candidates; got {:?}",
      candidate.target_spec.grounding
    ));
  }

  let observation: FocusTextCandidateObservation =
    serde_json::from_value(candidate.evidence.observation.clone()).map_err(|error| {
      format!("candidate observation is missing AX focus selector detail: {error}")
    })?;
  let ax = observation
    .query
    .and_then(|query| query.ax)
    .ok_or_else(|| "candidate observation is missing query.ax selector detail".to_string())?;
  let label = ax
    .label
    .as_deref()
    .and_then(non_empty_trimmed)
    .or_else(|| {
      candidate
        .label
        .clone()
        .and_then(|value| non_empty_trimmed(&value))
    })
    .or_else(|| {
      candidate
        .target_spec
        .anchor_text
        .clone()
        .and_then(|value| non_empty_trimmed(&value))
    });
  let path = ax.path.as_deref().and_then(non_empty_trimmed);
  let role = ax.role.as_deref().and_then(non_empty_trimmed);
  if label.is_none() && path.is_none() {
    return Err("candidate must carry at least one AX selector hint (label or path)".to_string());
  }
  let query = label
    .clone()
    .or_else(|| {
      candidate
        .target_spec
        .anchor_text
        .clone()
        .and_then(|value| non_empty_trimmed(&value))
    })
    .unwrap_or_else(|| candidate.candidate_local_id.clone());

  Ok(ResolvedFocusTextCandidate {
    candidate_local_id: candidate.candidate_local_id,
    query,
    role,
    label,
    path,
    observed_bounds: observation.bounds,
    expected_window: candidate.liveness.preconditions.window_ref,
  })
}

fn resolve_focus_text_candidate<'a>(
  snapshot: &'a auv_driver_macos::types::ObservedAxTreeSnapshot,
  candidate: &ResolvedFocusTextCandidate,
) -> Option<&'a ObservedAxNode> {
  if !focus_text_candidate_matches_window(snapshot, candidate.expected_window.as_ref()) {
    return None;
  }

  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter_map(|node| focus_text_candidate_score(node, candidate).map(|score| (score, node)))
    .max_by_key(|(score, _)| *score)
    .map(|(_, node)| node)
}

fn focus_text_candidate_score(
  node: &ObservedAxNode,
  candidate: &ResolvedFocusTextCandidate,
) -> Option<i64> {
  let mut score = 0i64;

  if let Some(path) = candidate.path.as_deref() {
    if node.path == path {
      score += 1000;
    } else if !path.is_empty() {
      score -= 200;
    }
  }

  if let Some(role) = candidate.role.as_deref() {
    if node.role == role {
      score += 200;
    } else {
      return None;
    }
  }

  if let Some(label) = candidate.label.as_deref() {
    let label_score = focus_text_node_label_score(node, label)?;
    score += label_score;
  }

  if let Some(bounds) = candidate.observed_bounds.as_ref() {
    score += focus_text_bounds_score(node, bounds);
  }

  Some(score - node.depth as i64)
}

fn focus_text_node_label_score(node: &ObservedAxNode, label: &str) -> Option<i64> {
  let normalized_label = normalize_focus_text_value(label);
  if normalized_label.is_empty() {
    return None;
  }

  let fields = [
    node.title.as_str(),
    node.description.as_str(),
    node.help.as_str(),
    node.identifier.as_str(),
    node.placeholder.as_str(),
    node.value.as_str(),
  ];

  let mut best_score = None;
  for raw_field in fields {
    let normalized_field = normalize_focus_text_value(raw_field);
    if normalized_field.is_empty() || !normalized_field.contains(&normalized_label) {
      continue;
    }
    let mut score = 120;
    if normalized_field == normalized_label {
      score += 40;
    }
    best_score = Some(best_score.map_or(score, |current: i64| current.max(score)));
  }

  best_score
}

fn focus_text_bounds_score(node: &ObservedAxNode, bounds: &FocusTextObservedBounds) -> i64 {
  let node_center_x = node.bounds.x + node.bounds.width / 2;
  let node_center_y = node.bounds.y + node.bounds.height / 2;
  let target_center_x = bounds.x + bounds.width / 2;
  let target_center_y = bounds.y + bounds.height / 2;
  let distance = (node_center_x - target_center_x).abs() + (node_center_y - target_center_y).abs();
  100 - distance.min(100)
}

fn normalize_focus_text_value(raw: &str) -> String {
  raw
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>()
    .to_lowercase()
}

fn focus_text_candidate_matches_window(
  snapshot: &auv_driver_macos::types::ObservedAxTreeSnapshot,
  expected_window: Option<&crate::contract::WindowRefPrecondition>,
) -> bool {
  let Some(expected_window) = expected_window else {
    return true;
  };

  if !expected_window.app_bundle_id.trim().is_empty()
    && !snapshot.bundle_id.trim().is_empty()
    && !snapshot
      .bundle_id
      .eq_ignore_ascii_case(expected_window.app_bundle_id.as_str())
  {
    return false;
  }

  if let Some(expected_title) = expected_window.window_title_substring.as_deref()
    && let Some(expected_title) = non_empty_trimmed(expected_title)
  {
    let normalized_expected = normalize_focus_text_value(&expected_title);
    let normalized_actual = normalize_focus_text_value(&snapshot.window_title);
    if normalized_actual.is_empty() || !normalized_actual.contains(&normalized_expected) {
      return false;
    }
  }

  // TODO(app-candidate-window-number-v1): enforce exact AX window identity once
  // AX snapshots carry a stable window number. Until then the value is surfaced
  // upstream via `unverified_window_number` so consumers see the silent-trust
  // gap instead of inferring it from a missing check.
  true
}

fn unverified_window_number(
  expected_window: Option<&crate::contract::WindowRefPrecondition>,
) -> Option<i64> {
  expected_window.and_then(|window| window.window_number)
}

fn non_empty_trimmed(raw: &str) -> Option<String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn append_focus_text_consumer_signals(
  target: &FocusTextTarget<'_>,
  notes: &mut Vec<String>,
  signals: &mut std::collections::BTreeMap<String, String>,
) {
  if let Some(candidate_local_id) = target.consumed_candidate_local_id.as_deref() {
    notes.push(format!("consumedCandidateLocalId={candidate_local_id}"));
    signals.insert(
      "focusTextInput.consumer".to_string(),
      "contract-candidate".to_string(),
    );
    signals.insert(
      "focusTextInput.candidateLocalId".to_string(),
      candidate_local_id.to_string(),
    );
  } else {
    signals.insert("focusTextInput.consumer".to_string(), "query".to_string());
  }

  if let Some(window_number) = target.unverified_window_number {
    signals.insert(
      "focusTextInput.windowNumberPrecondition".to_string(),
      "declared_but_unverified".to_string(),
    );
    notes.push(format!("windowNumberDeclaredButUnverified={window_number}"));
  }
}

pub(crate) fn ax_focus_text_input(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = optional_non_empty_string(call, "query");
  let candidate_json = optional_non_empty_string(call, "candidate");
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
  if query.is_none() && candidate_json.is_none() {
    return Err("operation requires --query <text> or --candidate <json>".to_string());
  }

  if activate {
    activate_app_if_needed(&app)?;
  }
  send_reveal_shortcut_if_needed(reveal_shortcut.as_deref(), reveal_settle_ms)?;

  let capture =
    auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?;
  let snapshot = &capture.snapshot;
  if capture.pid <= 0 {
    return Err(format!(
      "native AX tree capture did not return a valid pid for app {:?} (got {}); cannot dispatch AX focus",
      snapshot.app_name, capture.pid
    ));
  }
  let target = resolve_focus_text_target(snapshot, candidate_json.as_deref(), query.as_deref())?;
  let matched = target.matched;
  let (center_x, center_y) = ax_node_center(matched);

  let (focus_result, overlay_outcome) = if overlay {
    let (result, outcome) = with_overlay_cursor(center_x, center_y, &overlay_label, || {
      if preview_ms > 0 {
        auv_overlay_macos::pump_events(preview_ms)?;
      }
      let result = auv_driver_macos::native::ax_tree::set_ax_focused_path(
        capture.pid as i32,
        &matched.path,
        &matched.role,
      )?;
      if settle_ms > 0 {
        auv_overlay_macos::pump_events(settle_ms)?;
      }
      Ok(result)
    })?;
    (result, Some(outcome))
  } else {
    (
      auv_driver_macos::native::ax_tree::set_ax_focused_path(
        capture.pid as i32,
        &matched.path,
        &matched.role,
      )?,
      None,
    )
  };

  let report =
    render_ax_interaction_report("ax-focus-text-input", snapshot, matched, &target.query);
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
    &format!(
      "ax-focus-text-input-{}",
      sanitize_file_component(&target.query)
    ),
    report,
    "Focused a text input via AXUIElementSetAttributeValue(kAXFocusedAttribute); the real cursor is not moved.",
  )?;
  let capture_label = format!(
    "ax-focus-text-input-{}",
    sanitize_file_component(&target.query)
  );
  let (mut overlay_artifacts, overlay_capture_note) = best_effort_ax_overlay_artifacts(
    call,
    &capture_label,
    "ax-focus-text-input",
    &target.query,
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

  let mut notes = build_ax_click_notes(&target.query, matched, center_x, center_y);
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
  append_focus_text_consumer_signals(&target, &mut notes, &mut signals);

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Focused text input in {} via AXUIElementSetAttributeValue(kAXFocusedAttribute) using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        target.query,
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
        target.query
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
    auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
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
        auv_overlay_macos::pump_events(preview_ms)?;
      }
      let action = auv_driver_macos::native::ax_tree::perform_ax_path_action(
        snapshot.pid,
        &matched.path,
        &matched.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        auv_overlay_macos::pump_events(settle_ms)?;
      }
      Ok(action)
    })?;
    (action, Some(outcome))
  } else {
    (
      auv_driver_macos::native::ax_tree::perform_ax_path_action(
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
    auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "button-like"))?;
  let (center_x, center_y) = ax_node_center(matched);
  auv_driver_macos::native::pointer::click_point(
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
      "no OCR text match at index {match_index} for query {query} inside resolved window; observed {} match(es). Inspect `window.findText`",
      filtered.len()
    )
  })?;
  let (logical_x_base, logical_y_base) = logical_point_for_match(&capture, matched_ocr)?;
  let logical_x = logical_x_base + anchor_offset_x;
  let logical_y = logical_y_base + anchor_offset_y;

  // 2. Observe AX tree and resolve the pressable node under the OCR anchor.
  // Activation already happened inside capture_resolved_window_observation.
  let ax_capture =
    auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?;
  let ax_snapshot = &ax_capture.snapshot;
  if ax_capture.pid <= 0 {
    return Err(format!(
      "native AX tree capture did not return a valid pid for app {:?} (got {}); cannot dispatch AX action",
      ax_snapshot.app_name, ax_capture.pid
    ));
  }
  let ax_node = find_ax_node_at_point(ax_snapshot, logical_x, logical_y).ok_or_else(|| {
    format!(
      "no AX node found at OCR anchor ({logical_x:.3}, {logical_y:.3}) for text {:?}; the visible text may be canvas-rendered or outside the observed window's AX subtree. Try window.clickText if you accept cursor warp.",
      matched_ocr.text
    )
  })?;
  let (center_x, center_y) = ax_node_center(ax_node);

  // 3. AX press (optionally wrapped by the overlay marker for visual feedback).
  let (press_action, overlay_outcome) = if overlay {
    let (action, outcome) = with_overlay_cursor(center_x, center_y, &overlay_label, || {
      if preview_ms > 0 {
        auv_overlay_macos::pump_events(preview_ms)?;
      }
      let action = auv_driver_macos::native::ax_tree::perform_ax_path_action(
        ax_capture.pid as i32,
        &ax_node.path,
        &ax_node.role,
        &action_name,
      )?;
      if settle_ms > 0 {
        auv_overlay_macos::pump_events(settle_ms)?;
      }
      Ok(action)
    })?;
    (action, Some(outcome))
  } else {
    (
      auv_driver_macos::native::ax_tree::perform_ax_path_action(
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
    "Vision OCR text-anchor report consumed by input.axClickWindowText.",
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
    Ok(response) => mark_smart_press_response(
      response,
      &query,
      ResolvedActionMethod::AxAction,
      allow_pointer_fallback,
      None,
    ),
    Err(primary_error) => {
      if !allow_pointer_fallback {
        return Err(format!(
          "smartPress AX strategy failed and pointer fallback is disabled: {primary_error}"
        ));
      }

      match click_window_text(&smart_call) {
        Ok(response) => mark_smart_press_response(
          response,
          &query,
          ResolvedActionMethod::PointerClick,
          allow_pointer_fallback,
          Some(primary_error),
        ),
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
  selected_method: ResolvedActionMethod,
  fallback_allowed: bool,
  primary_error: Option<String>,
) -> AuvResult<DriverResponse> {
  let decision = ActionResolverDecision::smart_press(
    query,
    selected_method,
    fallback_allowed,
    primary_error.as_deref(),
  );
  let strategy = decision.selected_method.clone();
  let fallback_used = decision.fallback_used;
  augment_smart_press_overlay_annotation(
    &mut response,
    &strategy,
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
  response.notes.extend(decision.notes());
  for (key, value) in decision.signals() {
    response.signals.insert(key, value);
  }
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
  response.artifacts.push(decision.artifact()?);

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
  use crate::action_resolver_decision::ACTION_RESOLVER_VERSION;
  use crate::contract::{
    AnchorRecheckPrecondition, ArtifactRef, Candidate, CandidateEvidence, CandidateLiveness,
    ControlRequirements, LivenessPreconditions, RatioRegion, TargetGrounding, TargetSpec,
    WindowRefPrecondition,
  };
  use crate::driver::macos::support::temp_file_path;
  use crate::model::ProducedArtifact;
  use crate::trace::{ArtifactId, RunId, SpanId};

  fn sample_focus_snapshot() -> auv_driver_macos::types::ObservedAxTreeSnapshot {
    auv_driver_macos::types::ObservedAxTreeSnapshot {
      observed_at: "2026-06-03T00:00:00Z".to_string(),
      app_name: "Example".to_string(),
      bundle_id: "com.example.App".to_string(),
      pid: 4242,
      window_title: "Example Window".to_string(),
      nodes: vec![
        ObservedAxNode {
          depth: 1,
          path: "0.1".to_string(),
          role: "AXTextField".to_string(),
          subrole: String::new(),
          title: String::new(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: "Other".to_string(),
          value: String::new(),
          focused: false,
          bounds: auv_driver_macos::types::ObservedRect {
            x: 20,
            y: 20,
            width: 120,
            height: 32,
          },
        },
        ObservedAxNode {
          depth: 2,
          path: "0.3".to_string(),
          role: "AXTextField".to_string(),
          subrole: "AXSearchField".to_string(),
          title: String::new(),
          description: String::new(),
          help: String::new(),
          identifier: "search-field".to_string(),
          placeholder: "Search".to_string(),
          value: String::new(),
          focused: false,
          bounds: auv_driver_macos::types::ObservedRect {
            x: 120,
            y: 64,
            width: 220,
            height: 36,
          },
        },
      ],
    }
  }

  fn sample_focus_candidate_json() -> String {
    sample_focus_candidate_json_with_kind("search-entry-focus-ax", "search_entry", "Search")
  }

  fn sample_focus_candidate_json_with_kind(
    candidate_local_id: &str,
    kind: &str,
    label: &str,
  ) -> String {
    serde_json::to_string(&Candidate {
      candidate_local_id: candidate_local_id.to_string(),
      kind: kind.to_string(),
      label: Some(label.to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::AxNode,
        anchor_text: Some(label.to_string()),
        region_hint: Some(RatioRegion {
          left: 0.1,
          top: 0.1,
          right: 0.3,
          bottom: 0.2,
        }),
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0001"),
          captured_event_id: None,
        },
        observation: serde_json::json!({
          "source": "ax",
          "surface_candidate_id": candidate_local_id,
          "query": {
            "query_id": candidate_local_id,
            "output_kind": "focus-query",
            "selector_within": "target_window",
            "require_visible": true,
            "ax": {
              "role": "AXTextField",
              "label": label,
              "path": "0.3",
              "visible": true
            }
          },
          "bounds": {
            "x": 120,
            "y": 64,
            "width": 220,
            "height": 36
          }
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.example.App".to_string(),
            window_title_substring: Some("Example".to_string()),
            window_number: None,
          }),
          anchor_recheck: Some(AnchorRecheckPrecondition {
            text: label.to_string(),
            region_hint: None,
            expected_min_confidence: 0.0,
            max_pixel_distance: 48.0,
          }),
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    })
    .expect("sample candidate should serialize")
  }

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

  #[test]
  fn mark_smart_press_response_adds_action_resolver_contract() {
    let response = DriverResponse {
      summary: "pressed".to_string(),
      backend: None,
      signals: std::collections::BTreeMap::new(),
      notes: Vec::new(),
      artifacts: Vec::new(),
    };

    let response = mark_smart_press_response(
      response,
      "播放",
      ResolvedActionMethod::PointerClick,
      true,
      Some("AX target had no matching action".to_string()),
    )
    .expect("smart press marker should be applied");

    assert_eq!(
      response.signals.get("smartPress.strategy"),
      Some(&"pointer-click".to_string())
    );
    assert_eq!(
      response.signals.get("smartPress.fallbackUsed"),
      Some(&"true".to_string())
    );
    assert_eq!(
      response.signals.get("actionResolver.version"),
      Some(&ACTION_RESOLVER_VERSION.to_string())
    );
    assert_eq!(
      response.signals.get("actionResolver.selectedMethod"),
      Some(&"pointer-click".to_string())
    );
    assert_eq!(
      response.signals.get("actionResolver.fallbackReason"),
      Some(&"AX target had no matching action".to_string())
    );
    assert_eq!(
      response.signals.get("actionResolver.cursorDisturbance"),
      Some(&"warp-visible".to_string())
    );
    assert!(
      response
        .notes
        .contains(&"actionResolverSelectedMethod=pointer-click".to_string())
    );
    assert!(
      response
        .artifacts
        .iter()
        .any(|artifact| artifact.kind == "action.resolver.decision")
    );
    assert_eq!(
      response.backend.as_deref(),
      Some("macos.smart-press.pointer-click")
    );

    for artifact in response.artifacts {
      let _ = fs::remove_file(artifact.source_path);
    }
  }

  #[test]
  fn resolve_focus_text_target_consumes_promoted_candidate_selector() {
    let snapshot = sample_focus_snapshot();
    let candidate_json = sample_focus_candidate_json();

    let resolved = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect("candidate should resolve");

    assert_eq!(resolved.query, "Search");
    assert_eq!(resolved.matched.path, "0.3");
    assert_eq!(
      resolved.consumed_candidate_local_id.as_deref(),
      Some("search-entry-focus-ax")
    );
  }

  #[test]
  fn resolve_focus_text_target_accepts_native_text_candidate_kind() {
    let snapshot = sample_focus_snapshot();
    let candidate_json =
      sample_focus_candidate_json_with_kind("native-text-focus-ax", "native_text", "Search");

    let resolved = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect("native-text candidate should resolve");

    assert_eq!(resolved.query, "Search");
    assert_eq!(resolved.matched.path, "0.3");
    assert_eq!(
      resolved.consumed_candidate_local_id.as_deref(),
      Some("native-text-focus-ax")
    );
  }

  #[test]
  fn resolve_focus_text_target_rejects_candidate_when_window_ref_bundle_mismatches_snapshot() {
    let snapshot = sample_focus_snapshot();
    let candidate_json = sample_focus_candidate_json_with_bundle_id(
      "search-entry-focus-ax",
      "search_entry",
      "Search",
      "com.other.App",
      Some("Example"),
    );

    let error = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect_err("mismatched window_ref bundle should reject candidate");

    assert!(error.contains("no matching text input-like node found"));
  }

  #[test]
  fn resolve_focus_text_target_rejects_candidate_when_window_ref_title_mismatches_snapshot() {
    let snapshot = sample_focus_snapshot();
    let candidate_json = sample_focus_candidate_json_with_bundle_id(
      "search-entry-focus-ax",
      "search_entry",
      "Search",
      "com.example.App",
      Some("Other Window"),
    );

    let error = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect_err("mismatched window_ref title should reject candidate");

    assert!(error.contains("no matching text input-like node found"));
  }

  #[test]
  fn resolve_focus_text_target_surfaces_unverified_window_number_precondition() {
    let snapshot = sample_focus_snapshot();
    let candidate_json = {
      let mut value: serde_json::Value =
        serde_json::from_str(&sample_focus_candidate_json()).expect("parse fixture");
      value["liveness"]["preconditions"]["window_ref"]["window_number"] = serde_json::json!(42);
      serde_json::to_string(&value).expect("serialize patched fixture")
    };

    let resolved = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect("candidate should resolve");

    assert_eq!(resolved.unverified_window_number, Some(42));
  }

  #[test]
  fn resolve_focus_text_target_does_not_surface_window_number_when_precondition_omits_it() {
    let snapshot = sample_focus_snapshot();
    let candidate_json = sample_focus_candidate_json();

    let resolved = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect("candidate should resolve");

    assert_eq!(resolved.unverified_window_number, None);
  }

  #[test]
  fn resolve_focus_text_target_does_not_surface_window_number_on_query_only_path() {
    let snapshot = sample_focus_snapshot();

    let resolved = resolve_focus_text_target(&snapshot, None, Some("Search"))
      .expect("query path should resolve");

    assert_eq!(resolved.unverified_window_number, None);
  }

  #[test]
  fn append_focus_text_consumer_signals_marks_contract_candidate_path() {
    let snapshot = sample_focus_snapshot();
    let candidate_json =
      sample_focus_candidate_json_with_kind("native-text-focus-ax", "native_text", "Search");
    let resolved = resolve_focus_text_target(&snapshot, Some(&candidate_json), None)
      .expect("candidate should resolve");

    let mut notes = Vec::new();
    let mut signals = std::collections::BTreeMap::new();
    append_focus_text_consumer_signals(&resolved, &mut notes, &mut signals);

    assert_eq!(
      signals.get("focusTextInput.consumer").map(String::as_str),
      Some("contract-candidate")
    );
    assert_eq!(
      signals
        .get("focusTextInput.candidateLocalId")
        .map(String::as_str),
      Some("native-text-focus-ax")
    );
    assert!(
      notes
        .iter()
        .any(|note| { note == "consumedCandidateLocalId=native-text-focus-ax" })
    );
  }

  #[test]
  fn append_focus_text_consumer_signals_marks_query_path() {
    let snapshot = sample_focus_snapshot();
    let resolved = resolve_focus_text_target(&snapshot, None, Some("Search"))
      .expect("query path should resolve");

    let mut notes = Vec::new();
    let mut signals = std::collections::BTreeMap::new();
    append_focus_text_consumer_signals(&resolved, &mut notes, &mut signals);

    assert_eq!(
      signals.get("focusTextInput.consumer").map(String::as_str),
      Some("query")
    );
    assert!(!signals.contains_key("focusTextInput.candidateLocalId"));
    assert!(
      !notes
        .iter()
        .any(|note| note.starts_with("consumedCandidateLocalId="))
    );
  }

  #[test]
  fn parse_focus_text_candidate_rejects_non_ax_grounding() {
    let candidate_json = serde_json::to_string(&Candidate {
      candidate_local_id: "ocr-anchor".to_string(),
      kind: "search_entry".to_string(),
      label: Some("Search".to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::OcrAnchor,
        anchor_text: Some("Search".to_string()),
        region_hint: None,
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0001"),
          captured_event_id: None,
        },
        observation: serde_json::json!({}),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: None,
          anchor_recheck: None,
        },
        ttl_hint_ms: None,
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    })
    .expect("candidate json");

    let error =
      parse_focus_text_candidate(&candidate_json).expect_err("non-AxNode candidate should reject");
    assert!(error.contains("only accepts AxNode candidates"));
  }

  fn sample_focus_candidate_json_with_bundle_id(
    candidate_local_id: &str,
    kind: &str,
    label: &str,
    app_bundle_id: &str,
    window_title_substring: Option<&str>,
  ) -> String {
    serde_json::to_string(&Candidate {
      candidate_local_id: candidate_local_id.to_string(),
      kind: kind.to_string(),
      label: Some(label.to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::AxNode,
        anchor_text: Some(label.to_string()),
        region_hint: Some(RatioRegion {
          left: 0.1,
          top: 0.1,
          right: 0.3,
          bottom: 0.2,
        }),
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0001"),
          captured_event_id: None,
        },
        observation: serde_json::json!({
          "source": "ax",
          "surface_candidate_id": candidate_local_id,
          "query": {
            "query_id": candidate_local_id,
            "output_kind": "focus-query",
            "selector_within": "target_window",
            "require_visible": true,
            "ax": {
              "role": "AXTextField",
              "label": label,
              "path": "0.3",
              "visible": true
            }
          },
          "bounds": {
            "x": 120,
            "y": 64,
            "width": 220,
            "height": 36
          }
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: app_bundle_id.to_string(),
            window_title_substring: window_title_substring.map(str::to_string),
            window_number: None,
          }),
          anchor_recheck: Some(AnchorRecheckPrecondition {
            text: label.to_string(),
            region_hint: None,
            expected_min_confidence: 0.0,
            max_pixel_distance: 48.0,
          }),
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    })
    .expect("sample candidate should serialize")
  }
}
