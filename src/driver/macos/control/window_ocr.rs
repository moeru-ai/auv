use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use super::super::*;
use super::common::{ClickPointCallOptions, build_click_point_call, resolve_click_interval_ms};
use super::pointer::click_point;

pub(super) fn click_window_text_signals(text: &str) -> std::collections::BTreeMap<String, String> {
  std::collections::BTreeMap::from([("click.resolved_text".to_string(), text.to_string())])
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
  let query = required_non_empty_string(call, "query")?;
  let label = format!("window-text-click-{}", sanitize_file_component(&query));
  let capture = capture_resolved_window_observation(call, &label)?;
  let (snapshot, filtered, ocr_report, command_report) =
    run_text_match_on_capture(call, &capture, &query)?;
  let match_index = optional_i64(call, "match_index")?.unwrap_or(0).max(0) as usize;
  let matched = filtered.get(match_index).ok_or_else(|| {
    format!(
      "no filtered OCR text match at index {} for query {} inside resolved window; inspect `debug.findWindowText`",
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
  let nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
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
  let mut notes = text_notes(&capture, &query, &snapshot, filtered.len());
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
  ]);

  Ok(DriverResponse {
    summary: format!(
      "Clicked OCR text anchor {} for query {} inside the resolved window at logical point ({logical_x:.3}, {logical_y:.3}).",
      matched.text, query
    ),
    backend: Some("macos.vision.click-window-text".to_string()),
    signals: click_window_text_signals(&matched.text),
    notes,
    artifacts: match json_artifact {
      Some(json_artifact) => vec![screenshot_artifact, report_artifact, json_artifact],
      None => vec![screenshot_artifact, report_artifact],
    },
  })
}

pub(crate) fn find_window_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-rows".to_string());
  let capture = capture_resolved_window_observation(call, &label)?;
  let (detection, rows) = detect_rows_for_capture(call, &capture)?;
  let report_artifact = build_text_artifact(
    "window-rows-report",
    "txt",
    &format!("window-rows-report-{}", sanitize_file_component(&label)),
    detection.report.clone(),
    "Captured row-detection report for a resolved app window.",
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
  Ok(DriverResponse {
    summary,
    backend: Some(format!("macos.vision.window-rows.{}", detection.strategy)),
    signals: crate::driver::macos::observe::row_detection_signals(rows.len()),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(crate) fn wait_for_window_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-rows-wait".to_string());
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
        artifacts: vec![screenshot_artifact, report_artifact],
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
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(super) fn capture_resolved_window_observation(
  call: &DriverCall,
  label: &str,
) -> AuvResult<CapturedObservation> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| {
      "operation requires --target <application-id> or --app <application-id>".to_string()
    })?;
  let selection = parse_window_selection(call)?;
  let _ = maybe_activate_target_app_for_observation(call)?;
  let selector = parse_app_selector(&app)?;
  let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
  let (screenshot_path, capture_contract, selected) = retry_window_capture_operation(|| {
    let snapshot = crate::driver::macos::observe::observe_windows_snapshot(128, &app)?;
    let resolved_app = resolve_app_ref(&snapshot, &selector)?;
    let candidate = resolve_window_candidate(&snapshot, &resolved_app, &displays, &selection)?;
    if selection.has_selector() && !candidate.is_fully_contained_in_display {
      return Err(window_capture_readiness_diagnostic(&candidate, &displays));
    }
    let native_window_id = candidate.native_window_id.as_deref().ok_or_else(|| {
      "resolved window candidate has no native window id; inspect `debug.listWindows`".to_string()
    })?;
    let result = crate::driver::macos::capture::xcap_backend::capture_window_native_id_to_path(
      label,
      native_window_id,
      candidate.window_ref.window_number,
    );
    match result {
      Ok(x) => Ok(x),
      Err(ref e)
        if e.contains(crate::driver::macos::capture::types::capture_error::STALE_WINDOW_REF) =>
      {
        // xcap skips windows with kCGWindowSharingState==0 during enumeration.
        // Fall back to direct CGWindowListCreateImage which respects Screen Recording permission
        // regardless of sharing state.
        let logical_bounds = crate::driver::macos::capture::types::Rect {
          x: candidate.window_ref.bounds.x as f64,
          y: candidate.window_ref.bounds.y as f64,
          width: candidate.window_ref.bounds.width as f64,
          height: candidate.window_ref.bounds.height as f64,
        };
        crate::driver::macos::capture::xcap_backend::capture_window_cg_to_path(
          label,
          candidate.window_ref.window_number,
          &logical_bounds,
          &displays,
        )
      }
      Err(e) => Err(e),
    }
  })?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  Ok(CapturedObservation {
    scope: "window".to_string(),
    capture_source: selected.window_ref,
    screenshot_path,
    capture_contract,
    dimensions,
  })
}

fn detect_rows_for_capture(
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
  vec![
    "scope=window".to_string(),
    format!("windowRef={}", capture.capture_source),
    format!("query={query}"),
    format!("matchCount={}", snapshot.matches.len()),
    format!("filteredMatchCount={filtered_count}"),
    format!(
      "screenshotPixels={}x{}",
      snapshot.image_width, snapshot.image_height
    ),
  ]
}

fn row_notes(
  capture: &CapturedObservation,
  detection: &DetectedScreenRows,
  row_count: usize,
) -> Vec<String> {
  vec![
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
  ]
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

fn screenshot_artifact(
  capture: &CapturedObservation,
  label: &str,
  note_suffix: &str,
) -> ProducedArtifact {
  ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: capture.screenshot_path.clone(),
    preferred_name: format!("{}.png", sanitize_file_component(label)),
    note: Some(format!("Screenshot captured for {note_suffix}.")),
  }
}

#[cfg(test)]
mod tests {
  use super::{click_window_row_signals, click_window_text_signals};

  #[test]
  fn click_window_text_signals_exposes_resolved_text() {
    let signals = click_window_text_signals("Play Now");

    assert_eq!(
      signals.get("click.resolved_text"),
      Some(&"Play Now".to_string())
    );
  }

  #[test]
  fn click_window_row_signals_expose_clicked_index_and_count() {
    let signals = click_window_row_signals(2, 5);

    assert_eq!(signals.get("rows.clicked_index"), Some(&"2".to_string()));
    assert_eq!(signals.get("rows.count"), Some(&"5".to_string()));
  }
}
