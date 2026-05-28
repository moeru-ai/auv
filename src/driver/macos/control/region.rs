// File: src/driver/macos/control/region.rs
//! Region-scoped observation primitives for the macOS driver.
//!
//! Implements operations like `debug.observeWindowRegion`: capture a resolved
//! window, constrain an OCR region, detect row-like candidates, and emit
//! machine-readable artifacts (legacy rows JSON + `RecognitionResult`).
//!
//! Boundary: produces observation evidence; it does not imply generic list
//! understanding or a complete segmentation/scroll model.

use std::collections::BTreeMap;

use super::super::*;
use super::common::parse_input_policy;

pub(crate) fn observe_window_region(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "window-region-observe".to_string());
  let app_bundle_id = app_identifier(call).filter(|value| looks_like_bundle_identifier(value));
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
  let region = region_ratios_from_call(call)?;
  let capture = super::window_ocr::capture_resolved_window_observation(call, &label)?;
  let ocr_region = region.to_observed_rect(capture.dimensions.width, capture.dimensions.height)?;
  let detection = detect_screen_rows(
    capture.screenshot_path.as_path(),
    min_confidence,
    max_observations,
    Some(&ocr_region),
  )?;
  let rows = detection.rows;
  let row_filter = filter_list_row_candidates(&rows);
  let json = render_observe_window_region_json(
    &rows,
    &row_filter,
    &ocr_region,
    &capture.dimensions,
    &capture.screenshot_path,
  )?;
  let json_artifact = build_text_artifact(
    "window-region-observation",
    "json",
    &format!("{}-rows", sanitize_file_component(&label)),
    json,
    "Machine-readable OCR row observation for a window region.",
  )?;
  let (display_ref, native_display_id) = match &capture.capture_contract.capture_source {
    crate::driver::macos::capture::types::CaptureSource::Window {
      display_ref,
      native_display_id,
      ..
    } => (Some(display_ref.as_str()), Some(native_display_id.as_str())),
    _ => (None, None),
  };

  // Reserve refs up front so recognition artifacts can cite the screenshot's
  // ArtifactRef before it has been pushed onto the response.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let screenshot_ref = artifacts.ref_at(0);

  let recognition_artifact = row_recognition_artifact(
    "window-region-recognition",
    &format!("{}-recognition", sanitize_file_component(&label)),
    "Structured recognition result for OCR row observation in a window region.",
    RowRecognitionArtifactRequest {
      recognition_id: format!("window_region_{}", sanitize_file_component(&label)),
      source: recognition_source_for_rows(&detection.strategy, &rows),
      surface: crate::contract::RecognitionSurface::Region,
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
      region_hint: Some(observed_rect_to_ratio_region(
        &ocr_region,
        &capture.dimensions,
      )),
      capture_contract: Some(&capture.capture_contract),
      capture_artifact: Some(screenshot_ref.clone()),
      additional_detail: serde_json::json!({
        "scope": &capture.scope,
        "capture_source": &capture.capture_source,
        "region_pixels": {
          "x": ocr_region.x,
          "y": ocr_region.y,
          "width": ocr_region.width,
          "height": ocr_region.height,
        },
        "max_observations": max_observations,
        "min_confidence": min_confidence,
      }),
      known_limits: vec![
        "region observation still uses heuristic row filtering for list semantics".to_string(),
      ],
    },
  )?;
  // TODO: Emit a full typed capture contract artifact for window-region
  // observation. This command records the screenshot and OCR-region bounds so
  // scroll scan can crop list item candidates, but it still lacks the same
  // reusable capture contract produced by the dedicated capture commands.
  // TODO: Extend window-region observation into a real region-segmentation
  // pass. Scroll scan needs candidates for list bodies, section separators,
  // sticky headers, empty states, and scrollbars/thumbs so the scan loop can
  // distinguish top/bottom boundaries from ordinary repeated content.
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: capture.screenshot_path.clone(),
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Window screenshot captured for region observation.".to_string()),
  };

  let overlay_rows = rows
    .iter()
    .map(|row| OverlayEvidenceRow {
      row_index: row.row_index,
      source: row.source.clone(),
      bounds: row.bounds.clone(),
      text_fragments: row.text_fragments.clone(),
    })
    .collect::<Vec<_>>();
  let overlay_artifacts = build_row_observation_overlay_artifacts(RowObservationOverlayRequest {
    label: label.clone(),
    screenshot_path: capture.screenshot_path.clone(),
    screenshot_dimensions: capture.dimensions.clone(),
    strategy: detection.strategy.clone(),
    rows: overlay_rows,
  })?;

  let segments = classify_segmented_regions(&rows, &row_filter);
  let seg_artifact = if segments.is_empty() {
    None
  } else {
    Some(segmented_region_recognition_artifact(
      &format!("{}-segmentation", sanitize_file_component(&label)),
      SegmentedRegionArtifactRequest {
        recognition_id: format!(
          "window_region_{}_segmentation",
          sanitize_file_component(&label)
        ),
        surface: crate::contract::RecognitionSurface::Region,
        segments: &segments,
        row_count: rows.len(),
        screenshot_path: capture.screenshot_path.as_path(),
        screenshot_dimensions: &capture.dimensions,
        display_ref,
        native_display_id,
        app_bundle_id: app_bundle_id.as_deref(),
        window_number: window_number_from_ref(&capture.capture_source),
        region_hint: Some(observed_rect_to_ratio_region(
          &ocr_region,
          &capture.dimensions,
        )),
        capture_artifact: Some(screenshot_ref.clone()),
      },
    )?)
  };

  // Push in slot order: must match `ref_at(0)` reservation above.
  artifacts.push(screenshot_artifact);
  artifacts.push(json_artifact);
  artifacts.push(recognition_artifact);
  for overlay in overlay_artifacts {
    artifacts.push(overlay);
  }
  if let Some(seg) = seg_artifact {
    artifacts.push(seg);
  }

  Ok(DriverResponse {
    summary: format!(
      "Observed {} OCR row(s) in resolved window region.",
      rows.len()
    ),
    backend: Some("macos.vision.observe-window-region".to_string()),
    signals: crate::driver::macos::observe::row_detection_signals(rows.len()),
    notes: vec![
      format!("scope={}", capture.scope),
      format!("windowRef={}", capture.capture_source),
      format!("region={}", render_rect_compact(&ocr_region)),
      format!("rows.count={}", rows.len()),
      format!("strategy={}", detection.strategy),
      format!("minConfidence={min_confidence:.3}"),
      format!("maxObservations={max_observations}"),
      format!(
        "screenshotPixels={}x{}",
        capture.dimensions.width, capture.dimensions.height
      ),
    ],
    artifacts: artifacts.into_vec(),
  })
}

pub(crate) fn scroll_window_region(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).ok_or_else(|| {
    "scroll_window_region requires --target <application-id> or --app".to_string()
  })?;
  let raw_direction = optional_string(call, "direction").unwrap_or_else(|| "down".to_string());
  let direction = normalize_scroll_direction(&raw_direction)?.to_string();
  let amount = optional_f64(call, "amount")?.unwrap_or(6.0).max(1.0);
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(250);
  let input_policy = parse_input_policy(call)?;
  let region = region_ratios_from_call(call)?;
  if input_policy == auv_driver::InputPolicy::ForegroundPreferred {
    activate_target_app(&app)?;
  }
  let snapshot = super::super::observe::observe_windows_snapshot(24, &app)?;
  let xcap_displays = super::super::capture::xcap_backend::list_displays()?;
  let display_snapshot = enumerate_displays()?;
  let selector = parse_app_selector(&app)?;
  let resolved_app = resolve_app_ref(&snapshot, &selector)?;
  let candidate = resolve_window_candidate_for_input(
    &snapshot,
    &resolved_app,
    &xcap_displays,
    &parse_window_selection(call)?,
  )?;
  let window = &candidate.window_ref;
  let x =
    window.bounds.x as f64 + window.bounds.width as f64 * ((region.left + region.right) / 2.0);
  let y =
    window.bounds.y as f64 + window.bounds.height as f64 * ((region.top + region.bottom) / 2.0);
  let resolution = resolve_display_point(&display_snapshot, x, y).ok_or_else(|| {
    format!("resolved scroll point ({x:.3}, {y:.3}) is outside all connected displays")
  })?;
  let (delta_x, delta_y) = scan_scroll_delta(&direction, amount)?;
  let scroll_outcome = crate::driver::macos::typed::session::scroll_point_bridge(
    x,
    y,
    delta_x,
    delta_y,
    input_policy,
    settle_ms,
  )?;

  let report = [
    "coordinateSpace=global-logical".to_string(),
    format!("applicationId={app}"),
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
    format!("windowId={}", window.window_number),
    format!("windowTitle={}", window.title),
    format!("windowBounds={}", render_rect_compact(&window.bounds)),
    format!(
      "regionRatios={:.3},{:.3},{:.3},{:.3}",
      region.left, region.top, region.right, region.bottom
    ),
    format!("scrollPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "displayBounds={}",
      render_rect_compact(&resolution.display.bounds)
    ),
    format!("candidateIndex={}", candidate.candidate_index),
    format!("selectionReason={}", candidate.selection_reason),
    format!(
      "isFullyContainedInDisplay={}",
      candidate.is_fully_contained_in_display
    ),
    format!(
      "displayRef={}",
      candidate
        .display_ref
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!(
      "nativeDisplayId={}",
      candidate
        .native_display_id
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!("candidateArea={}", candidate.area),
    format!("direction={direction}"),
    format!("amount={amount:.3}"),
    format!("deltaX={delta_x:.0}"),
    format!("deltaY={delta_y:.0}"),
    format!("inputPolicy={}", scroll_outcome.input_policy),
    format!("inputBridge={}", scroll_outcome.input_bridge),
    format!("selectedPath={}", scroll_outcome.selected_path),
    format!("settleMs={settle_ms}"),
  ]
  .join("\n");
  let artifact = build_text_artifact(
    "window-region-scroll",
    "txt",
    "window-region-scroll",
    report,
    "Scrolled at the center of a resolved window region.",
  )?;

  Ok(DriverResponse {
    summary: format!("Scrolled window region {direction} by amount {amount:.3}."),
    backend: Some("macos.typed.input.scroll-window-region".to_string()),
    signals: BTreeMap::from([
      ("scroll.direction".to_string(), direction),
      ("scroll.amount".to_string(), format!("{amount:.3}")),
    ]),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("windowId={}", window.window_number),
      format!("windowBounds={}", render_rect_compact(&window.bounds)),
      format!(
        "regionRatios={:.3},{:.3},{:.3},{:.3}",
        region.left, region.top, region.right, region.bottom
      ),
      format!("scrollPoint={x:.3},{y:.3}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      format!("displayId={}", resolution.display.display_id),
      format!("candidateIndex={}", candidate.candidate_index),
      format!("selectionReason={}", candidate.selection_reason),
      format!("deltaX={delta_x:.0}"),
      format!("deltaY={delta_y:.0}"),
      format!("inputPolicy={}", scroll_outcome.input_policy),
      format!("inputBridge={}", scroll_outcome.input_bridge),
      format!("selectedPath={}", scroll_outcome.selected_path),
      format!("settleMs={settle_ms}"),
    ],
    artifacts: vec![artifact],
  })
}

fn scan_scroll_delta(direction: &str, amount: f64) -> AuvResult<(f64, f64)> {
  match normalize_scroll_direction(direction)? {
    "down" => Ok((0.0, -amount)),
    "up" => Ok((0.0, amount)),
    "right" => Ok((-amount, 0.0)),
    "left" => Ok((amount, 0.0)),
    _ => unreachable!("normalize_scroll_direction only returns supported directions"),
  }
}

fn normalize_scroll_direction(direction: &str) -> AuvResult<&'static str> {
  match direction.trim().to_ascii_lowercase().as_str() {
    "down" => Ok("down"),
    "up" => Ok("up"),
    "right" => Ok("right"),
    "left" => Ok("left"),
    other => Err(format!(
      "invalid scroll direction {other:?}; expected down, up, left, or right"
    )),
  }
}

#[derive(Clone, Copy, Debug)]
struct RegionRatios {
  left: f64,
  top: f64,
  right: f64,
  bottom: f64,
}

impl RegionRatios {
  fn to_observed_rect(self, width: i64, height: i64) -> AuvResult<ObservedRect> {
    if width <= 0 || height <= 0 {
      return Err(format!(
        "invalid screenshot dimensions {}x{}: expected positive width and height",
        width, height
      ));
    }
    let (left_px, right_px) = ratio_edges_to_pixels(self.left, self.right, width);
    let (top_px, bottom_px) = ratio_edges_to_pixels(self.top, self.bottom, height);

    Ok(ObservedRect {
      x: left_px,
      y: top_px,
      width: right_px - left_px,
      height: bottom_px - top_px,
    })
  }
}

fn ratio_edges_to_pixels(left: f64, right: f64, size: i64) -> (i64, i64) {
  let size_f = size as f64;
  let left_px = (left * size_f).floor() as i64;
  let left_px = left_px.clamp(0, size - 1);
  let right_px = (right * size_f).ceil() as i64;
  let right_px = right_px.clamp(left_px + 1, size);

  (left_px, right_px)
}

fn region_ratios_from_call(call: &DriverCall) -> AuvResult<RegionRatios> {
  let left = optional_f64(call, "region_left_ratio")?.unwrap_or(0.0);
  let top = optional_f64(call, "region_top_ratio")?.unwrap_or(0.0);
  let right = optional_f64(call, "region_right_ratio")?.unwrap_or(1.0);
  let bottom = optional_f64(call, "region_bottom_ratio")?.unwrap_or(1.0);
  validate_region_ratios(left, top, right, bottom)?;
  Ok(RegionRatios {
    left,
    top,
    right,
    bottom,
  })
}

fn validate_region_ratios(left: f64, top: f64, right: f64, bottom: f64) -> AuvResult<()> {
  if !(0.0 <= left && left < right && right <= 1.0) {
    return Err("invalid region x ratios: expected 0.0 <= left < right <= 1.0".to_string());
  }
  if !(0.0 <= top && top < bottom && bottom <= 1.0) {
    return Err("invalid region y ratios: expected 0.0 <= top < bottom <= 1.0".to_string());
  }
  Ok(())
}

fn render_observe_window_region_json(
  rows: &[ObservedOcrRow],
  filter: &ListRowFilterResult,
  region: &ObservedRect,
  dimensions: &ScreenshotDimensions,
  screenshot_path: &std::path::Path,
) -> AuvResult<String> {
  let row_candidates = rows
    .iter()
    .map(|row| {
      let accepted = filter.accepted_indices.contains(&row.row_index);
      let reject_reason = filter
        .rejected
        .iter()
        .find(|rejected| rejected.row_index == row.row_index)
        .map(|rejected| rejected.reason.as_str());
      let mut value = serde_json::json!({
        "row_index": row.row_index,
        "source": row.source,
        "text": row.text_fragments.join(" | "),
        "text_fragments": row.text_fragments,
        "accepted_by_row_filter": accepted,
        "bounds": {
          "x": row.bounds.x,
          "y": row.bounds.y,
          "width": row.bounds.width,
          "height": row.bounds.height,
        },
      });
      if let Some(reason) = reject_reason {
        value["reject_reason"] = serde_json::Value::String(reason.to_string());
      }
      value
    })
    .collect::<Vec<_>>();
  let item_candidates = rows
    .iter()
    .filter(|row| filter.accepted_indices.contains(&row.row_index))
    .enumerate()
    .map(|(item_index, row)| {
      serde_json::json!({
        "item_index": item_index,
        "row_candidate_index": row.row_index,
        "source": "row_filter",
        "text": row.text_fragments.join(" | "),
        "text_fragments": row.text_fragments,
        "filter_reason": "accepted_repeating_row_geometry",
        "segmented_region_role": "list_region",
        "bounds": {
          "x": row.bounds.x,
          "y": row.bounds.y,
          "width": row.bounds.width,
          "height": row.bounds.height,
        },
      })
    })
    .collect::<Vec<_>>();
  let rejected_row_candidates = filter
    .rejected
    .iter()
    .filter_map(|rejected| {
      rows
        .iter()
        .find(|row| row.row_index == rejected.row_index)
        .map(|row| (rejected, row))
    })
    .map(|(rejected, row)| {
      serde_json::json!({
        "row_candidate_index": row.row_index,
        "reject_reason": rejected.reason,
        "source": row.source,
        "bounds": {
          "x": row.bounds.x,
          "y": row.bounds.y,
          "width": row.bounds.width,
          "height": row.bounds.height,
        },
      })
    })
    .collect::<Vec<_>>();
  let segmented_regions = filter
    .list_region
    .as_ref()
    .map(|list_region| {
      vec![serde_json::json!({
        "region_index": 0,
        "role": "list_region",
        "confidence": filter.confidence,
        "evidence": filter.evidence,
        "bounds": {
          "x": list_region.x,
          "y": list_region.y,
          "width": list_region.width,
          "height": list_region.height,
        },
      })]
    })
    .unwrap_or_default();
  serde_json::to_string_pretty(&serde_json::json!({
    "extractor": "ocr-row",
    "coordinate_space": "window_screenshot_pixels",
    "screenshot_path": screenshot_path.display().to_string(),
    "screenshot_width": dimensions.width,
    "screenshot_height": dimensions.height,
    "region": {
      "x": region.x,
      "y": region.y,
      "width": region.width,
      "height": region.height,
    },
    "segmented_regions": segmented_regions,
    "rows": row_candidates,
    "row_candidates": row_candidates,
    "rejected_row_candidates": rejected_row_candidates,
    "item_candidates": item_candidates,
  }))
  .map(|mut rendered| {
    rendered.push('\n');
    rendered
  })
  .map_err(|error| format!("failed to render window region observation json: {error}"))
}

#[derive(Clone, Debug)]
struct ListRowFilterResult {
  accepted_indices: Vec<usize>,
  rejected: Vec<RejectedRowCandidate>,
  list_region: Option<ObservedRect>,
  height_band: Option<RowHeightBand>,
  confidence: &'static str,
  evidence: &'static str,
}

#[derive(Clone, Debug)]
struct RejectedRowCandidate {
  row_index: usize,
  reason: String,
}

fn filter_list_row_candidates(rows: &[ObservedOcrRow]) -> ListRowFilterResult {
  if rows.is_empty() {
    return ListRowFilterResult {
      accepted_indices: Vec::new(),
      rejected: Vec::new(),
      list_region: None,
      height_band: None,
      confidence: "none",
      evidence: "no_row_candidates",
    };
  }

  // TODO: Add OCR/AX anchor evidence and optional icon/template matching to the
  // Row Filter before making semantic decisions. This is still geometry-only
  // and intentionally rejects only clear height outliers so likely list rows
  // remain available to later hooks. Icon evidence will matter for rows whose
  // identity or state is mostly visual, such as selected/liked/downloaded
  // markers or section-specific affordances.
  let Some((height_band, evidence_count)) = repeating_row_height_band(rows) else {
    let accepted_indices = rows.iter().map(|row| row.row_index).collect::<Vec<_>>();
    return ListRowFilterResult {
      list_region: union_row_bounds(rows),
      accepted_indices,
      rejected: Vec::new(),
      height_band: None,
      confidence: "low",
      evidence: "insufficient_repeating_row_evidence",
    };
  };

  if rows.len() < 4 || evidence_count < 3 {
    let accepted_indices = rows.iter().map(|row| row.row_index).collect::<Vec<_>>();
    return ListRowFilterResult {
      list_region: union_row_bounds(rows),
      accepted_indices,
      rejected: Vec::new(),
      height_band: Some(height_band),
      confidence: "low",
      evidence: "insufficient_repeating_row_evidence",
    };
  }

  let mut accepted = Vec::new();
  let mut rejected = Vec::new();
  for row in rows {
    if height_band.contains(row.bounds.height) {
      accepted.push(row.row_index);
    } else {
      rejected.push(RejectedRowCandidate {
        row_index: row.row_index,
        reason: "height_outside_repeating_row_band".to_string(),
      });
    }
  }

  let accepted_rows = rows
    .iter()
    .filter(|row| accepted.contains(&row.row_index))
    .cloned()
    .collect::<Vec<_>>();

  ListRowFilterResult {
    accepted_indices: accepted,
    rejected,
    list_region: union_row_bounds(&accepted_rows),
    height_band: Some(height_band),
    confidence: "heuristic",
    evidence: "repeating_row_height_band",
  }
}

#[derive(Clone, Copy, Debug)]
struct RowHeightBand {
  min: i64,
  max: i64,
}

impl RowHeightBand {
  fn contains(self, height: i64) -> bool {
    self.min <= height && height <= self.max
  }
}

fn repeating_row_height_band(rows: &[ObservedOcrRow]) -> Option<(RowHeightBand, usize)> {
  let mut heights = rows.iter().map(|row| row.bounds.height).collect::<Vec<_>>();
  heights.sort_unstable();
  if heights.is_empty() {
    return None;
  }

  let sample = trimmed_height_sample(&heights);
  let median = sample[sample.len() / 2];
  let min = ((median as f64) * 0.80).floor() as i64;
  let max = ((median as f64) * 1.45).ceil() as i64;
  let band = RowHeightBand {
    min: min.max(1),
    max: max.max(min + 1),
  };
  let evidence_count = heights
    .iter()
    .filter(|height| band.contains(**height))
    .count();
  Some((band, evidence_count))
}

fn trimmed_height_sample(heights: &[i64]) -> &[i64] {
  if heights.len() < 5 {
    return heights;
  }
  let trim = (heights.len() / 5).max(1);
  &heights[trim..heights.len() - trim]
}

fn union_row_bounds(rows: &[ObservedOcrRow]) -> Option<ObservedRect> {
  let mut iter = rows.iter();
  let first = iter.next()?;
  Some(iter.fold(first.bounds.clone(), |bounds, row| {
    union_observed_rects(&bounds, &row.bounds)
  }))
}

fn union_observed_rects(left: &ObservedRect, right: &ObservedRect) -> ObservedRect {
  let min_x = left.x.min(right.x);
  let min_y = left.y.min(right.y);
  let max_x = (left.x + left.width).max(right.x + right.width);
  let max_y = (left.y + left.height).max(right.y + right.height);
  ObservedRect {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  }
}

// --- Phase 7a: rule-based region segmentation ---

#[derive(Clone, Debug)]
pub(crate) struct SegmentedRegion {
  pub(crate) role: String,
  pub(crate) bounds: ObservedRect,
  pub(crate) confidence: &'static str,
  pub(crate) evidence: Vec<&'static str>,
  pub(crate) contributing_row_count: usize,
}

fn classify_segmented_regions(
  rows: &[ObservedOcrRow],
  filter: &ListRowFilterResult,
) -> Vec<SegmentedRegion> {
  let mut regions = Vec::new();

  if let Some(ref list_bounds) = filter.list_region {
    regions.push(SegmentedRegion {
      role: "list_region".to_string(),
      bounds: list_bounds.clone(),
      confidence: filter.confidence,
      evidence: vec![filter.evidence],
      contributing_row_count: filter.accepted_indices.len(),
    });
  }

  let first_accepted_y = filter
    .accepted_indices
    .iter()
    .filter_map(|&idx| rows.iter().find(|r| r.row_index == idx))
    .map(|r| r.bounds.y)
    .min();
  let last_accepted_bottom = filter
    .accepted_indices
    .iter()
    .filter_map(|&idx| rows.iter().find(|r| r.row_index == idx))
    .map(|r| r.bounds.y + r.bounds.height)
    .max();
  let band_min = filter.height_band.map(|b| b.min).unwrap_or(0);

  for rejected in &filter.rejected {
    let Some(row) = rows.iter().find(|r| r.row_index == rejected.row_index) else {
      continue;
    };
    let row_bottom = row.bounds.y + row.bounds.height;
    let is_above_list = first_accepted_y.map_or(false, |y| row_bottom <= y);
    let is_below_list = last_accepted_bottom.map_or(false, |y| row.bounds.y >= y);
    let is_thin = band_min > 0 && row.bounds.height < (band_min / 2).max(4);

    let role = if is_thin {
      "section_separator"
    } else if is_above_list {
      "sticky_header"
    } else if is_below_list {
      "trailing_element"
    } else {
      continue;
    };

    regions.push(SegmentedRegion {
      role: role.to_string(),
      bounds: row.bounds.clone(),
      confidence: "low",
      evidence: vec!["outlier_row_geometry"],
      contributing_row_count: 1,
    });
  }

  regions
}

struct SegmentedRegionArtifactRequest<'a> {
  recognition_id: String,
  surface: crate::contract::RecognitionSurface,
  segments: &'a [SegmentedRegion],
  row_count: usize,
  screenshot_path: &'a std::path::Path,
  screenshot_dimensions: &'a ScreenshotDimensions,
  display_ref: Option<&'a str>,
  native_display_id: Option<&'a str>,
  app_bundle_id: Option<&'a str>,
  window_number: Option<i64>,
  region_hint: Option<crate::contract::RatioRegion>,
  /// ArtifactRef pointing at the screenshot artifact this segmentation is
  /// derived from. Populates `scope.capture_artifact` and `evidence`.
  capture_artifact: Option<crate::contract::ArtifactRef>,
}

fn segmented_region_recognition_artifact(
  label: &str,
  request: SegmentedRegionArtifactRequest<'_>,
) -> AuvResult<ProducedArtifact> {
  use crate::contract::{
    RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource, RecognizedItem,
  };

  let items: Vec<RecognizedItem> = request
    .segments
    .iter()
    .enumerate()
    .map(|(i, seg)| RecognizedItem {
      item_id: format!("region#{}", i + 1),
      kind: seg.role.clone(),
      box_: RecognitionBox {
        x: seg.bounds.x,
        y: seg.bounds.y,
        width: seg.bounds.width,
        height: seg.bounds.height,
      },
      text: None,
      provider_score: None,
      detail: serde_json::json!({
        "role": seg.role,
        "confidence": seg.confidence,
        "evidence": seg.evidence,
        "contributing_row_count": seg.contributing_row_count,
      }),
    })
    .collect();

  let best = items.first().cloned();
  let evidence = match request.capture_artifact.as_ref() {
    Some(reference) => vec![reference.clone()],
    None => Vec::new(),
  };

  let result = RecognitionResult {
    recognition_id: request.recognition_id,
    source: RecognitionSource::SegmentedRegion,
    scope: RecognitionScope {
      surface: request.surface,
      display_ref: request.display_ref.map(str::to_string),
      native_display_id: request.native_display_id.map(str::to_string),
      app_bundle_id: request.app_bundle_id.map(str::to_string),
      window_title: None,
      window_number: request.window_number,
      region_hint: request.region_hint,
      capture_artifact: request.capture_artifact,
      capture_contract_artifact: None,
    },
    best,
    filtered: items.clone(),
    all: items,
    detail: serde_json::json!({
      "provider": "rule_based_segmentation",
      "region_count": request.segments.len(),
      "row_count": request.row_count,
      "screenshot": {
        "path": request.screenshot_path.display().to_string(),
        "width": request.screenshot_dimensions.width,
        "height": request.screenshot_dimensions.height,
      },
    }),
    evidence,
    known_limits: vec![
      "geometry-only segmentation: no AX element tree or visual diff evidence".to_string(),
    ],
  };

  let json = serde_json::to_string_pretty(&result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|e| format!("failed to encode segmented region recognition result: {e}"))?;

  build_text_artifact(
    "window-region-segmentation",
    "json",
    label,
    json,
    "Rule-based segmented region recognition result.",
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_region_ratios_rejects_inverted_region() {
    let error = validate_region_ratios(0.8, 0.2, 0.4, 0.9).expect_err("region should fail");
    assert!(error.contains("expected 0.0 <= left < right <= 1.0"));
  }

  #[test]
  fn validate_region_ratios_accepts_normal_region() {
    validate_region_ratios(0.1, 0.2, 0.9, 0.8).expect("region should pass");
  }

  #[test]
  fn scan_scroll_delta_defaults_to_vertical_down() {
    let (delta_x, delta_y) = scan_scroll_delta("down", 6.0).expect("delta");
    assert_eq!(delta_x, 0.0);
    assert!(delta_y < 0.0);
  }

  #[test]
  fn scan_scroll_delta_normalizes_direction_case() {
    let (delta_x, delta_y) = scan_scroll_delta("DOWN", 6.0).expect("delta");
    assert_eq!((delta_x, delta_y), (0.0, -6.0));
  }

  #[test]
  fn scan_scroll_delta_maps_all_cardinal_directions() {
    assert_eq!(scan_scroll_delta("up", 4.0).expect("up"), (0.0, 4.0));
    assert_eq!(scan_scroll_delta("down", 4.0).expect("down"), (0.0, -4.0));
    assert_eq!(scan_scroll_delta("left", 4.0).expect("left"), (4.0, 0.0));
    assert_eq!(scan_scroll_delta("right", 4.0).expect("right"), (-4.0, 0.0));
  }

  #[test]
  fn scan_scroll_delta_rejects_unknown_direction() {
    let error = scan_scroll_delta("diagonal", 4.0).expect_err("direction should fail");
    assert!(error.contains("expected down, up, left, or right"));
  }

  #[test]
  fn region_ratios_to_observed_rect_clamps_near_right_edge() {
    let region = RegionRatios {
      left: 0.996,
      top: 0.996,
      right: 1.0,
      bottom: 1.0,
    }
    .to_observed_rect(100, 100)
    .expect("valid dimensions should convert");

    assert_eq!(
      region,
      ObservedRect {
        x: 99,
        y: 99,
        width: 1,
        height: 1,
      }
    );
    assert!(region.x < 100);
    assert!(region.y < 100);
    assert!(region.x + region.width <= 100);
    assert!(region.y + region.height <= 100);
  }

  fn render_json(
    rows: &[ObservedOcrRow],
    region: ObservedRect,
    w: i64,
    h: i64,
  ) -> serde_json::Value {
    let filter = filter_list_row_candidates(rows);
    let json = render_observe_window_region_json(
      rows,
      &filter,
      &region,
      &ScreenshotDimensions {
        width: w,
        height: h,
      },
      std::path::Path::new("/tmp/window.png"),
    )
    .expect("json should render");
    serde_json::from_str(&json).expect("json should parse")
  }

  #[test]
  fn render_observe_window_region_json_includes_coordinate_semantics() {
    let value = render_json(
      &[],
      ObservedRect {
        x: 99,
        y: 10,
        width: 1,
        height: 80,
      },
      100,
      200,
    );
    assert_eq!(
      value["coordinate_space"],
      serde_json::Value::String("window_screenshot_pixels".to_string())
    );
    assert_eq!(value["screenshot_width"], serde_json::Value::from(100));
    assert_eq!(value["screenshot_height"], serde_json::Value::from(200));
  }

  #[test]
  fn render_observe_window_region_json_emits_list_item_candidates() {
    let rows = vec![
      observed_row(0, 100, 120, 500, 160),
      observed_row(1, 100, 340, 700, 64),
      observed_row(2, 120, 460, 700, 84),
      observed_row(3, 120, 588, 700, 86),
      observed_row(4, 120, 716, 700, 82),
    ];
    let value = render_json(
      &rows,
      ObservedRect {
        x: 80,
        y: 100,
        width: 800,
        height: 720,
      },
      1000,
      900,
    );

    assert_eq!(value["row_candidates"].as_array().unwrap().len(), 5);
    assert_eq!(value["item_candidates"].as_array().unwrap().len(), 3);
    assert_eq!(
      value["item_candidates"][0]["row_candidate_index"],
      serde_json::Value::from(2)
    );
    assert_eq!(
      value["rejected_row_candidates"][0]["reject_reason"],
      serde_json::Value::from("height_outside_repeating_row_band")
    );
    assert_eq!(
      value["segmented_regions"][0]["role"],
      serde_json::Value::from("list_region")
    );
  }

  #[test]
  fn row_filter_keeps_varied_music_rows_and_rejects_clear_outliers() {
    let heights = [213, 65, 113, 85, 100, 113, 92, 113, 97, 113, 63];
    let rows = heights
      .iter()
      .enumerate()
      .map(|(index, height)| observed_row(index, 120, 100 + index as i64 * 120, 700, *height))
      .collect::<Vec<_>>();
    let value = render_json(
      &rows,
      ObservedRect {
        x: 80,
        y: 100,
        width: 800,
        height: 1200,
      },
      1000,
      1400,
    );

    let item_row_indices = value["item_candidates"]
      .as_array()
      .unwrap()
      .iter()
      .map(|item| item["row_candidate_index"].as_u64().unwrap())
      .collect::<Vec<_>>();
    let rejected_row_indices = value["rejected_row_candidates"]
      .as_array()
      .unwrap()
      .iter()
      .map(|item| item["row_candidate_index"].as_u64().unwrap())
      .collect::<Vec<_>>();

    assert_eq!(item_row_indices, vec![2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(rejected_row_indices, vec![0, 1, 10]);
  }

  #[test]
  fn classify_segmented_regions_identifies_list_and_header() {
    // Row 0: tall header above the list
    // Rows 1-4: list items with consistent height ~80px
    // Row 5: thin separator (height 4px)
    let rows = vec![
      observed_row(0, 100, 50, 700, 120), // sticky_header candidate
      observed_row(1, 100, 200, 700, 80), // list item
      observed_row(2, 100, 300, 700, 82), // list item
      observed_row(3, 100, 400, 700, 78), // list item
      observed_row(4, 100, 500, 700, 81), // list item
      observed_row(5, 100, 620, 700, 3),  // section_separator (thin)
    ];
    let filter = filter_list_row_candidates(&rows);
    let segments = classify_segmented_regions(&rows, &filter);

    let roles: Vec<&str> = segments.iter().map(|s| s.role.as_str()).collect();
    assert!(
      roles.contains(&"list_region"),
      "should identify list_region: {:?}",
      roles
    );
    assert!(
      roles.contains(&"sticky_header"),
      "should identify sticky_header: {:?}",
      roles
    );
    assert!(
      roles.contains(&"section_separator"),
      "should identify section_separator: {:?}",
      roles
    );
  }

  #[test]
  fn classify_segmented_regions_emits_segmented_region_recognition_result() {
    let rows = vec![
      observed_row(0, 100, 200, 700, 80),
      observed_row(1, 100, 300, 700, 82),
      observed_row(2, 100, 400, 700, 78),
      observed_row(3, 100, 500, 700, 81),
    ];
    let filter = filter_list_row_candidates(&rows);
    let segments = classify_segmented_regions(&rows, &filter);

    let artifact = segmented_region_recognition_artifact(
      "test-segmentation",
      SegmentedRegionArtifactRequest {
        recognition_id: "test_seg_01".to_string(),
        surface: crate::contract::RecognitionSurface::Window,
        segments: &segments,
        row_count: rows.len(),
        screenshot_path: std::path::Path::new("/tmp/test.png"),
        screenshot_dimensions: &ScreenshotDimensions {
          width: 1000,
          height: 800,
        },
        display_ref: Some("display_1"),
        native_display_id: Some("2"),
        app_bundle_id: Some("com.example.app"),
        window_number: Some(42),
        region_hint: None,
        capture_artifact: None,
      },
    )
    .expect("artifact should build");

    let raw = std::fs::read_to_string(&artifact.source_path).expect("artifact file should exist");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("json should parse");

    assert_eq!(json["source"], serde_json::json!("segmented_region"));
    assert_eq!(json["scope"]["surface"], serde_json::json!("window"));
    assert_eq!(json["best"]["kind"], serde_json::json!("list_region"));
    assert_eq!(
      json["detail"]["provider"],
      serde_json::json!("rule_based_segmentation")
    );
    assert_eq!(json["detail"]["row_count"], serde_json::json!(4));
  }

  fn observed_row(index: usize, x: i64, y: i64, width: i64, height: i64) -> ObservedOcrRow {
    ObservedOcrRow {
      row_index: index,
      source: "visual-bands".to_string(),
      bounds: ObservedRect {
        x,
        y,
        width,
        height,
      },
      text_fragments: Vec::new(),
    }
  }
}
