// File: src/driver/macos/observe.rs
//! macOS observation + verification helpers.
//!
//! This module is part of the `macos.desktop` driver implementation. It shapes
//! signals/notes and builds artifacts for observation-style operations
//! (OCR/row detection, AX snapshot probing, simple verification reads).
//!
//! Boundary: these are evidence-producing primitives and heuristics, not a
//! generic UI model. Higher-level meaning lives in recipes and typed consumers.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::time::Instant;

#[cfg(test)]
use super::ResolvedAppRef;
pub(super) use super::typed::observe::{
  find_ax_text_node, ocr_detection_signals, permission_probe_report, preferred_ax_signal_text,
  render_window_list_json, render_window_snapshot_report, row_detection_signals,
  verify_ax_text_signals, verify_now_playing_title_signals, wait_ocr_detection_signals,
  wait_row_detection_signals,
};
use super::*;
use crate::contract::{
  ArtifactRef, OperationOutput, OperationResult, OperationStatus, VerificationMethod,
  VerificationResult,
};
use crate::trace::RunId;

const VERIFY_AX_TEXT_OPERATION_ID: &str = "verify.axText";
const VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID: &str = "verify.musicNowPlaying";

pub(super) fn probe_coordinate_readiness(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "coordinate-readiness".to_string());
  let (screenshot_path, _capture_contract) =
    crate::driver::macos::capture::xcap_backend::capture_main_display_to_path(&label)?;
  let screenshot = read_png_dimensions(&screenshot_path)?;
  let snapshot = enumerate_displays()?;
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  let assessment = assess_coordinate_readiness(&snapshot, &screenshot)?;
  let report = render_coordinate_readiness_report(&snapshot, &screenshot, &assessment);
  let report_artifact = build_text_artifact(
    "coordinate-readiness",
    "txt",
    "coordinate-readiness-report",
    report,
    "Captured screenshot-backed coordinate readiness report from the observation driver.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some(
      "Screenshot captured while validating observation-side coordinate readiness.".to_string(),
    ),
  };

  let summary = if assessment.ready_for_logical_input {
    format!(
      "Coordinate readiness looks aligned for logical input; screenshot is {}x{} and matches the observed logical desktop space.",
      screenshot.width, screenshot.height
    )
  } else if assessment.likely_retina_backing_mismatch {
    format!(
      "Coordinate readiness is not aligned yet; screenshot is {}x{} physical pixels while main display #{} is {}x{} logical points at scale {:.3}.",
      screenshot.width,
      screenshot.height,
      main_display.display_id,
      main_display.bounds.width,
      main_display.bounds.height,
      main_display.scale_factor
    )
  } else {
    format!(
      "Coordinate readiness is not aligned yet; screenshot is {}x{} and does not match the observed logical desktop bounds.",
      screenshot.width, screenshot.height
    )
  };

  let mut notes = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!(
      "screenshotPixels={}x{}",
      screenshot.width, screenshot.height
    ),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
    format!(
      "readyForLogicalInput={}",
      assessment.ready_for_logical_input
    ),
    format!("reason={}", assessment.reason),
  ];
  notes.push(render_display_note(main_display));

  Ok(DriverResponse {
    summary,
    backend: Some("macos.desktop.coordinate-readiness".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(super) fn list_windows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let limit = optional_i64(call, "limit")?.unwrap_or(12).max(1);
  let app_filter = app_identifier(call).unwrap_or_default();
  let snapshot = observe_windows_snapshot(limit, &app_filter)?;
  let report = render_window_snapshot_report(&snapshot);
  let window_count = snapshot.windows.len();
  let frontmost_app = snapshot.frontmost_app_name.clone();
  let frontmost_window = snapshot.frontmost_window_title.clone();
  let observed_at = snapshot.observed_at.clone();
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let candidate_app = if app_filter.trim().is_empty() {
    snapshot.frontmost_app_bundle_id.trim()
  } else {
    app_filter.trim()
  };
  let mut candidate_note = None;
  let rendered_candidates = if candidate_app.is_empty() {
    Vec::new()
  } else {
    match parse_app_selector(candidate_app)
      .and_then(|selector| resolve_app_ref(&snapshot, &selector))
      .and_then(|resolved_app| build_window_candidates(&snapshot, &resolved_app, &displays))
    {
      Ok(candidates) => candidates,
      Err(error) => {
        candidate_note = Some(format!("candidateResolution={error}"));
        Vec::new()
      }
    }
  };
  let json = render_window_list_json(&snapshot, &rendered_candidates, candidate_note.as_deref())?;
  let json_artifact = build_text_artifact(
    "window-list",
    "json",
    "window-list",
    json,
    "Machine-readable macOS window candidate list.",
  )?;
  let text_artifact = build_text_artifact(
    "window-list",
    "txt",
    &format!("window-list-{}", sanitize_file_component(&frontmost_app)),
    report.clone(),
    "Human-readable macOS window candidate report.",
  )?;
  let mut notes = vec![format!("observedAt={observed_at}")];
  if let Some(candidate_note) = candidate_note {
    notes.push(candidate_note);
  }
  for line in report
    .lines()
    .filter(|line| line.starts_with("window\t"))
    .take(5)
  {
    notes.push(line.to_string());
  }
  let summary = if frontmost_app.is_empty() {
    format!("Observed {} visible macOS window(s).", window_count)
  } else if frontmost_window.is_empty() {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {}.",
      window_count, frontmost_app
    )
  } else {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {} ({})",
      window_count, frontmost_app, frontmost_window
    )
  };
  Ok(DriverResponse {
    summary,
    backend: Some("macos.swift.cgwindowlist".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![json_artifact, text_artifact],
  })
}

pub(super) fn observe_windows_snapshot(
  limit: i64,
  app_filter: &str,
) -> AuvResult<ObservedWindowSnapshot> {
  crate::driver::macos::native::window::observe_windows_snapshot(limit, app_filter)
}

/// Exposed for unit-tests only — wraps the private impl so tests can call it
/// without going through the full driver call machinery.
#[cfg(test)]
pub(super) fn build_selector_filtered_report_for_test(
  raw_report: &str,
  filtered_windows: &[&ObservedWindow],
  resolved_app: &ResolvedAppRef,
) -> String {
  build_selector_filtered_report_impl(raw_report, filtered_windows, resolved_app)
}

#[cfg(test)]
fn build_selector_filtered_report_impl(
  raw_report: &str,
  filtered_windows: &[&ObservedWindow],
  resolved_app: &ResolvedAppRef,
) -> String {
  let mut lines: Vec<String> = raw_report
    .lines()
    .filter(|line| !line.starts_with("windowCount=") && !line.starts_with("window\t"))
    .map(|line| line.to_string())
    .collect();
  lines.push(format!("appSelector={}", resolved_app.selector.raw));
  lines.push(format!("matchStrategy={}", resolved_app.match_strategy));
  lines.push(format!(
    "resolvedAppBundleId={}",
    resolved_app.resolved_bundle_id.as_deref().unwrap_or("")
  ));
  lines.push(format!(
    "resolvedAppName={}",
    resolved_app.resolved_app_name
  ));
  lines.push(format!("windowCount={}", filtered_windows.len()));
  for window in filtered_windows {
    lines.push(format!(
      "window\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      window.app_name,
      window.owner_pid,
      window.owner_bundle_id,
      window.window_number,
      window.layer,
      window.title,
      window.bounds.x,
      window.bounds.y,
      window.bounds.width,
      window.bounds.height,
    ));
  }
  lines.join("\n")
}

pub(super) fn verify_now_playing_title(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let expected_title = required_non_empty_string(call, "target_title")?;
  let expected_artist = optional_non_empty_string(call, "target_artist");
  let scope_path_prefix = optional_non_empty_string(call, "scope_path_prefix");
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(24)
    .clamp(1, 60);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }

  let snapshot =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let matched = find_now_playing_ax_node(
    &snapshot,
    &expected_title,
    expected_artist.as_deref(),
    scope_path_prefix.as_deref(),
  )
  .ok_or_else(|| {
    let mut detail = format!(
      "no matching now-playing node found for target_title {}",
      expected_title
    );
    if let Some(artist) = expected_artist.as_deref() {
      detail.push_str(&format!(" and target_artist {}", artist));
    }
    detail
  })?;

  // Reserve slots up front so the OperationResult evidence list can cite the
  // text report by its forward `ArtifactRef` before the artifact is staged.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let report_ref = artifacts.ref_at(0);
  let _operation_result_ref = artifacts.ref_at(1);

  let report = render_ax_interaction_report(
    "verify-now-playing-title",
    &snapshot,
    matched,
    &expected_title,
  );
  artifacts.push(build_text_artifact(
    "verify-now-playing-title",
    "txt",
    &format!(
      "verify-now-playing-title-{}",
      sanitize_file_component(&expected_title)
    ),
    report,
    "Captured an AX tree snapshot and matched the current now-playing title without relying on screenshot OCR.",
  )?);

  let verification = build_verify_now_playing_title_verification(matched, vec![report_ref]);
  let operation_result = build_verify_now_playing_title_operation_result(call, verification);
  artifacts.push(build_verify_now_playing_title_operation_result_artifact(
    &operation_result,
    &expected_title,
  )?);

  let mut notes = vec![
    format!("targetTitle={expected_title}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
  ];
  if let Some(artist) = expected_artist.as_deref() {
    notes.push(format!("targetArtist={artist}"));
  }
  if let Some(scope) = scope_path_prefix.as_deref() {
    notes.push(format!("scopePathPrefix={scope}"));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.value.is_empty() {
    notes.push(format!("matchedValue={}", matched.value));
  }

  Ok(DriverResponse {
    summary: format!(
      "Verified now-playing title {} in {} through the AX tree.",
      expected_title,
      if snapshot.app_name.is_empty() {
        "target app"
      } else {
        &snapshot.app_name
      }
    ),
    backend: Some("macos.desktop.verify-now-playing-title".to_string()),
    signals: verify_now_playing_title_signals(&matched.title),
    notes,
    artifacts: artifacts.into_vec(),
  })
}

/// Build the typed [`VerificationResult`] for `verify.musicNowPlaying`.
///
/// Only invoked on the success path — `find_now_playing_ax_node` returned
/// `None` for no-match cases above, so the assertion held and
/// `state_changed` reflects that the now-playing state matches the asserted
/// title. The method is [`VerificationMethod::SemanticMatch`] because the
/// match couples a target title (and optional artist) against the node's
/// observed signal text — the same shape `music.result.play` already emits
/// on the failure path in `music_result_play_failure_response`.
fn build_verify_now_playing_title_verification(
  matched: &ObservedAxNode,
  evidence: Vec<ArtifactRef>,
) -> VerificationResult {
  let observed_label = preferred_ax_signal_text(matched);
  VerificationResult {
    method: VerificationMethod::SemanticMatch,
    executed: true,
    state_changed: true,
    semantic_matched: Some(true),
    failure_layer: None,
    evidence,
    consumed_candidate_ref: None,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: None,
    consumed_recognition_id: None,
    consumed_recognized_item_id: None,
    observed_label: if observed_label.is_empty() {
      None
    } else {
      Some(observed_label)
    },
  }
}

fn build_verify_now_playing_title_operation_result(
  call: &DriverCall,
  verification: VerificationResult,
) -> OperationResult {
  let evidence = verification.evidence.clone();
  OperationResult {
    run_id: RunId::new(call.run_context.run_id.as_str()),
    status: OperationStatus::Completed,
    operation_id: VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID.to_string(),
    evidence_artifacts: evidence,
    output: OperationOutput::Acknowledged { message: None },
    verifications: vec![verification],
    freshness_basis: None,
    known_limits: Vec::new(),
  }
}

fn build_verify_now_playing_title_operation_result_artifact(
  operation_result: &OperationResult,
  expected_title: &str,
) -> AuvResult<ProducedArtifact> {
  let json = serde_json::to_string_pretty(operation_result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|error| {
      format!("failed to serialize verify.musicNowPlaying OperationResult: {error}")
    })?;
  build_text_artifact(
    "operation-result",
    "json",
    &format!(
      "verify-now-playing-title-{}-operation-result",
      sanitize_file_component(expected_title)
    ),
    json,
    "Typed OperationResult verification for verify.musicNowPlaying.",
  )
}

pub(super) fn verify_ax_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let expected_text = required_non_empty_string(call, "target_text")?;
  let expected_role = optional_non_empty_string(call, "target_role");
  let expected_subrole = optional_non_empty_string(call, "target_subrole");
  let scope_path_prefix = optional_non_empty_string(call, "scope_path_prefix");
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(24)
    .clamp(1, 60);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }

  let snapshot =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?
      .snapshot;
  let scope_path_prefix = scope_path_prefix
    .as_deref()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());

  let matched = find_ax_text_node(
    &snapshot.nodes,
    &expected_text,
    expected_role.as_deref(),
    expected_subrole.as_deref(),
    scope_path_prefix.as_deref(),
  )?;

  // Reserve slots up front so the OperationResult evidence list can cite the
  // text report by its forward `ArtifactRef` before the artifact is staged.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let report_ref = artifacts.ref_at(0);
  let _operation_result_ref = artifacts.ref_at(1);

  let report = render_ax_interaction_report("verify-ax-text", &snapshot, matched, &expected_text);
  artifacts.push(build_text_artifact(
    "verify-ax-text",
    "txt",
    &format!("verify-ax-text-{}", sanitize_file_component(&expected_text)),
    report,
    "Captured an AX tree snapshot and matched a text-bearing node without relying on screenshot OCR.",
  )?);

  let verification = build_verify_ax_text_verification(matched, vec![report_ref]);
  let operation_result = build_verify_ax_text_operation_result(call, verification);
  artifacts.push(build_verify_ax_text_operation_result_artifact(
    &operation_result,
    &expected_text,
  )?);

  let mut notes = vec![
    format!("targetText={expected_text}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
  ];
  if let Some(role) = expected_role.as_deref() {
    notes.push(format!("targetRole={role}"));
  }
  if let Some(subrole) = expected_subrole.as_deref() {
    notes.push(format!("targetSubrole={subrole}"));
  }
  if let Some(scope) = scope_path_prefix.as_deref() {
    notes.push(format!("scopePathPrefix={scope}"));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.value.is_empty() {
    notes.push(format!("matchedValue={}", matched.value));
  }

  let mut summary_suffix = String::new();
  if let Some(role) = expected_role.as_deref() {
    summary_suffix.push_str(&format!(" as {role}"));
  }
  if let Some(subrole) = expected_subrole.as_deref() {
    summary_suffix.push_str(&format!(" ({subrole})"));
  }

  Ok(DriverResponse {
    summary: format!(
      "Verified AX text {} in {}{} through the AX tree.",
      expected_text,
      if snapshot.app_name.is_empty() {
        "target app"
      } else {
        &snapshot.app_name
      },
      summary_suffix
    ),
    backend: Some("macos.desktop.verify-ax-text".to_string()),
    signals: verify_ax_text_signals(&preferred_ax_signal_text(matched), &matched.role),
    notes,
    artifacts: artifacts.into_vec(),
  })
}

/// Build the typed [`VerificationResult`] for `verify.axText`.
///
/// Only invoked on the success path — `find_ax_text_node` returned `Err` for
/// no-match cases above, so the assertion held and `state_changed` reflects
/// that the world matches the asserted text. `observed_label` carries the
/// node's preferred display text (value > title > description > help >
/// placeholder) so downstream consumers can show what AUV actually saw.
fn build_verify_ax_text_verification(
  matched: &ObservedAxNode,
  evidence: Vec<ArtifactRef>,
) -> VerificationResult {
  let observed_label = preferred_ax_signal_text(matched);
  VerificationResult {
    method: VerificationMethod::AxText,
    executed: true,
    state_changed: true,
    semantic_matched: Some(true),
    failure_layer: None,
    evidence,
    consumed_candidate_ref: None,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: None,
    consumed_recognition_id: None,
    consumed_recognized_item_id: None,
    observed_label: if observed_label.is_empty() {
      None
    } else {
      Some(observed_label)
    },
  }
}

fn build_verify_ax_text_operation_result(
  call: &DriverCall,
  verification: VerificationResult,
) -> OperationResult {
  let evidence = verification.evidence.clone();
  OperationResult {
    run_id: RunId::new(call.run_context.run_id.as_str()),
    status: OperationStatus::Completed,
    operation_id: VERIFY_AX_TEXT_OPERATION_ID.to_string(),
    evidence_artifacts: evidence,
    output: OperationOutput::Acknowledged { message: None },
    verifications: vec![verification],
    freshness_basis: None,
    known_limits: Vec::new(),
  }
}

fn build_verify_ax_text_operation_result_artifact(
  operation_result: &OperationResult,
  expected_text: &str,
) -> AuvResult<ProducedArtifact> {
  let json = serde_json::to_string_pretty(operation_result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|error| format!("failed to serialize verify.axText OperationResult: {error}"))?;
  build_text_artifact(
    "operation-result",
    "json",
    &format!(
      "verify-ax-text-{}-operation-result",
      sanitize_file_component(expected_text)
    ),
    json,
    "Typed OperationResult verification for verify.axText.",
  )
}

pub(super) fn project_screenshot_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let snapshot = enumerate_displays()?;
  let (logical_x, logical_y) = project_main_screenshot_point(&snapshot, x, y)?;
  let resolution = resolve_display_point(&snapshot, logical_x, logical_y)
    .ok_or_else(|| "projected logical point fell outside connected displays".to_string())?;
  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("screenshotPixelPoint={x:.3},{y:.3}"),
    format!("projectedLogicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "displayLogicalBounds={}",
      render_rect_compact(&resolution.display.bounds)
    ),
    format!(
      "displayPixelSize={}x{}",
      resolution.display.pixel_width, resolution.display.pixel_height
    ),
    format!("displayScaleFactor={:.3}", resolution.display.scale_factor),
    "coordinateContract=legacy live screenshot path uses main-display physical pixels".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "screenshot-point-projection",
    "txt",
    &format!(
      "screenshot-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Projected screenshot pixel coordinates back into AUV global logical coordinates.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Projected screenshot pixel ({x:.3}, {y:.3}) to global logical point ({logical_x:.3}, {logical_y:.3}) on display #{}.",
      resolution.display.display_id
    ),
    backend: Some("macos.desktop.screenshot-point".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: vec![
      format!("capturedAt={}", snapshot.captured_at),
      "coordinateSpace=main-display-physical-screenshot-pixels".to_string(),
      format!("globalLogicalPoint={logical_x:.3},{logical_y:.3}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
    ],
    artifacts: vec![artifact],
  })
}

pub(super) fn find_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-{}", sanitize_file_component(&query));
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let display_selection = parse_display_selection(call)?;
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let capture_source = resolve_screen_capture_source(&displays, display_selection.as_ref(), None)?;
  let (screenshot_path, capture_contract) =
    crate::driver::macos::capture::xcap_backend::capture_display_to_path(
      &label,
      Some(&capture_source.display_ref),
      Some(&capture_source.native_display_id),
      false,
    )?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  let capture = CapturedObservation {
    scope: "screen".to_string(),
    capture_source: capture_source.display_ref.clone(),
    screenshot_path: screenshot_path.clone(),
    capture_contract,
    dimensions: dimensions.clone(),
  };
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let (ocr_snapshot, filtered_matches, ocr_report, command_report) =
    run_text_match_on_capture(call, &capture, &query)?;
  let report_artifact = build_text_artifact(
    "screen-text-report",
    "txt",
    &format!("screen-text-report-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report for a desktop screenshot.",
  )?;
  let json_artifact = command_report
    .as_ref()
    .map(|report| {
      build_text_artifact(
        "screen-text-report",
        "json",
        &format!("screen-text-report-{}", sanitize_file_component(&query)),
        render_text_match_command_json(report)?,
        "Machine-readable OCR text-anchor command report.",
      )
    })
    .transpose()?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR text-anchor detection.".to_string()),
  };
  let mut notes = vec![
    format!("query={query}"),
    format!("matchCount={}", ocr_snapshot.matches.len()),
    format!("filteredMatchCount={}", filtered_matches.len()),
    format!("caseSensitive={case_sensitive}"),
    format!("exact={exact}"),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "screenshotPixels={}x{}",
      ocr_snapshot.image_width, ocr_snapshot.image_height
    ),
    format!("displayRef={}", capture_source.display_ref),
    format!("nativeDisplayId={}", capture_source.native_display_id),
    format!("captureSourceReason={}", capture_source.selection_reason),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }

  let summary = if let Some(best_match) = filtered_matches.first() {
    let (screenshot_center_x, screenshot_center_y) = ocr_match_center(best_match);
    let (logical_x, logical_y) =
      crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
        &capture.capture_contract,
        screenshot_center_x,
        screenshot_center_y,
      )?;
    notes.push(format!("bestMatchText={}", best_match.text));
    notes.push(format!(
      "bestMatchBounds={}",
      render_rect_compact(&best_match.bounds)
    ));
    notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
    notes.push(format!("bestLogicalPoint={logical_x:.3},{logical_y:.3}"));
    format!(
      "Found {} OCR text match(es) for query {} after filtering; best anchor {} projects to logical point ({logical_x:.3}, {logical_y:.3}).",
      filtered_matches.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches in the current desktop screenshot after applying the active filters."
      .to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.screen-text".to_string()),
    signals: ocr_detection_signals(
      filtered_matches.len(),
      filtered_matches
        .first()
        .map(|matched| matched.text.as_str()),
    ),
    notes,
    artifacts: match json_artifact {
      Some(json_artifact) => vec![screenshot_artifact, report_artifact, json_artifact],
      None => vec![screenshot_artifact, report_artifact],
    },
  })
}

pub(super) fn wait_for_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-wait-{}", sanitize_file_component(&query));
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let display_selection = parse_display_selection(call)?;
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let capture_source = resolve_screen_capture_source(&displays, display_selection.as_ref(), None)?;
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path: Option<PathBuf> = None;

  loop {
    attempts += 1;
    let attempt_label = format!("{label}-attempt-{attempts}");
    let activated_app = maybe_activate_target_app_for_observation(call)?;
    let (screenshot_path, capture_contract) =
      crate::driver::macos::capture::xcap_backend::capture_display_to_path(
        &attempt_label,
        Some(&capture_source.display_ref),
        Some(&capture_source.native_display_id),
        false,
      )?;
    let dimensions = read_png_dimensions(&screenshot_path)?;
    let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
    let ocr_capture = crate::driver::macos::native::ocr::find_text(
      screenshot_path.as_path(),
      &query,
      exact,
      case_sensitive,
      max_observations,
      region.as_ref(),
    )?;
    let ocr_report = crate::driver::macos::native::ocr::render_ocr_text_report(&ocr_capture);
    let ocr_snapshot = ocr_capture.snapshot;
    let filtered_matches =
      filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref())
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if !filtered_matches.is_empty() || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }

      let report_artifact = build_text_artifact(
        "screen-text-wait-report",
        "txt",
        &format!(
          "screen-text-wait-report-{}",
          sanitize_file_component(&query)
        ),
        ocr_report,
        "Captured Vision OCR text-anchor report from the final wait-for-screen-text polling attempt.",
      )?;
      let screenshot_artifact = ProducedArtifact {
        kind: "screenshot".to_string(),
        source_path: screenshot_path,
        preferred_name: format!("{}.png", sanitize_file_component(&label)),
        note: Some(
          "Final screenshot retained from waitForScreenText polling over the live desktop."
            .to_string(),
        ),
      };
      let mut notes = vec![
        format!("query={query}"),
        format!("attemptCount={attempts}"),
        format!("elapsedMs={elapsed_ms}"),
        format!("timeoutMs={timeout_ms}"),
        format!("pollIntervalMs={poll_interval_ms}"),
        format!("timedOut={timed_out}"),
        format!("matchCount={}", ocr_snapshot.matches.len()),
        format!("filteredMatchCount={}", filtered_matches.len()),
        format!("caseSensitive={case_sensitive}"),
        format!("exact={exact}"),
        format!("minConfidence={min_confidence:.3}"),
        format!(
          "screenshotPixels={}x{}",
          ocr_snapshot.image_width, ocr_snapshot.image_height
        ),
        format!("displayRef={}", capture_source.display_ref),
        format!("nativeDisplayId={}", capture_source.native_display_id),
        format!("captureSourceReason={}", capture_source.selection_reason),
      ];
      if let Some(region) = region.as_ref() {
        notes.push(render_ocr_region_note(region));
      }
      if let Some(app) = activated_app {
        notes.push(format!("activatedTargetBeforeCapture={app}"));
      }

      let summary = if let Some(best_match) = filtered_matches.first() {
        let (screenshot_center_x, screenshot_center_y) = ocr_match_center(best_match);
        let (logical_x, logical_y) =
          crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
            &capture_contract,
            screenshot_center_x,
            screenshot_center_y,
          )?;
        notes.push(format!("bestMatchText={}", best_match.text));
        notes.push(format!(
          "bestMatchBounds={}",
          render_rect_compact(&best_match.bounds)
        ));
        notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
        notes.push(format!("bestLogicalPoint={logical_x:.3},{logical_y:.3}"));
        format!(
          "Observed OCR text anchor {} after {} polling attempt(s) over {} ms; best anchor projects to logical point ({logical_x:.3}, {logical_y:.3}).",
          best_match.text, attempts, elapsed_ms
        )
      } else {
        "Timed out while polling the live desktop for a filtered OCR text anchor.".to_string()
      };

      return Ok(DriverResponse {
        summary,
        backend: Some("macos.vision.wait-screen-text".to_string()),
        signals: wait_ocr_detection_signals(
          filtered_matches.len(),
          filtered_matches
            .first()
            .map(|matched| matched.text.as_str()),
          timed_out,
        ),
        notes,
        artifacts: vec![screenshot_artifact, report_artifact],
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(super) fn find_screen_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "screen-rows".to_string());
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let app_bundle_id = app_identifier(call).filter(|value| looks_like_bundle_identifier(value));
  let display_selection = parse_display_selection(call)?;
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let capture_source = resolve_screen_capture_source(&displays, display_selection.as_ref(), None)?;
  let (screenshot_path, _capture_contract) =
    crate::driver::macos::capture::xcap_backend::capture_display_to_path(
      &label,
      Some(&capture_source.display_ref),
      Some(&capture_source.native_display_id),
      false,
    )?;
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
  let detection = detect_screen_rows(
    screenshot_path.as_path(),
    min_confidence,
    max_observations,
    region.as_ref(),
  )?;
  let rows = detection.rows;
  let report_artifact = build_text_artifact(
    "screen-rows-report",
    "txt",
    &format!("screen-rows-report-{}", sanitize_file_component(&label)),
    detection.report,
    "Captured row-detection report used for visible-row grouping (OCR first, then visual-band fallback).",
  )?;

  // Reserve slot 0 for the screenshot so the recognition artifact can cite its
  // ArtifactRef before the screenshot is pushed.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let screenshot_ref = artifacts.ref_at(0);

  let recognition_artifact = row_recognition_artifact(
    "screen-rows-recognition",
    &format!(
      "screen-rows-recognition-{}",
      sanitize_file_component(&label)
    ),
    "Structured recognition result for screen row detection.",
    RowRecognitionArtifactRequest {
      recognition_id: format!("screen_rows_{}", sanitize_file_component(&label)),
      source: recognition_source_for_rows(&detection.strategy, &rows),
      surface: crate::contract::RecognitionSurface::Display,
      rows: &rows,
      strategy: &detection.strategy,
      raw_match_count: detection.raw_match_count,
      filtered_match_count: detection.filtered_match_count,
      screenshot_path: screenshot_path.as_path(),
      screenshot_dimensions: &dimensions,
      display_ref: Some(&capture_source.display_ref),
      native_display_id: Some(&capture_source.native_display_id),
      app_bundle_id: app_bundle_id.as_deref(),
      window_title: None,
      window_number: None,
      region_hint: region
        .as_ref()
        .map(|value| observed_rect_to_ratio_region(value, &dimensions)),
      capture_contract: None,
      capture_artifact: Some(screenshot_ref.clone()),
      additional_detail: serde_json::json!({
        "capture_source_reason": &capture_source.selection_reason,
      }),
      known_limits: Vec::new(),
    },
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR-based visible-row detection.".to_string()),
  };
  let mut notes = vec![
    format!("rowStrategy={}", detection.strategy),
    format!("rowCount={}", rows.len()),
    format!("matchCount={}", detection.raw_match_count),
    format!("filteredMatchCount={}", detection.filtered_match_count),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "screenshotPixels={}x{}",
      dimensions.width, dimensions.height
    ),
    format!("displayRef={}", capture_source.display_ref),
    format!("nativeDisplayId={}", capture_source.native_display_id),
    format!("captureSourceReason={}", capture_source.selection_reason),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }
  for row in rows.iter().take(5) {
    notes.push(render_ocr_row_note(row));
  }

  let summary = if let Some(first_row) = rows.first() {
    let preview = if first_row.text_fragments.is_empty() {
      format!("bounds={}", render_rect_compact(&first_row.bounds))
    } else {
      first_row
        .text_fragments
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ")
    };
    format!(
      "Detected {} visible row(s) with strategy {} in the constrained region; first row preview: {}.",
      rows.len(),
      detection.strategy,
      preview
    )
  } else {
    format!(
      "Detected 0 visible row(s) in the constrained region after strategy {}.",
      detection.strategy
    )
  };

  // Push in slot order: must match `ref_at(0)` reservation.
  artifacts.push(screenshot_artifact);
  artifacts.push(report_artifact);
  artifacts.push(recognition_artifact);

  Ok(DriverResponse {
    summary,
    backend: Some(format!("macos.vision.screen-rows.{}", detection.strategy)),
    signals: row_detection_signals(rows.len()),
    notes,
    artifacts: artifacts.into_vec(),
  })
}

pub(super) fn wait_for_screen_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "screen-rows-wait".to_string());
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
  let min_row_count = optional_i64(call, "min_row_count")?
    .unwrap_or(1)
    .clamp(1, 64) as usize;
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let display_selection = parse_display_selection(call)?;
  let app_bundle_id = app_identifier(call).filter(|value| looks_like_bundle_identifier(value));
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let capture_source = resolve_screen_capture_source(&displays, display_selection.as_ref(), None)?;
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path: Option<PathBuf> = None;

  loop {
    attempts += 1;
    let attempt_label = format!("{label}-attempt-{attempts}");
    let activated_app = maybe_activate_target_app_for_observation(call)?;
    let (screenshot_path, _capture_contract) =
      crate::driver::macos::capture::xcap_backend::capture_display_to_path(
        &attempt_label,
        Some(&capture_source.display_ref),
        Some(&capture_source.native_display_id),
        false,
      )?;
    let dimensions = read_png_dimensions(&screenshot_path)?;
    let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
    let detection = detect_screen_rows(
      screenshot_path.as_path(),
      min_confidence,
      max_observations,
      region.as_ref(),
    )?;
    let rows = detection.rows;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if rows.len() >= min_row_count || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }
      let report_artifact = build_text_artifact(
        "screen-rows-wait-report",
        "txt",
        &format!(
          "screen-rows-wait-report-{}",
          sanitize_file_component(&label)
        ),
        detection.report,
        "Captured row-detection report from the final wait-for-screen-rows polling attempt.",
      )?;

      // Reserve slot 0 for the screenshot so the recognition artifact can cite
      // its ArtifactRef before the screenshot is pushed.
      let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
      let screenshot_ref = artifacts.ref_at(0);

      let recognition_artifact = row_recognition_artifact(
        "screen-rows-wait-recognition",
        &format!(
          "screen-rows-wait-recognition-{}",
          sanitize_file_component(&label)
        ),
        "Structured recognition result from the final wait-for-screen-rows polling attempt.",
        RowRecognitionArtifactRequest {
          recognition_id: format!("screen_rows_wait_{}", sanitize_file_component(&label)),
          source: recognition_source_for_rows(&detection.strategy, &rows),
          surface: crate::contract::RecognitionSurface::Display,
          rows: &rows,
          strategy: &detection.strategy,
          raw_match_count: detection.raw_match_count,
          filtered_match_count: detection.filtered_match_count,
          screenshot_path: screenshot_path.as_path(),
          screenshot_dimensions: &dimensions,
          display_ref: Some(&capture_source.display_ref),
          native_display_id: Some(&capture_source.native_display_id),
          app_bundle_id: app_bundle_id.as_deref(),
          window_title: None,
          window_number: None,
          region_hint: region
            .as_ref()
            .map(|value| observed_rect_to_ratio_region(value, &dimensions)),
          capture_contract: None,
          capture_artifact: Some(screenshot_ref.clone()),
          additional_detail: serde_json::json!({
            "capture_source_reason": &capture_source.selection_reason,
            "timed_out": timed_out,
            "attempt_count": attempts,
          }),
          known_limits: Vec::new(),
        },
      )?;
      let screenshot_artifact = ProducedArtifact {
        kind: "screenshot".to_string(),
        source_path: screenshot_path,
        preferred_name: format!("{}.png", sanitize_file_component(&label)),
        note: Some(
          "Final screenshot retained from waitForScreenRows polling over the live desktop."
            .to_string(),
        ),
      };
      let mut notes = vec![
        format!("rowStrategy={}", detection.strategy),
        format!("rowCount={}", rows.len()),
        format!("requiredRowCount={min_row_count}"),
        format!("attemptCount={attempts}"),
        format!("elapsedMs={elapsed_ms}"),
        format!("timeoutMs={timeout_ms}"),
        format!("pollIntervalMs={poll_interval_ms}"),
        format!("timedOut={timed_out}"),
        format!("matchCount={}", detection.raw_match_count),
        format!("filteredMatchCount={}", detection.filtered_match_count),
        format!("minConfidence={min_confidence:.3}"),
        format!(
          "screenshotPixels={}x{}",
          dimensions.width, dimensions.height
        ),
        format!("displayRef={}", capture_source.display_ref),
        format!("nativeDisplayId={}", capture_source.native_display_id),
        format!("captureSourceReason={}", capture_source.selection_reason),
      ];
      if let Some(region) = region.as_ref() {
        notes.push(render_ocr_region_note(region));
      }
      if let Some(app) = activated_app {
        notes.push(format!("activatedTargetBeforeCapture={app}"));
      }
      for row in rows.iter().take(5) {
        notes.push(render_ocr_row_note(row));
      }

      let summary = if rows.len() >= min_row_count {
        format!(
          "Observed {} visible row(s) with strategy {} after {} polling attempt(s) over {} ms.",
          rows.len(),
          detection.strategy,
          attempts,
          elapsed_ms
        )
      } else {
        format!(
          "Timed out while polling the live desktop for visible rows after strategy {}.",
          detection.strategy
        )
      };

      // Push in slot order: must match `ref_at(0)` reservation.
      artifacts.push(screenshot_artifact);
      artifacts.push(report_artifact);
      artifacts.push(recognition_artifact);

      return Ok(DriverResponse {
        summary,
        backend: Some(format!(
          "macos.vision.wait-screen-rows.{}",
          detection.strategy
        )),
        signals: wait_row_detection_signals(rows.len(), min_row_count, timed_out),
        notes,
        artifacts: artifacts.into_vec(),
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(super) fn find_image_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let image_path = PathBuf::from(required_non_empty_string(call, "image_path")?);
  if !image_path.exists() {
    return Err(format!(
      "image_path does not exist: {}",
      image_path.display()
    ));
  }

  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let dimensions = read_png_dimensions(&image_path)?;
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let ocr_capture = crate::driver::macos::native::ocr::find_text(
    image_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  )?;
  let ocr_report = crate::driver::macos::native::ocr::render_ocr_text_report(&ocr_capture);
  let ocr_snapshot = ocr_capture.snapshot;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref());
  let report_artifact = build_text_artifact(
    "image-text-report",
    "txt",
    &format!("image-text-report-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report for a provided image artifact.",
  )?;

  let mut notes = vec![
    format!("query={query}"),
    format!("imagePath={}", image_path.display()),
    format!("matchCount={}", ocr_snapshot.matches.len()),
    format!("filteredMatchCount={}", filtered_matches.len()),
    format!("caseSensitive={case_sensitive}"),
    format!("exact={exact}"),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "imagePixels={}x{}",
      ocr_snapshot.image_width, ocr_snapshot.image_height
    ),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }

  let summary = if let Some(best_match) = filtered_matches.first() {
    notes.push(format!("bestMatchText={}", best_match.text));
    notes.push(format!(
      "bestMatchBounds={}",
      render_rect_compact(&best_match.bounds)
    ));
    notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
    format!(
      "Found {} OCR text match(es) for query {} inside the provided image after filtering; best anchor is {}.",
      filtered_matches.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches in the provided image after applying the active filters.".to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.image-text".to_string()),
    signals: ocr_detection_signals(
      filtered_matches.len(),
      filtered_matches
        .first()
        .map(|matched| matched.text.as_str()),
    ),
    notes,
    artifacts: vec![report_artifact],
  })
}

pub(super) fn identify_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y);
  let report = render_point_identification_report(&snapshot, x, y, resolution.as_ref());
  let label = format!(
    "point-{}-{}",
    sanitize_file_component(&format!("{x:.3}")),
    sanitize_file_component(&format!("{y:.3}"))
  );
  let artifact = build_text_artifact(
    "point-resolution",
    "txt",
    &label,
    report,
    "Captured macOS point-to-display resolution report from the observation driver.",
  )?;

  let mut notes = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];
  let summary = if let Some(resolution) = resolution {
    notes.push(render_display_note(&resolution.display));
    notes.push(format!(
      "localPoint={:.3},{:.3}",
      resolution.local_x, resolution.local_y
    ));
    notes.push(format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ));
    let role = if resolution.display.is_main {
      "main"
    } else {
      "secondary"
    };
    format!(
      "Point ({x:.3}, {y:.3}) is on {role} display #{}; local=({:.3}, {:.3}), backingPixel=({}, {}).",
      resolution.display.display_id,
      resolution.local_x,
      resolution.local_y,
      resolution.backing_pixel_x,
      resolution.backing_pixel_y
    )
  } else {
    format!("Point ({x:.3}, {y:.3}) is outside all connected macOS displays.")
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.desktop.display-point".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}

pub(super) fn probe_permissions(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let native_permissions = crate::driver::macos::native::permission::probe_native_permissions()?;
  let screen_recording = native_permissions.screen_recording.to_string();
  let accessibility = native_permissions.accessibility.to_string();
  let automation = probe_automation_to_system_events();
  let launch_host = launch_host_process();

  let report =
    permission_probe_report(&screen_recording, &accessibility, &automation, &launch_host);

  let artifact = build_text_artifact(
    "probe-permissions",
    "txt",
    "permission-report",
    report.clone(),
    "Captured macOS permission probe report from the desktop driver.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Permission probe: screenRecording={}, accessibility={}, automationToSystemEvents={}.",
      screen_recording, accessibility, automation
    ),
    backend: Some("macos.swift-and-osascript".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: report.lines().map(str::to_string).collect(),
    artifacts: vec![artifact],
  })
}

#[cfg(test)]
mod tests {
  use super::{
    ObservedAxNode, ObservedRect, VERIFY_AX_TEXT_OPERATION_ID,
    VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID, build_verify_ax_text_operation_result,
    build_verify_ax_text_verification, build_verify_now_playing_title_operation_result,
    build_verify_now_playing_title_verification, ocr_detection_signals, permission_probe_report,
    preferred_ax_signal_text, row_detection_signals, verify_ax_text_signals,
    verify_now_playing_title_signals, wait_ocr_detection_signals, wait_row_detection_signals,
  };
  use crate::contract::{ArtifactRef, OperationOutput, OperationStatus, VerificationMethod};
  use crate::model::{DriverCall, DriverRunContext, ExecutionTarget};
  use crate::trace::{ArtifactId, RunId, SpanId};
  use std::collections::BTreeMap;

  #[test]
  fn verify_now_playing_title_signals_include_title_and_presence() {
    let signals = verify_now_playing_title_signals("晴天");

    assert_eq!(signals.get("ax.node_found"), Some(&"true".to_string()));
    assert_eq!(
      signals.get("ax.now_playing_title"),
      Some(&"晴天".to_string())
    );
  }

  #[test]
  fn verify_ax_text_signals_include_presence_and_match_text() {
    let signals = verify_ax_text_signals("已粘贴完成", "AXTextArea");

    assert_eq!(signals.get("ax.node_found"), Some(&"true".to_string()));
    assert_eq!(
      signals.get("ax.matched_text"),
      Some(&"已粘贴完成".to_string())
    );
    assert_eq!(
      signals.get("ax.matched_role"),
      Some(&"AXTextArea".to_string())
    );
  }

  #[test]
  fn permission_probe_report_preserves_contract_fields() {
    let report = permission_probe_report("granted", "missing", "granted", "current-process");

    assert_eq!(
      report,
      "screenRecording=granted\naccessibility=missing\nautomationToSystemEvents=granted\nlaunchHostProcess=current-process\n"
    );
  }

  #[test]
  fn ocr_detection_signals_include_match_presence_and_best_text() {
    let signals = ocr_detection_signals(2, Some("晴天"));
    let empty = ocr_detection_signals(0, None);

    assert_eq!(signals.get("ocr.match_found"), Some(&"true".to_string()));
    assert_eq!(
      signals.get("ocr.filtered_match_count"),
      Some(&"2".to_string())
    );
    assert_eq!(
      signals.get("ocr.best_match_text"),
      Some(&"晴天".to_string())
    );
    assert_eq!(empty.get("ocr.match_found"), Some(&"false".to_string()));
    assert_eq!(
      empty.get("ocr.filtered_match_count"),
      Some(&"0".to_string())
    );
    assert!(!empty.contains_key("ocr.best_match_text"));
  }

  #[test]
  fn wait_ocr_detection_signals_include_timeout_status() {
    let signals = wait_ocr_detection_signals(1, Some("播放"), false);

    assert_eq!(signals.get("ocr.match_found"), Some(&"true".to_string()));
    assert_eq!(signals.get("ocr.timed_out"), Some(&"false".to_string()));
  }

  #[test]
  fn row_detection_signals_report_count_and_visibility() {
    let empty = row_detection_signals(0);
    let visible = row_detection_signals(3);

    assert_eq!(empty.get("rows.count"), Some(&"0".to_string()));
    assert_eq!(empty.get("rows.visible"), Some(&"false".to_string()));
    assert_eq!(visible.get("rows.count"), Some(&"3".to_string()));
    assert_eq!(visible.get("rows.visible"), Some(&"true".to_string()));
  }

  #[test]
  fn wait_row_detection_signals_include_requirement_state() {
    let ready = wait_row_detection_signals(3, 2, false);
    let timed_out = wait_row_detection_signals(1, 2, true);

    assert_eq!(ready.get("rows.requirement_met"), Some(&"true".to_string()));
    assert_eq!(ready.get("rows.timed_out"), Some(&"false".to_string()));
    assert_eq!(
      timed_out.get("rows.requirement_met"),
      Some(&"false".to_string())
    );
    assert_eq!(timed_out.get("rows.timed_out"), Some(&"true".to_string()));
  }

  #[test]
  fn preferred_ax_signal_text_prefers_value_then_fallback_fields() {
    let node = ObservedAxNode {
      depth: 1,
      path: "0.1".to_string(),
      role: "AXStaticText".to_string(),
      subrole: String::new(),
      title: "Title".to_string(),
      description: "Description".to_string(),
      help: "Help".to_string(),
      identifier: String::new(),
      placeholder: "Placeholder".to_string(),
      value: "Value".to_string(),
      bounds: ObservedRect {
        x: 0,
        y: 0,
        width: 100,
        height: 20,
      },
    };
    assert_eq!(preferred_ax_signal_text(&node), "Value");

    let fallback = ObservedAxNode {
      value: "   ".to_string(),
      title: String::new(),
      description: "Description".to_string(),
      help: String::new(),
      placeholder: String::new(),
      ..node
    };
    assert_eq!(preferred_ax_signal_text(&fallback), "Description");
  }

  fn sample_matched_node() -> ObservedAxNode {
    ObservedAxNode {
      depth: 4,
      path: "AXApplication/AXWindow[0]/AXGroup[1]/AXStaticText".to_string(),
      role: "AXStaticText".to_string(),
      subrole: String::new(),
      title: "Title".to_string(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: String::new(),
      value: "已粘贴完成".to_string(),
      bounds: ObservedRect {
        x: 100,
        y: 200,
        width: 240,
        height: 28,
      },
    }
  }

  fn sample_driver_call() -> DriverCall {
    DriverCall {
      operation: "verify_ax_text".to_string(),
      target: ExecutionTarget {
        application_id: Some("com.example.notes".to_string()),
      },
      inputs: BTreeMap::new(),
      working_directory: std::path::PathBuf::from("."),
      run_context: DriverRunContext {
        run_id: "run_verify_ax_text".to_string(),
        span_id: "span_verify_ax_text".to_string(),
        device_id: "local".to_string(),
        session_id: "default".to_string(),
      },
    }
  }

  fn sample_report_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_verify_ax_text"),
      artifact_id: ArtifactId::new("artifact_0001"),
      span_id: SpanId::new("span_verify_ax_text"),
      captured_event_id: None,
    }
  }

  #[test]
  fn verify_ax_text_verification_records_ax_text_method_with_observed_label() {
    let matched = sample_matched_node();
    let evidence = vec![sample_report_ref()];

    let verification = build_verify_ax_text_verification(&matched, evidence.clone());

    assert_eq!(verification.method, VerificationMethod::AxText);
    assert!(verification.executed);
    assert!(verification.state_changed);
    assert_eq!(verification.semantic_matched, Some(true));
    assert!(verification.failure_layer.is_none());
    assert_eq!(verification.evidence, evidence);
    assert_eq!(
      verification.observed_label.as_deref(),
      Some("已粘贴完成"),
      "observed_label must surface the node's preferred display text"
    );
  }

  #[test]
  fn verify_ax_text_verification_omits_observed_label_when_node_has_no_text() {
    let blank = ObservedAxNode {
      value: String::new(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      placeholder: String::new(),
      ..sample_matched_node()
    };

    let verification = build_verify_ax_text_verification(&blank, Vec::new());

    assert!(
      verification.observed_label.is_none(),
      "no display text should map to None, not an empty string"
    );
  }

  #[test]
  fn verify_ax_text_operation_result_promotes_claim_to_top_level_verifications() {
    let call = sample_driver_call();
    let verification =
      build_verify_ax_text_verification(&sample_matched_node(), vec![sample_report_ref()]);

    let result = build_verify_ax_text_operation_result(&call, verification.clone());

    assert_eq!(result.operation_id, VERIFY_AX_TEXT_OPERATION_ID);
    assert_eq!(result.status, OperationStatus::Completed);
    assert!(matches!(
      result.output,
      OperationOutput::Acknowledged { .. }
    ));
    assert_eq!(
      result.verifications,
      vec![verification.clone()],
      "verify.axText must populate the first-class verifications field"
    );
    assert_eq!(
      result.evidence_artifacts, verification.evidence,
      "evidence_artifacts must mirror the verification's evidence list"
    );
  }

  #[test]
  fn verify_now_playing_title_verification_records_semantic_match_with_observed_label() {
    let matched = sample_matched_node();
    let evidence = vec![sample_report_ref()];

    let verification =
      build_verify_now_playing_title_verification(&matched, evidence.clone());

    assert_eq!(verification.method, VerificationMethod::SemanticMatch);
    assert!(verification.executed);
    assert!(verification.state_changed);
    assert_eq!(verification.semantic_matched, Some(true));
    assert!(verification.failure_layer.is_none());
    assert_eq!(verification.evidence, evidence);
    assert_eq!(
      verification.observed_label.as_deref(),
      Some("已粘贴完成"),
      "observed_label must surface the matched node's preferred display text"
    );
  }

  #[test]
  fn verify_now_playing_title_verification_omits_observed_label_when_node_has_no_text() {
    let blank = ObservedAxNode {
      value: String::new(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      placeholder: String::new(),
      ..sample_matched_node()
    };

    let verification = build_verify_now_playing_title_verification(&blank, Vec::new());

    assert!(
      verification.observed_label.is_none(),
      "no display text should map to None, not an empty string"
    );
  }

  #[test]
  fn verify_now_playing_title_operation_result_promotes_claim_to_top_level_verifications() {
    let call = sample_driver_call();
    let verification = build_verify_now_playing_title_verification(
      &sample_matched_node(),
      vec![sample_report_ref()],
    );

    let result =
      build_verify_now_playing_title_operation_result(&call, verification.clone());

    assert_eq!(result.operation_id, VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID);
    assert_eq!(result.status, OperationStatus::Completed);
    assert!(matches!(
      result.output,
      OperationOutput::Acknowledged { .. }
    ));
    assert_eq!(
      result.verifications,
      vec![verification.clone()],
      "verify.musicNowPlaying must populate the first-class verifications field"
    );
    assert_eq!(
      result.evidence_artifacts, verification.evidence,
      "evidence_artifacts must mirror the verification's evidence list"
    );
  }
}
