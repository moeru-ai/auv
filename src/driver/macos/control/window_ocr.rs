// File: src/driver/macos/control/window_ocr.rs
//! Window-scoped OCR observation + click primitives.
//!
//! Implements commands that capture a resolved window, run Vision OCR (text or
//! row detection), and optionally click resolved anchors/rows in projected
//! logical coordinates.
//!
//! Boundary: OCR anchors are heuristic evidence in pixel space; callers should
//! pair with verification/liveness checks when using the results for actions.

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use super::super::overlay::overlay_click_point;
use super::super::support::{
  artifacts::{
    DriverArtifactBuilder, build_text_artifact, looks_like_bundle_identifier,
    sanitize_file_component, screenshot_temp_path,
  },
  call::{
    app_identifier, optional_bool, optional_f64, optional_i64, optional_non_empty_string,
    optional_positive_u64, optional_string, required_non_empty_string,
  },
  geometry::{ocr_match_center, render_rect_compact},
  ocr::{detect_screen_rows, parse_ocr_region_constraint, render_ocr_row_note},
  ocr_commands::{
    CapturedObservation, render_text_match_command_json, run_text_match_on_capture,
    screenshot_artifact,
  },
  overlay_evidence::{
    OverlayEvidenceMatch, OverlayEvidenceRequest, OverlayEvidenceRow,
    build_overlay_evidence_artifacts, capture_pixel_to_logical, logical_to_capture_pixel,
  },
  recognition::{
    RowRecognitionArtifactRequest, observed_rect_to_ratio_region, recognition_source_for_rows,
    row_recognition_artifact, window_number_from_ref,
  },
  typed_capture::capture_window_with_typed_session,
};
use super::super::{
  DetectedScreenRows, DriverCall, DriverResponse, ObservedOcrRow, OcrTextMatch, OcrTextSnapshot,
};
use super::common::{ClickPointCallOptions, build_click_point_call, resolve_click_interval_ms};
use super::pointer::click_point;
use crate::contract::{Candidate, TargetGrounding};
use crate::model::AuvResult;

pub(super) fn click_window_text_signals(
  text: &str,
  candidate_local_id: Option<&str>,
) -> std::collections::BTreeMap<String, String> {
  let mut signals =
    std::collections::BTreeMap::from([("click.resolved_text".to_string(), text.to_string())]);
  match candidate_local_id {
    Some(candidate_local_id) => {
      signals.insert(
        "clickWindowText.consumer".to_string(),
        "contract-candidate".to_string(),
      );
      signals.insert(
        "clickWindowText.candidateLocalId".to_string(),
        candidate_local_id.to_string(),
      );
    }
    None => {
      signals.insert("clickWindowText.consumer".to_string(), "query".to_string());
    }
  }
  signals
}

pub(super) fn click_window_row_signals(
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

pub(crate) fn find_window_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("window-text-{}", sanitize_file_component(&query));
  let capture = capture_resolved_window_observation(call, &label)?;
  let (snapshot, filtered, ocr_report, command_report) =
    run_text_match_on_capture(call, &capture, &query)?;
  let report_artifact = build_text_artifact(
    "window-text-report",
    "txt",
    &format!("window-text-report-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report for a resolved app window.",
  )?;
  let json_artifact = command_report
    .as_ref()
    .map(|report| {
      build_text_artifact(
        "window-text-report",
        "json",
        &format!("window-text-report-{}", sanitize_file_component(&query)),
        render_text_match_command_json(report)?,
        "Machine-readable window OCR text-anchor command report.",
      )
    })
    .transpose()?;
  let screenshot_artifact = screenshot_artifact(&capture, &label, "window text-anchor detection");
  let mut notes = text_notes(&capture, &query, &snapshot, filtered.len());
  let summary = if let Some(best_match) = filtered.first() {
    let (logical_x, logical_y) = logical_point_for_match(&capture, best_match)?;
    notes.push(format!("bestMatchText={}", best_match.text));
    notes.push(format!(
      "bestMatchBounds={}",
      render_rect_compact(&best_match.bounds)
    ));
    notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
    notes.push(format!("bestLogicalPoint={logical_x:.3},{logical_y:.3}"));
    format!(
      "Found {} OCR text match(es) for query {} inside the resolved window; best anchor {} projects to logical point ({logical_x:.3}, {logical_y:.3}).",
      filtered.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches inside the resolved window after applying the active filters."
      .to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.window-text".to_string()),
    signals: crate::driver::macos::observe::ocr_detection_signals(
      filtered.len(),
      filtered.first().map(|matched| matched.text.as_str()),
    ),
    notes,
    artifacts: match json_artifact {
      Some(json_artifact) => vec![screenshot_artifact, report_artifact, json_artifact],
      None => vec![screenshot_artifact, report_artifact],
    },
  })
}

pub(crate) fn wait_for_window_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("window-text-wait-{}", sanitize_file_component(&query));
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path = None;

  loop {
    attempts += 1;
    let capture =
      capture_resolved_window_observation(call, &format!("{label}-attempt-{attempts}"))?;
    let (snapshot, filtered, ocr_report, command_report) =
      run_text_match_on_capture(call, &capture, &query)?;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if !filtered.is_empty() || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }
      let report_artifact = build_text_artifact(
        "window-text-wait-report",
        "txt",
        &format!(
          "window-text-wait-report-{}",
          sanitize_file_component(&query)
        ),
        ocr_report,
        "Captured Vision OCR text-anchor report from the final wait-for-window-text polling attempt.",
      )?;
      let json_artifact = command_report
        .as_ref()
        .map(|report| {
          build_text_artifact(
            "window-text-wait-report",
            "json",
            &format!(
              "window-text-wait-report-{}",
              sanitize_file_component(&query)
            ),
            render_text_match_command_json(report)?,
            "Machine-readable window OCR wait command report.",
          )
        })
        .transpose()?;
      let screenshot_artifact =
        screenshot_artifact(&capture, &label, "final waitForWindowText polling attempt");
      let mut notes = text_notes(&capture, &query, &snapshot, filtered.len());
      notes.push(format!("attemptCount={attempts}"));
      notes.push(format!("elapsedMs={elapsed_ms}"));
      notes.push(format!("timeoutMs={timeout_ms}"));
      notes.push(format!("pollIntervalMs={poll_interval_ms}"));
      notes.push(format!("timedOut={timed_out}"));

      let summary = if let Some(best_match) = filtered.first() {
        let (logical_x, logical_y) = logical_point_for_match(&capture, best_match)?;
        notes.push(format!("bestMatchText={}", best_match.text));
        notes.push(format!("bestLogicalPoint={logical_x:.3},{logical_y:.3}"));
        format!(
          "Observed OCR text anchor {} in the resolved window after {} polling attempt(s) over {} ms.",
          best_match.text, attempts, elapsed_ms
        )
      } else {
        "Timed out while polling the resolved window for a filtered OCR text anchor.".to_string()
      };

      return Ok(DriverResponse {
        summary,
        backend: Some("macos.vision.wait-window-text".to_string()),
        signals: crate::driver::macos::observe::wait_ocr_detection_signals(
          filtered.len(),
          filtered.first().map(|matched| matched.text.as_str()),
          timed_out,
        ),
        notes,
        artifacts: match json_artifact {
          Some(json_artifact) => vec![screenshot_artifact, report_artifact, json_artifact],
          None => vec![screenshot_artifact, report_artifact],
        },
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(capture.screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(crate) fn click_window_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let candidate_json = optional_non_empty_string(call, "candidate");
  let resolved_candidate = candidate_json
    .as_deref()
    .map(parse_window_text_candidate)
    .transpose()?;
  let query = if let Some(candidate) = resolved_candidate.as_ref() {
    candidate.query.clone()
  } else {
    required_non_empty_string(call, "query")?
  };
  let label = format!("window-text-click-{}", sanitize_file_component(&query));
  let capture = capture_resolved_window_observation(call, &label)?;
  if let Some(candidate) = resolved_candidate.as_ref() {
    ensure_window_text_candidate_matches_capture(candidate, &capture)?;
  }
  let effective_call = build_window_text_effective_call(call, &query, resolved_candidate.as_ref());
  let (snapshot, filtered, ocr_report, command_report) =
    run_text_match_on_capture(&effective_call, &capture, &query)?;
  let match_index = resolved_candidate
    .as_ref()
    .and_then(|candidate| candidate.match_index)
    .unwrap_or(optional_i64(call, "match_index")?.unwrap_or(0).max(0) as usize);
  let matched = filtered.get(match_index).ok_or_else(|| {
    format!(
      "no filtered OCR text match at index {} for query {} inside resolved window; inspect `window.findText`",
      match_index, query
    )
  })?;
  let anchor_offset_x = optional_f64(call, "anchor_offset_x")?.unwrap_or(0.0);
  let anchor_offset_y = optional_f64(call, "anchor_offset_y")?.unwrap_or(0.0);
  let (sx, sy) = ocr_match_center(matched);
  let (logical_x, logical_y) =
    crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture.capture_contract,
      sx + anchor_offset_x,
      sy + anchor_offset_y,
    )?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let overlay = optional_bool(call, "overlay")?.unwrap_or(false);
  let overlay_label =
    optional_non_empty_string(call, "label").unwrap_or_else(|| "auv · replay".to_string());
  let preview_ms =
    optional_positive_u64(call, "preview_ms")?.unwrap_or(if overlay { 250 } else { 0 });
  let mut nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
    call.run_context.clone(),
    ClickPointCallOptions {
      x: logical_x,
      y: logical_y,
      button: &button_label,
      click_count,
      click_interval_ms: Some(click_interval_ms),
      settle_ms: Some(settle_ms),
      app: call.target.application_id.as_deref(),
    },
  );
  if overlay {
    nested_call.operation = "overlay_click_point".to_string();
    nested_call
      .inputs
      .insert("label".to_string(), overlay_label.clone());
    nested_call
      .inputs
      .insert("preview_ms".to_string(), preview_ms.to_string());
    let _ = overlay_click_point(&nested_call)?;
  } else {
    let _ = click_point(&nested_call)?;
  }

  let report_artifact = build_text_artifact(
    "window-text-click",
    "txt",
    &format!("window-text-click-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report before clicking a resolved app window.",
  )?;
  let json_artifact = command_report
    .as_ref()
    .map(|report| {
      build_text_artifact(
        "window-text-click",
        "json",
        &format!("window-text-click-{}", sanitize_file_component(&query)),
        render_text_match_command_json(report)?,
        "Machine-readable window OCR click command report.",
      )
    })
    .transpose()?;
  let screenshot_artifact = screenshot_artifact(&capture, &label, "window text click detection");
  let mut overlay_artifacts = build_overlay_evidence_artifacts(OverlayEvidenceRequest {
    kind: "window-text-click",
    label: label.clone(),
    screenshot_path: screenshot_artifact.source_path.clone(),
    screenshot_dimensions: capture.dimensions.clone(),
    capture_contract: capture.capture_contract.clone(),
    query: Some(query.clone()),
    strategy: Some("ocr-text".to_string()),
    fallback_used: None,
    cursor_disturbance: Some("warp-visible".to_string()),
    press_mechanism: Some("pointer-click".to_string()),
    overlay_presentation: overlay.then_some("dual-cursor-visual-only".to_string()),
    action_point: logical_to_capture_pixel(&capture.capture_contract, logical_x, logical_y),
    expected_target: Some(capture_pixel_to_logical(
      &capture.capture_contract,
      sx + anchor_offset_x,
      sy + anchor_offset_y,
    )),
    ocr_match: Some(OverlayEvidenceMatch {
      text: matched.text.clone(),
      confidence: matched.confidence,
      bounds: matched.bounds.clone(),
    }),
    row: None,
    ax_target: None,
    decision: None,
    include_user_cursor: overlay,
    auv_cursor_variant: "auv-click",
  })?;
  let mut notes = text_notes(&capture, &query, &snapshot, filtered.len());
  if let Some(candidate) = resolved_candidate.as_ref() {
    notes.push(format!(
      "consumedCandidateLocalId={}",
      candidate.candidate_local_id
    ));
  }
  notes.extend([
    format!("matchIndex={match_index}"),
    format!("matchText={}", matched.text),
    format!("matchBounds={}", render_rect_compact(&matched.bounds)),
    format!("anchorOffset={anchor_offset_x:.3},{anchor_offset_y:.3}"),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("button={button_label}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
    "pressMechanism=pointer-click".to_string(),
    "cursorDisturbance=warp-visible".to_string(),
  ]);
  if overlay {
    notes.push("overlayPresentation=dual-cursor-visual-only".to_string());
    notes.push("userCursorSource=current-hardware-cursor".to_string());
    notes.push(format!("overlayLabel={overlay_label}"));
    notes.push(format!("previewMs={preview_ms}"));
  }

  let mut signals = click_window_text_signals(
    &matched.text,
    resolved_candidate
      .as_ref()
      .map(|candidate| candidate.candidate_local_id.as_str()),
  );
  signals.insert("pressMechanism".to_string(), "pointer-click".to_string());
  signals.insert("cursorDisturbance".to_string(), "warp-visible".to_string());
  if overlay {
    signals.insert(
      "overlayPresentation".to_string(),
      "dual-cursor-visual-only".to_string(),
    );
    signals.insert("dualCursor".to_string(), "true".to_string());
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked OCR text anchor {} for query {} inside the resolved window at logical point ({logical_x:.3}, {logical_y:.3}).",
      matched.text, query
    ),
    backend: Some("macos.vision.click-window-text".to_string()),
    signals,
    notes,
    artifacts: match json_artifact {
      Some(json_artifact) => {
        let mut artifacts = vec![screenshot_artifact, report_artifact, json_artifact];
        artifacts.append(&mut overlay_artifacts);
        artifacts
      }
      None => {
        let mut artifacts = vec![screenshot_artifact, report_artifact];
        artifacts.append(&mut overlay_artifacts);
        artifacts
      }
    },
  })
}

#[derive(Clone, Debug, PartialEq)]
struct ResolvedWindowTextCandidate {
  candidate_local_id: String,
  query: String,
  expected_window: Option<crate::contract::WindowRefPrecondition>,
  region_hint: Option<crate::contract::RatioRegion>,
  min_provider_score: Option<f64>,
  match_index: Option<usize>,
}

fn parse_window_text_candidate(raw_candidate: &str) -> AuvResult<ResolvedWindowTextCandidate> {
  let candidate: Candidate = serde_json::from_str(raw_candidate)
    .map_err(|error| format!("invalid --candidate JSON: {error}"))?;
  if candidate.target_spec.grounding != TargetGrounding::OcrAnchor {
    return Err(format!(
      "click_window_text only accepts OcrAnchor candidates; got {:?}",
      candidate.target_spec.grounding
    ));
  }
  let query = candidate
    .target_spec
    .anchor_text
    .as_deref()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .ok_or_else(|| "click_window_text candidate is missing target_spec.anchor_text".to_string())?;
  let query_detail = candidate
    .evidence
    .observation
    .get("query")
    .and_then(|value| value.as_object());
  let ocr_detail = query_detail
    .and_then(|query| query.get("ocr"))
    .and_then(|value| value.as_object());
  let min_provider_score = ocr_detail
    .and_then(|detail| detail.get("min_provider_score"))
    .and_then(|value| value.as_f64());
  let match_index = candidate
    .evidence
    .observation
    .get("match_index")
    .and_then(|value| value.as_u64())
    .and_then(|value| usize::try_from(value).ok());

  Ok(ResolvedWindowTextCandidate {
    candidate_local_id: candidate.candidate_local_id,
    query,
    expected_window: candidate.liveness.preconditions.window_ref,
    region_hint: ocr_detail
      .and_then(|detail| detail.get("region_hint"))
      .cloned()
      .and_then(|value| serde_json::from_value(value).ok())
      .or(
        candidate
          .liveness
          .preconditions
          .anchor_recheck
          .and_then(|recheck| recheck.region_hint),
      ),
    min_provider_score,
    match_index,
  })
}

fn build_window_text_effective_call(
  call: &DriverCall,
  query: &str,
  candidate: Option<&ResolvedWindowTextCandidate>,
) -> DriverCall {
  let mut effective_call = call.clone();
  effective_call
    .inputs
    .insert("query".to_string(), query.to_string());

  if let Some(candidate) = candidate {
    if !effective_call.inputs.contains_key("min_confidence")
      && let Some(min_provider_score) = candidate.min_provider_score
    {
      effective_call.inputs.insert(
        "min_confidence".to_string(),
        format!("{min_provider_score:.6}"),
      );
    }
    if !effective_call.inputs.contains_key("match_index")
      && let Some(match_index) = candidate.match_index
    {
      effective_call
        .inputs
        .insert("match_index".to_string(), match_index.to_string());
    }
    if !has_explicit_region_constraint(call)
      && let Some(region_hint) = candidate.region_hint
    {
      effective_call.inputs.insert(
        "region_left_ratio".to_string(),
        format!("{:.6}", region_hint.left),
      );
      effective_call.inputs.insert(
        "region_top_ratio".to_string(),
        format!("{:.6}", region_hint.top),
      );
      effective_call.inputs.insert(
        "region_right_ratio".to_string(),
        format!("{:.6}", region_hint.right),
      );
      effective_call.inputs.insert(
        "region_bottom_ratio".to_string(),
        format!("{:.6}", region_hint.bottom),
      );
    }
  }

  effective_call
}

fn has_explicit_region_constraint(call: &DriverCall) -> bool {
  [
    "region_left_ratio",
    "region_top_ratio",
    "region_right_ratio",
    "region_bottom_ratio",
  ]
  .iter()
  .any(|key| optional_non_empty_string(call, key).is_some())
}

fn ensure_window_text_candidate_matches_capture(
  candidate: &ResolvedWindowTextCandidate,
  capture: &CapturedObservation,
) -> AuvResult<()> {
  let Some(expected_window) = candidate.expected_window.as_ref() else {
    return Ok(());
  };

  if !expected_window.app_bundle_id.trim().is_empty() {
    let expected_bundle = expected_window.app_bundle_id.trim();
    let actual_bundle = capture.owner_bundle_id.as_deref().unwrap_or_default();
    if !actual_bundle.trim().is_empty() && !actual_bundle.eq_ignore_ascii_case(expected_bundle) {
      return Err(format!(
        "click_window_text candidate {} expected app bundle {} but resolved window belonged to {}",
        candidate.candidate_local_id, expected_bundle, actual_bundle
      ));
    }
  }

  if let Some(expected_title) = expected_window.window_title_substring.as_deref() {
    let expected_title = expected_title.trim();
    let actual_title = capture.window_title.as_deref().unwrap_or_default();
    if !expected_title.is_empty()
      && (actual_title.is_empty() || !actual_title.contains(expected_title))
    {
      return Err(format!(
        "click_window_text candidate {} expected window title containing {:?} but resolved window title was {:?}",
        candidate.candidate_local_id, expected_title, actual_title
      ));
    }
  }

  if let Some(expected_window_number) = expected_window.window_number {
    let actual_window_number = window_number_from_ref(&capture.capture_source)
      .ok_or_else(|| {
        format!(
          "click_window_text candidate {} expected window number {} but capture source had no window number",
          candidate.candidate_local_id, expected_window_number
        )
      })?;
    if actual_window_number != expected_window_number {
      return Err(format!(
        "click_window_text candidate {} expected window number {} but resolved {}",
        candidate.candidate_local_id, expected_window_number, actual_window_number
      ));
    }
  }

  Ok(())
}

pub(crate) fn find_window_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-rows".to_string());
  let app_bundle_id = app_identifier(call).filter(|value| looks_like_bundle_identifier(value));
  let capture = capture_resolved_window_observation(call, &label)?;
  let (detection, rows) = detect_rows_for_capture(call, &capture)?;
  let region =
    parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;
  let report_artifact = build_text_artifact(
    "window-rows-report",
    "txt",
    &format!("window-rows-report-{}", sanitize_file_component(&label)),
    detection.report.clone(),
    "Captured row-detection report for a resolved app window.",
  )?;
  let (display_ref, native_display_id) = match &capture.capture_contract.capture_source {
    crate::driver::macos::capture::types::CaptureSource::Window {
      display_ref,
      native_display_id,
      ..
    } => (Some(display_ref.as_str()), Some(native_display_id.as_str())),
    _ => (None, None),
  };

  // Reserve slot 0 for the screenshot so the recognition artifact can cite its
  // ArtifactRef before the screenshot itself is pushed.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let screenshot_ref = artifacts.ref_at(0);

  let recognition_artifact = row_recognition_artifact(
    "window-rows-recognition",
    &format!(
      "window-rows-recognition-{}",
      sanitize_file_component(&label)
    ),
    "Structured recognition result for window row detection.",
    RowRecognitionArtifactRequest {
      recognition_id: format!("window_rows_{}", sanitize_file_component(&label)),
      source: recognition_source_for_rows(&detection.strategy, &rows),
      surface: crate::contract::RecognitionSurface::Window,
      rows: &rows,
      strategy: &detection.strategy,
      raw_match_count: detection.raw_match_count,
      filtered_match_count: detection.filtered_match_count,
      screenshot_path: capture.screenshot_path.as_path(),
      screenshot_dimensions: &capture.dimensions,
      display_ref,
      native_display_id,
      app_bundle_id: app_bundle_id.as_deref(),
      window_title: None,
      window_number: window_number_from_ref(&capture.capture_source),
      region_hint: region
        .as_ref()
        .map(|value| observed_rect_to_ratio_region(value, &capture.dimensions)),
      capture_contract: Some(&capture.capture_contract),
      capture_artifact: Some(screenshot_ref.clone()),
      additional_detail: serde_json::json!({
        "scope": &capture.scope,
        "capture_source": &capture.capture_source,
      }),
      known_limits: Vec::new(),
    },
  )?;
  let screenshot_artifact = screenshot_artifact(&capture, &label, "window row detection");
  let mut notes = row_notes(&capture, &detection, rows.len());
  for row in rows.iter().take(5) {
    notes.push(render_ocr_row_note(row));
  }
  let summary = if let Some(row) = rows.first() {
    format!(
      "Detected {} visible row(s) with strategy {} inside the resolved window; first row bounds {}.",
      rows.len(),
      detection.strategy,
      render_rect_compact(&row.bounds)
    )
  } else {
    format!(
      "Detected 0 visible row(s) inside the resolved window after strategy {}.",
      detection.strategy
    )
  };
  // Push in slot order: must match `ref_at(0)` reservation.
  artifacts.push(screenshot_artifact);
  artifacts.push(report_artifact);
  artifacts.push(recognition_artifact);

  Ok(DriverResponse {
    summary,
    backend: Some(format!("macos.vision.window-rows.{}", detection.strategy)),
    signals: crate::driver::macos::observe::row_detection_signals(rows.len()),
    notes,
    artifacts: artifacts.into_vec(),
  })
}

pub(crate) fn wait_for_window_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-rows-wait".to_string());
  let app_bundle_id = app_identifier(call).filter(|value| looks_like_bundle_identifier(value));
  let min_row_count = optional_i64(call, "min_row_count")?
    .unwrap_or(1)
    .clamp(1, 64) as usize;
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path = None;

  loop {
    attempts += 1;
    let capture =
      capture_resolved_window_observation(call, &format!("{label}-attempt-{attempts}"))?;
    let (detection, rows) = detect_rows_for_capture(call, &capture)?;
    let region =
      parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if rows.len() >= min_row_count || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }
      let report_artifact = build_text_artifact(
        "window-rows-wait-report",
        "txt",
        &format!(
          "window-rows-wait-report-{}",
          sanitize_file_component(&label)
        ),
        detection.report.clone(),
        "Captured row-detection report from the final wait-for-window-rows polling attempt.",
      )?;
      let (display_ref, native_display_id) = match &capture.capture_contract.capture_source {
        crate::driver::macos::capture::types::CaptureSource::Window {
          display_ref,
          native_display_id,
          ..
        } => (Some(display_ref.as_str()), Some(native_display_id.as_str())),
        _ => (None, None),
      };

      // Reserve slot 0 for the screenshot so the recognition artifact can cite
      // its ArtifactRef before the screenshot itself is pushed.
      let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
      let screenshot_ref = artifacts.ref_at(0);

      let recognition_artifact = row_recognition_artifact(
        "window-rows-wait-recognition",
        &format!(
          "window-rows-wait-recognition-{}",
          sanitize_file_component(&label)
        ),
        "Structured recognition result from the final wait-for-window-rows polling attempt.",
        RowRecognitionArtifactRequest {
          recognition_id: format!("window_rows_wait_{}", sanitize_file_component(&label)),
          source: recognition_source_for_rows(&detection.strategy, &rows),
          surface: crate::contract::RecognitionSurface::Window,
          rows: &rows,
          strategy: &detection.strategy,
          raw_match_count: detection.raw_match_count,
          filtered_match_count: detection.filtered_match_count,
          screenshot_path: capture.screenshot_path.as_path(),
          screenshot_dimensions: &capture.dimensions,
          display_ref,
          native_display_id,
          app_bundle_id: app_bundle_id.as_deref(),
          window_title: None,
          window_number: window_number_from_ref(&capture.capture_source),
          region_hint: region
            .as_ref()
            .map(|value| observed_rect_to_ratio_region(value, &capture.dimensions)),
          capture_contract: Some(&capture.capture_contract),
          capture_artifact: Some(screenshot_ref.clone()),
          additional_detail: serde_json::json!({
            "scope": &capture.scope,
            "capture_source": &capture.capture_source,
            "attempt_count": attempts,
            "timed_out": timed_out,
          }),
          known_limits: Vec::new(),
        },
      )?;
      let screenshot_artifact =
        screenshot_artifact(&capture, &label, "final waitForWindowRows polling attempt");
      let mut notes = row_notes(&capture, &detection, rows.len());
      notes.push(format!("requiredRowCount={min_row_count}"));
      notes.push(format!("attemptCount={attempts}"));
      notes.push(format!("elapsedMs={elapsed_ms}"));
      notes.push(format!("timeoutMs={timeout_ms}"));
      notes.push(format!("pollIntervalMs={poll_interval_ms}"));
      notes.push(format!("timedOut={timed_out}"));
      for row in rows.iter().take(5) {
        notes.push(render_ocr_row_note(row));
      }
      let summary = if rows.len() >= min_row_count {
        format!(
          "Observed {} visible row(s) inside the resolved window after {} polling attempt(s) over {} ms.",
          rows.len(),
          attempts,
          elapsed_ms
        )
      } else {
        format!(
          "Timed out while polling the resolved window for visible rows after strategy {}.",
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
          "macos.vision.wait-window-rows.{}",
          detection.strategy
        )),
        signals: crate::driver::macos::observe::wait_row_detection_signals(
          rows.len(),
          min_row_count,
          timed_out,
        ),
        notes,
        artifacts: artifacts.into_vec(),
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(capture.screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(crate) fn click_window_row(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-row-click".to_string());
  let capture = capture_resolved_window_observation(call, &label)?;
  let (detection, rows) = detect_rows_for_capture(call, &capture)?;
  let row_index = optional_i64(call, "row_index")?.unwrap_or(1).clamp(1, 64) as usize - 1;
  let row = rows.get(row_index).ok_or_else(|| {
    format!(
      "no visible row at index {} inside resolved window (detected {} row(s) with strategy {})",
      row_index + 1,
      rows.len(),
      detection.strategy
    )
  })?;
  let row_anchor_x_ratio = optional_f64(call, "row_anchor_x_ratio")?.unwrap_or(0.25);
  let row_anchor_y_ratio = optional_f64(call, "row_anchor_y_ratio")?.unwrap_or(0.5);
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
  let sx = row.bounds.x as f64 + (row.bounds.width as f64 * row_anchor_x_ratio);
  let sy = row.bounds.y as f64 + (row.bounds.height as f64 * row_anchor_y_ratio);
  let (logical_x, logical_y) =
    crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture.capture_contract,
      sx,
      sy,
    )?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
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
      settle_ms: Some(settle_ms),
      app: call.target.application_id.as_deref(),
    },
  );
  let _ = click_point(&nested_call)?;

  let report_artifact = build_text_artifact(
    "window-row-click",
    "txt",
    &format!("window-row-click-{}", sanitize_file_component(&label)),
    detection.report.clone(),
    "Captured row-detection report before clicking a resolved app window row.",
  )?;
  let screenshot_artifact = screenshot_artifact(&capture, &label, "window row click detection");
  let mut overlay_artifacts = build_overlay_evidence_artifacts(OverlayEvidenceRequest {
    kind: "window-row-click",
    label: label.clone(),
    screenshot_path: screenshot_artifact.source_path.clone(),
    screenshot_dimensions: capture.dimensions.clone(),
    capture_contract: capture.capture_contract.clone(),
    query: None,
    strategy: Some(detection.strategy.clone()),
    fallback_used: None,
    cursor_disturbance: Some("warp-visible".to_string()),
    press_mechanism: Some("pointer-click".to_string()),
    overlay_presentation: None,
    action_point: logical_to_capture_pixel(&capture.capture_contract, logical_x, logical_y),
    expected_target: Some(capture_pixel_to_logical(&capture.capture_contract, sx, sy)),
    ocr_match: None,
    row: Some(OverlayEvidenceRow {
      row_index,
      source: row.source.clone(),
      bounds: row.bounds.clone(),
      text_fragments: row.text_fragments.clone(),
    }),
    ax_target: None,
    decision: None,
    include_user_cursor: false,
    auv_cursor_variant: "auv-click",
  })?;
  let mut notes = row_notes(&capture, &detection, rows.len());
  notes.extend([
    format!("rowIndex={}", row_index + 1),
    format!("rowBounds={}", render_rect_compact(&row.bounds)),
    format!("rowText={}", row.text_fragments.join(" | ")),
    format!("rowAnchorRatio={row_anchor_x_ratio:.3},{row_anchor_y_ratio:.3}"),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("button={button_label}"),
    format!("clickCount={click_count}"),
    format!("clickIntervalMs={click_interval_ms}"),
    format!("settleMs={settle_ms}"),
  ]);

  Ok(DriverResponse {
    summary: format!(
      "Clicked visible row {} inside the resolved window at logical point ({logical_x:.3}, {logical_y:.3}).",
      row_index + 1
    ),
    backend: Some(format!(
      "macos.vision.click-window-row.{}",
      detection.strategy
    )),
    signals: click_window_row_signals(row_index + 1, rows.len()),
    notes,
    artifacts: {
      let mut artifacts = vec![screenshot_artifact, report_artifact];
      artifacts.append(&mut overlay_artifacts);
      artifacts
    },
  })
}

pub(super) fn capture_resolved_window_observation(
  call: &DriverCall,
  label: &str,
) -> AuvResult<CapturedObservation> {
  let observation = capture_window_with_typed_session(call, label)?;
  let screenshot_path = screenshot_temp_path(label);
  crate::driver::macos::capture::xcap_backend::save_rgba_image(
    observation.capture.image.clone(),
    &screenshot_path,
  )?;
  Ok(CapturedObservation {
    scope: "window".to_string(),
    capture_source: format!("window_{}", observation.candidate.window_ref.window_number),
    owner_bundle_id: (!observation.candidate.window_ref.owner_bundle_id.is_empty())
      .then(|| observation.candidate.window_ref.owner_bundle_id.clone()),
    window_title: (!observation.candidate.window_ref.title.is_empty())
      .then(|| observation.candidate.window_ref.title.clone()),
    screenshot_path,
    capture_contract: observation.contract,
    dimensions: observation.dimensions,
    image: Some(observation.capture.image),
    backend: Some(observation.capture.backend),
    fallback_reason: observation.capture.fallback_reason,
  })
}

pub(super) fn detect_rows_for_capture(
  call: &DriverCall,
  capture: &CapturedObservation,
) -> AuvResult<(DetectedScreenRows, Vec<ObservedOcrRow>)> {
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
  let region =
    parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;
  let detection = detect_screen_rows(
    capture.screenshot_path.as_path(),
    min_confidence,
    max_observations,
    region.as_ref(),
  )?;
  let rows = detection.rows.clone();
  Ok((detection, rows))
}

fn text_notes(
  capture: &CapturedObservation,
  query: &str,
  snapshot: &OcrTextSnapshot,
  filtered_count: usize,
) -> Vec<String> {
  let mut notes = vec![
    "scope=window".to_string(),
    format!("windowRef={}", capture.capture_source),
    format!("query={query}"),
    format!("matchCount={}", snapshot.matches.len()),
    format!("filteredMatchCount={filtered_count}"),
    format!(
      "screenshotPixels={}x{}",
      snapshot.image_width, snapshot.image_height
    ),
  ];
  if let Some(backend) = &capture.backend {
    notes.push(format!("captureBackend={backend}"));
  }
  if let Some(reason) = &capture.fallback_reason {
    notes.push(format!("fallbackReason={reason}"));
  }
  notes
}

fn row_notes(
  capture: &CapturedObservation,
  detection: &DetectedScreenRows,
  row_count: usize,
) -> Vec<String> {
  let mut notes = vec![
    "scope=window".to_string(),
    format!("windowRef={}", capture.capture_source),
    format!("rowStrategy={}", detection.strategy),
    format!("rowCount={row_count}"),
    format!("matchCount={}", detection.raw_match_count),
    format!("filteredMatchCount={}", detection.filtered_match_count),
    format!(
      "screenshotPixels={}x{}",
      capture.dimensions.width, capture.dimensions.height
    ),
  ];
  if let Some(backend) = &capture.backend {
    notes.push(format!("captureBackend={backend}"));
  }
  if let Some(reason) = &capture.fallback_reason {
    notes.push(format!("fallbackReason={reason}"));
  }
  notes
}

pub(super) fn logical_point_for_match(
  capture: &CapturedObservation,
  matched: &OcrTextMatch,
) -> AuvResult<(f64, f64)> {
  let (sx, sy) = ocr_match_center(matched);
  crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
    &capture.capture_contract,
    sx,
    sy,
  )
}

#[cfg(test)]
mod tests {
  use super::{
    ResolvedWindowTextCandidate, click_window_row_signals, click_window_text_signals,
    ensure_window_text_candidate_matches_capture, parse_window_text_candidate,
  };
  use crate::contract::{
    AnchorRecheckPrecondition, ArtifactRef, Candidate, CandidateEvidence, CandidateLiveness,
    ControlRequirements, LivenessPreconditions, RatioRegion, TargetGrounding, TargetSpec,
    WindowRefPrecondition,
  };
  use crate::driver::macos::ScreenshotDimensions;
  use crate::driver::macos::capture::types::{
    CaptureBackend, CaptureContract, CaptureSource, Rect, Scale2D, Size,
  };
  use crate::driver::macos::support::ocr_commands::CapturedObservation;
  use crate::trace::{ArtifactId, RunId, SpanId};
  use serde_json::json;
  use std::path::PathBuf;

  fn sample_capture_contract() -> CaptureContract {
    CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Window {
        display_ref: "display-main".to_string(),
        native_display_id: "69733248".to_string(),
        window_ref: "window_7".to_string(),
        native_window_id: "window-native-7".to_string(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 100.0,
        y: 200.0,
        width: 640.0,
        height: 480.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 960.0,
      },
      screenshot_pixel_size: Size {
        width: 1280.0,
        height: 960.0,
      },
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      captured_at_unix_ms: 1_717_000_000_000,
    }
  }

  fn sample_captured_observation() -> CapturedObservation {
    CapturedObservation {
      scope: "window".to_string(),
      capture_source: "window_7".to_string(),
      owner_bundle_id: Some("com.example.editor".to_string()),
      window_title: Some("Untitled".to_string()),
      screenshot_path: PathBuf::from("/tmp/window_ocr_test.png"),
      capture_contract: sample_capture_contract(),
      dimensions: ScreenshotDimensions {
        width: 1280,
        height: 960,
      },
      image: None,
      backend: Some("xcap.macos".to_string()),
      fallback_reason: None,
    }
  }

  fn sample_window_text_candidate_json(grounding: TargetGrounding) -> String {
    serde_json::to_string(&Candidate {
      candidate_local_id: "result-selection-anchor-ax".to_string(),
      kind: "result_selection".to_string(),
      label: Some("Play Now".to_string()),
      target_spec: TargetSpec {
        grounding,
        anchor_text: Some("Play Now".to_string()),
        region_hint: Some(RatioRegion {
          left: 0.10,
          top: 0.10,
          right: 0.35,
          bottom: 0.25,
        }),
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0003"),
          captured_event_id: None,
        },
        observation: json!({
          "query": {
            "ocr": {
              "region_hint": {
                "left": 0.10,
                "top": 0.10,
                "right": 0.35,
                "bottom": 0.25
              },
              "min_provider_score": 0.97
            }
          },
          "match_index": 1
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.example.editor".to_string(),
            window_title_substring: Some("Untitled".to_string()),
            window_number: Some(7),
          }),
          anchor_recheck: Some(AnchorRecheckPrecondition {
            text: "Play Now".to_string(),
            region_hint: Some(RatioRegion {
              left: 0.10,
              top: 0.10,
              right: 0.35,
              bottom: 0.25,
            }),
            expected_min_confidence: 0.97,
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
  fn click_window_text_signals_exposes_resolved_text() {
    let signals = click_window_text_signals("Play Now", None);

    assert_eq!(
      signals.get("click.resolved_text"),
      Some(&"Play Now".to_string())
    );
    assert_eq!(
      signals.get("clickWindowText.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn click_window_text_signals_mark_contract_candidate_consumption() {
    let signals = click_window_text_signals("Play Now", Some("result-selection-anchor-ax"));

    assert_eq!(
      signals.get("clickWindowText.consumer"),
      Some(&"contract-candidate".to_string())
    );
    assert_eq!(
      signals.get("clickWindowText.candidateLocalId"),
      Some(&"result-selection-anchor-ax".to_string())
    );
  }

  #[test]
  fn click_window_row_signals_expose_clicked_index_and_count() {
    let signals = click_window_row_signals(2, 5);

    assert_eq!(signals.get("rows.clicked_index"), Some(&"2".to_string()));
    assert_eq!(signals.get("rows.count"), Some(&"5".to_string()));
  }

  #[test]
  fn parse_window_text_candidate_accepts_ocr_anchor_candidate() {
    let candidate = parse_window_text_candidate(
      sample_window_text_candidate_json(TargetGrounding::OcrAnchor).as_str(),
    )
    .expect("ocr-anchor candidate should parse");

    assert_eq!(
      candidate,
      ResolvedWindowTextCandidate {
        candidate_local_id: "result-selection-anchor-ax".to_string(),
        query: "Play Now".to_string(),
        expected_window: Some(WindowRefPrecondition {
          app_bundle_id: "com.example.editor".to_string(),
          window_title_substring: Some("Untitled".to_string()),
          window_number: Some(7),
        }),
        region_hint: Some(RatioRegion {
          left: 0.10,
          top: 0.10,
          right: 0.35,
          bottom: 0.25,
        }),
        min_provider_score: Some(0.97),
        match_index: Some(1),
      }
    );
  }

  #[test]
  fn parse_window_text_candidate_rejects_non_ocr_anchor_candidate() {
    let error = parse_window_text_candidate(
      sample_window_text_candidate_json(TargetGrounding::Coordinate).as_str(),
    )
    .expect_err("non-ocr-anchor candidate should fail");

    assert!(error.contains("only accepts OcrAnchor candidates"));
  }

  #[test]
  fn ensure_window_text_candidate_matches_capture_accepts_matching_window() {
    let candidate = parse_window_text_candidate(
      sample_window_text_candidate_json(TargetGrounding::OcrAnchor).as_str(),
    )
    .expect("candidate should parse");

    ensure_window_text_candidate_matches_capture(&candidate, &sample_captured_observation())
      .expect("matching capture should validate");
  }

  #[test]
  fn ensure_window_text_candidate_matches_capture_rejects_mismatched_bundle() {
    let mut capture = sample_captured_observation();
    capture.owner_bundle_id = Some("com.other.app".to_string());
    let candidate = parse_window_text_candidate(
      sample_window_text_candidate_json(TargetGrounding::OcrAnchor).as_str(),
    )
    .expect("candidate should parse");

    let error = ensure_window_text_candidate_matches_capture(&candidate, &capture)
      .expect_err("mismatched bundle should fail");

    assert!(error.contains("expected app bundle"));
  }
}
