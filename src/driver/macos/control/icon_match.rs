// File: src/driver/macos/control/icon_match.rs
use super::super::*;
use crate::contract::{
  RatioRegion, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
  RecognitionSurface, RecognizedItem,
};
use crate::driver::macos::support::template_match;

pub(crate) fn find_icon_match(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "icon-match".to_string());
  let template_path_str = optional_string(call, "template")
    .ok_or_else(|| "find_icon_match requires --template <path>".to_string())?;
  let template_path = std::path::Path::new(&template_path_str);
  if !template_path.exists() {
    return Err(format!(
      "template file not found: {}",
      template_path.display()
    ));
  }
  let threshold = optional_f64(call, "threshold")?.unwrap_or(0.7);
  if !(0.0..=1.0).contains(&threshold) {
    return Err(format!(
      "invalid --threshold {threshold:.3}: expected 0.0..=1.0"
    ));
  }

  let app_bundle_id = app_identifier(call).filter(|v| looks_like_bundle_identifier(v));
  let capture = super::window_ocr::capture_resolved_window_observation(call, &label)?;

  let search_region =
    parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;

  let (display_ref, native_display_id) = match &capture.capture_contract.capture_source {
    crate::driver::macos::capture::types::CaptureSource::Window {
      display_ref,
      native_display_id,
      ..
    } => (Some(display_ref.as_str()), Some(native_display_id.as_str())),
    _ => (None, None),
  };

  let match_output = template_match::match_template(
    capture.screenshot_path.as_path(),
    template_path,
    search_region.as_ref(),
    threshold,
  )?;

  let items: Vec<RecognizedItem> = match_output
    .matches
    .iter()
    .enumerate()
    .map(|(i, m)| RecognizedItem {
      item_id: format!("icon_match#{}", i + 1),
      kind: "icon".to_string(),
      box_: RecognitionBox {
        x: m.x,
        y: m.y,
        width: m.width,
        height: m.height,
      },
      text: None,
      provider_score: Some(m.score),
      detail: serde_json::json!({
        "ncc_score": m.score,
        "template": template_path_str,
      }),
    })
    .collect();

  let best = items.first().cloned();
  let match_count = items.len();

  let region_hint = search_region.as_ref().map(|r| RatioRegion {
    left: r.x as f64 / capture.dimensions.width as f64,
    top: r.y as f64 / capture.dimensions.height as f64,
    right: (r.x + r.width) as f64 / capture.dimensions.width as f64,
    bottom: (r.y + r.height) as f64 / capture.dimensions.height as f64,
  });

  // Reserve slot 0 for the screenshot so the recognition can cite its
  // ArtifactRef before being pushed itself.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let screenshot_ref = artifacts.ref_at(0);

  let result = RecognitionResult {
    recognition_id: format!("icon_match_{}", sanitize_file_component(&label)),
    source: RecognitionSource::IconMatch,
    scope: RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: display_ref.map(str::to_string),
      native_display_id: native_display_id.map(str::to_string),
      app_bundle_id,
      window_title: None,
      window_number: window_number_from_ref(&capture.capture_source),
      region_hint,
      capture_artifact: Some(screenshot_ref.clone()),
      capture_contract_artifact: None,
    },
    best: best.clone(),
    filtered: items.clone(),
    all: items,
    detail: serde_json::json!({
      "provider": "ncc_template_match",
      "template": template_path_str,
      "threshold": threshold,
      "match_count": match_count,
      "template_size": {
        "width": match_output.template_width,
        "height": match_output.template_height,
      },
      "search_region": {
        "x": match_output.search_x,
        "y": match_output.search_y,
        "width": match_output.search_width,
        "height": match_output.search_height,
      },
      "screenshot": {
        "path": capture.screenshot_path.display().to_string(),
        "width": capture.dimensions.width,
        "height": capture.dimensions.height,
      },
    }),
    evidence: vec![screenshot_ref.clone()],
    known_limits: vec![
      "grayscale NCC only: color and alpha channels are ignored".to_string(),
      "no scale or rotation invariance: template must match screenshot resolution".to_string(),
    ],
  };

  let recognition_json = serde_json::to_string_pretty(&result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|e| format!("failed to encode icon match result: {e}"))?;

  let recognition_artifact = build_text_artifact(
    "icon-match-recognition",
    "json",
    &format!("{}-recognition", sanitize_file_component(&label)),
    recognition_json,
    "NCC template match recognition result.",
  )?;

  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: capture.screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Window screenshot used for icon template matching.".to_string()),
  };

  // Push in slot order: must match `ref_at(0)` reservation.
  artifacts.push(screenshot_artifact);
  artifacts.push(recognition_artifact);

  Ok(DriverResponse {
    summary: format!(
      "Icon template match: found {} match(es) above threshold {:.2}.",
      match_count, threshold
    ),
    backend: Some("macos.template.find-icon-match".to_string()),
    signals: {
      let mut s = std::collections::BTreeMap::new();
      s.insert("match_count".to_string(), match_count.to_string());
      s.insert(
        "best_score".to_string(),
        best
          .as_ref()
          .and_then(|b| b.provider_score)
          .map(|sc| format!("{sc:.3}"))
          .unwrap_or_else(|| "none".to_string()),
      );
      s
    },
    notes: vec![
      format!("template={template_path_str}"),
      format!("threshold={threshold:.3}"),
      format!("matchCount={match_count}"),
      format!(
        "screenshotPixels={}x{}",
        capture.dimensions.width, capture.dimensions.height
      ),
    ],
    artifacts: artifacts.into_vec(),
  })
}
