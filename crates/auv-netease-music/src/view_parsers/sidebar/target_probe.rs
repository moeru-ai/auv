use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::scroll::policies::detection_motion::{MotionDetectionPolicy, MotionEvidence};
use crate::view_parsers::sidebar::classify_sidebar_text;
use crate::{
  SidebarCandidateKind, SidebarViewportCandidate, SidebarViewportObservation, ViewBounds,
  normalize_identity,
};
use auv_driver::RatioRect;
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};

const OCR_TEXT_PREVIEW_LIMIT: usize = 200;

pub(crate) const PROBE_SIDEBAR_ENHANCED_V1: &str = "probe_sidebar_enhanced_v1";
pub(crate) const PROBE_FULL_WINDOW_FALLBACK_V1: &str = "probe_full_window_fallback_v1";
pub(crate) const LS_OCR_FULL_WINDOW_FALLBACK_NOTE: &str = "ls_ocr_full_window_fallback";

const PROBE_DEFAULT_RECOGNITION_LANGUAGES: &[&str] = &["zh-Hans", "en-US"];
// NOTICE(a6c-7d): live Case B had playlist OCR bbox y=809 vs sidebar_bounds bottom=808.
pub(crate) const PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING: f64 = 48.0;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SidebarTargetMissReason {
  NoEvidenceNodes,
  NoPlaylistItems {
    visible_labels: Vec<String>,
  },
  LabelNotMatched {
    playlist_labels: Vec<String>,
    ocr_contains_target: Vec<String>,
    misclassified: Vec<MisclassifiedSidebarText>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MisclassifiedSidebarText {
  pub label: String,
  pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbe {
  pub observation_index: usize,
  pub evidence_count: usize,
  pub playlist_item_count: usize,
  pub viewport_fingerprint: String,
  pub result: Option<ViewBounds>,
  pub miss_reason: Option<SidebarTargetMissReason>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub artifact_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PrecedingScrollContext {
  pub step_name: String,
  pub delta_y: f64,
  pub policy: String,
  pub settle_ms: u64,
  pub delivery_path: Option<String>,
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeScrollContext {
  pub phase: String,
  pub attempt: usize,
  pub scroll_anchor: (f64, f64),
  pub preceding_scroll: Option<PrecedingScrollContext>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeOcrContext {
  pub profile: String,
  pub options: TextRecognitionOptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeCaptureContext {
  pub capture_bounds: ViewBounds,
  pub scale_factor: f64,
  pub sidebar_bounds: ViewBounds,
  pub sidebar_ratio: RatioRect,
  pub crop_pixel_size: (u32, u32),
  pub ocr_region_count: usize,
  pub ocr_text_preview: String,
  pub evidence_count: usize,
  pub scroll_motion: Option<MotionEvidence>,
  pub ocr_profile: String,
  pub ocr_recognition_languages: Option<Vec<String>>,
  pub ocr_custom_word_count: usize,
  pub parse_viewport_bounds: ViewBounds,
  pub ocr_regions_below_sidebar_bottom: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeArtifactPaths {
  pub probe_json: String,
  pub window_png: String,
  pub sidebar_crop_png: String,
  pub recognition_json: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeOutcome {
  pub probe: SidebarTargetProbe,
  pub capture_context: SidebarTargetProbeCaptureContext,
  pub artifact_paths: SidebarTargetProbeArtifactPaths,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SidebarTargetProbeArtifact {
  probe: SidebarTargetProbe,
  candidates: Vec<SidebarTargetProbeCandidateSummary>,
  scroll_context: SidebarTargetProbeScrollContext,
  capture_context: SidebarTargetProbeCaptureContext,
  artifact_paths: SidebarTargetProbeArtifactPaths,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SidebarTargetProbeCandidateSummary {
  id: String,
  kind: String,
  label: Option<String>,
}

pub(crate) fn preceding_scroll_context(
  step_name: impl Into<String>,
  delta_y: f64,
  policy: impl Into<String>,
  settle_ms: u64,
  delivery_path: Option<String>,
  fallback_reason: Option<String>,
) -> PrecedingScrollContext {
  PrecedingScrollContext {
    step_name: step_name.into(),
    delta_y,
    policy: policy.into(),
    settle_ms,
    delivery_path,
    fallback_reason,
  }
}

pub(crate) fn merge_custom_words(base: &[String], words: &[&str]) -> Vec<String> {
  let mut custom_words = base.to_vec();
  for word in words {
    let word = word.trim();
    if word.is_empty() {
      continue;
    }
    if !custom_words.iter().any(|existing| existing == word) {
      custom_words.push(word.to_string());
    }
  }
  custom_words
}

pub(crate) fn build_sidebar_target_probe_ocr_options(
  base: &TextRecognitionOptions,
  target_label: &str,
  query: &str,
) -> TextRecognitionOptions {
  TextRecognitionOptions {
    custom_words: merge_custom_words(&base.custom_words, &[target_label, query]),
    recognition_languages: base.recognition_languages.clone().or_else(|| {
      Some(
        PROBE_DEFAULT_RECOGNITION_LANGUAGES
          .iter()
          .map(|language| (*language).to_string())
          .collect(),
      )
    }),
  }
}

pub(crate) fn resolve_probe_ocr_profile_after_sidebar(sidebar_region_count: usize) -> &'static str {
  if sidebar_region_count > 0 {
    PROBE_SIDEBAR_ENHANCED_V1
  } else {
    PROBE_FULL_WINDOW_FALLBACK_V1
  }
}

pub(crate) fn probe_parse_viewport_bounds(
  sidebar_bounds: ViewBounds,
  ocr_profile: &str,
) -> ViewBounds {
  if ocr_profile == PROBE_FULL_WINDOW_FALLBACK_V1 {
    ViewBounds::new(
      sidebar_bounds.x,
      sidebar_bounds.y,
      sidebar_bounds.width,
      sidebar_bounds.height + PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING,
    )
  } else {
    sidebar_bounds
  }
}

pub(crate) fn ls_parse_viewport_bounds_for_sidebar_ocr(
  sidebar_bounds: ViewBounds,
  sidebar_region_count: usize,
  numeric_query: bool,
) -> ViewBounds {
  if numeric_query && sidebar_region_count == 0 {
    probe_parse_viewport_bounds(sidebar_bounds, PROBE_FULL_WINDOW_FALLBACK_V1)
  } else {
    sidebar_bounds
  }
}

pub(crate) fn count_ocr_regions_below_sidebar_bottom(
  recognition: &TextRecognition,
  sidebar_bounds: ViewBounds,
  parse_viewport_bounds: ViewBounds,
) -> usize {
  let sidebar_bottom = sidebar_bounds.y + sidebar_bounds.height;
  let parse_bottom = parse_viewport_bounds.y + parse_viewport_bounds.height;
  recognition
    .regions
    .iter()
    .filter(|region| {
      let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
      center_y > sidebar_bottom && center_y <= parse_bottom
    })
    .count()
}

pub(crate) fn build_probe_capture_context(
  capture_bounds: ViewBounds,
  scale_factor: f64,
  sidebar_bounds: ViewBounds,
  sidebar_ratio: RatioRect,
  recognition: &TextRecognition,
  observation: &SidebarViewportObservation,
  crop_pixel_size: (u32, u32),
  scroll_motion: Option<MotionEvidence>,
  ocr_context: &SidebarTargetProbeOcrContext,
  parse_viewport_bounds: ViewBounds,
) -> SidebarTargetProbeCaptureContext {
  SidebarTargetProbeCaptureContext {
    capture_bounds,
    scale_factor,
    sidebar_bounds,
    sidebar_ratio,
    crop_pixel_size,
    ocr_region_count: recognition.regions.len(),
    ocr_text_preview: truncate_ocr_preview(&recognition.text),
    evidence_count: observation.evidence_nodes.len(),
    scroll_motion,
    ocr_profile: ocr_context.profile.clone(),
    ocr_recognition_languages: ocr_context.options.recognition_languages.clone(),
    ocr_custom_word_count: ocr_context.options.custom_words.len(),
    parse_viewport_bounds,
    ocr_regions_below_sidebar_bottom: count_ocr_regions_below_sidebar_bottom(
      recognition,
      sidebar_bounds,
      parse_viewport_bounds,
    ),
  }
}

pub(crate) fn analyze_sidebar_target_probe(
  observation: &SidebarViewportObservation,
  target_label: &str,
  query: &str,
) -> SidebarTargetProbe {
  let target_identity = normalize_identity(target_label);
  let query_identity = normalize_identity(query);
  let playlist_items = observation
    .candidates
    .iter()
    .filter(|candidate| candidate.kind == SidebarCandidateKind::PlaylistItem)
    .collect::<Vec<_>>();

  let result = playlist_items
    .iter()
    .filter_map(|candidate| matching_playlist_bounds(candidate, &target_identity, &query_identity))
    .next();

  let miss_reason = result.is_none().then(|| {
    if observation.evidence_nodes.is_empty() {
      return SidebarTargetMissReason::NoEvidenceNodes;
    }

    if playlist_items.is_empty() {
      let visible_labels = observation
        .evidence_nodes
        .iter()
        .filter_map(|node| node.label.clone())
        .collect();
      return SidebarTargetMissReason::NoPlaylistItems { visible_labels };
    }

    let playlist_labels = playlist_items
      .iter()
      .filter_map(|candidate| candidate.label.clone())
      .collect();
    let ocr_contains_target = ocr_labels_containing_target(
      &observation.evidence_nodes,
      &target_identity,
      &query_identity,
    );
    let misclassified = misclassified_target_evidence(
      &observation.evidence_nodes,
      &target_identity,
      &query_identity,
    );
    SidebarTargetMissReason::LabelNotMatched {
      playlist_labels,
      ocr_contains_target,
      misclassified,
    }
  });

  SidebarTargetProbe {
    observation_index: observation.observation_index,
    evidence_count: observation.evidence_nodes.len(),
    playlist_item_count: playlist_items.len(),
    viewport_fingerprint: observation.viewport_fingerprint.clone(),
    result,
    miss_reason,
    artifact_path: None,
  }
}

pub(crate) fn write_sidebar_target_probe_artifacts(
  artifact_dir: &std::path::Path,
  artifact_stem: &str,
  window_image: &RgbaImage,
  sidebar_crop: &RgbaImage,
  recognition: &TextRecognition,
  observation: &SidebarViewportObservation,
  probe: &SidebarTargetProbe,
  scroll_context: &SidebarTargetProbeScrollContext,
  capture_context: &SidebarTargetProbeCaptureContext,
) -> Result<SidebarTargetProbeArtifactPaths, String> {
  // NOTICE(a6c-7): probe image + recognition artifacts for ROI vs motion bisection.
  std::fs::create_dir_all(artifact_dir)
    .map_err(|error| format!("failed to create {}: {error}", artifact_dir.display()))?;

  let window_png = artifact_dir.join(format!("{artifact_stem}-window.png"));
  let sidebar_crop_png = artifact_dir.join(format!("{artifact_stem}-sidebar-crop.png"));
  let recognition_json = artifact_dir.join(format!("{artifact_stem}-recognition.json"));
  let probe_json = artifact_dir.join(format!("{artifact_stem}.json"));

  window_image
    .save(&window_png)
    .map_err(|error| format!("failed to save {}: {error}", window_png.display()))?;
  sidebar_crop
    .save(&sidebar_crop_png)
    .map_err(|error| format!("failed to save {}: {error}", sidebar_crop_png.display()))?;
  std::fs::write(
    &recognition_json,
    serde_json::to_string_pretty(recognition)
      .map_err(|error| format!("failed to serialize recognition: {error}"))?,
  )
  .map_err(|error| format!("failed to write {}: {error}", recognition_json.display()))?;

  let artifact_paths = SidebarTargetProbeArtifactPaths {
    probe_json: probe_json.display().to_string(),
    window_png: window_png.display().to_string(),
    sidebar_crop_png: sidebar_crop_png.display().to_string(),
    recognition_json: recognition_json.display().to_string(),
  };
  let payload = SidebarTargetProbeArtifact {
    probe: probe.clone(),
    candidates: observation
      .candidates
      .iter()
      .map(|candidate| SidebarTargetProbeCandidateSummary {
        id: candidate.id.clone(),
        kind: format!("{:?}", candidate.kind),
        label: candidate.label.clone(),
      })
      .collect(),
    scroll_context: scroll_context.clone(),
    capture_context: capture_context.clone(),
    artifact_paths: artifact_paths.clone(),
  };
  std::fs::write(
    &probe_json,
    serde_json::to_string_pretty(&payload)
      .map_err(|error| format!("failed to serialize sidebar target probe: {error}"))?,
  )
  .map_err(|error| format!("failed to write {}: {error}", probe_json.display()))?;

  Ok(artifact_paths)
}

pub(crate) fn sidebar_target_probe_diagnostic_message(
  phase: &str,
  attempt: usize,
  outcome: &SidebarTargetProbeOutcome,
) -> String {
  let probe = &outcome.probe;
  let capture_context = &outcome.capture_context;
  let artifact_paths = &outcome.artifact_paths;
  let miss = probe
    .miss_reason
    .as_ref()
    .map(|reason| serde_json::to_string(reason).unwrap_or_else(|_| format!("{reason:?}")))
    .unwrap_or_else(|| "null".to_string());
  serde_json::json!({
    "phase": phase,
    "attempt": attempt,
    "evidence_count": probe.evidence_count,
    "playlist_item_count": probe.playlist_item_count,
    "ocr_region_count": capture_context.ocr_region_count,
    "crop_w": capture_context.crop_pixel_size.0,
    "crop_h": capture_context.crop_pixel_size.1,
    "viewport_fingerprint": probe.viewport_fingerprint,
    "found": probe.result.is_some(),
    "scroll_motion_no_motion": capture_context.scroll_motion.as_ref().map(|motion| motion.no_motion),
    "ocr_profile": capture_context.ocr_profile,
    "ocr_custom_word_count": capture_context.ocr_custom_word_count,
    "ocr_recognition_languages": capture_context.ocr_recognition_languages,
    "parse_viewport_bottom": capture_context.parse_viewport_bounds.y
      + capture_context.parse_viewport_bounds.height,
    "ocr_regions_below_sidebar_bottom": capture_context.ocr_regions_below_sidebar_bottom,
    "miss_reason": serde_json::from_str::<serde_json::Value>(&miss).unwrap_or(serde_json::Value::Null),
    "artifact_path": probe.artifact_path,
    "window_png": artifact_paths.window_png,
    "sidebar_crop_png": artifact_paths.sidebar_crop_png,
  })
  .to_string()
}

fn truncate_ocr_preview(text: &str) -> String {
  if text.chars().count() <= OCR_TEXT_PREVIEW_LIMIT {
    return text.to_string();
  }
  text
    .chars()
    .take(OCR_TEXT_PREVIEW_LIMIT)
    .collect::<String>()
    + "..."
}

fn capture_view_bounds(capture: &auv_driver::Capture) -> ViewBounds {
  ViewBounds::new(
    capture.bounds.origin.x,
    capture.bounds.origin.y,
    capture.bounds.size.width,
    capture.bounds.size.height,
  )
}

fn matching_playlist_bounds(
  candidate: &SidebarViewportCandidate,
  target_identity: &str,
  query_identity: &str,
) -> Option<ViewBounds> {
  let label = candidate.label.as_deref()?;
  let bounds = candidate.bounds?;
  label_matches_target(label, target_identity, query_identity).then_some(bounds)
}

fn label_matches_target(label: &str, target_identity: &str, _query_identity: &str) -> bool {
  normalize_identity(label) == target_identity
}

fn ocr_labels_containing_target(
  evidence_nodes: &[crate::ViewEvidenceNode],
  target_identity: &str,
  query_identity: &str,
) -> Vec<String> {
  evidence_nodes
    .iter()
    .filter_map(|node| node.label.as_deref())
    .filter(|label| label_matches_target(label, target_identity, query_identity))
    .map(str::to_string)
    .collect()
}

fn misclassified_target_evidence(
  evidence_nodes: &[crate::ViewEvidenceNode],
  target_identity: &str,
  query_identity: &str,
) -> Vec<MisclassifiedSidebarText> {
  evidence_nodes
    .iter()
    .filter_map(|node| {
      let label = node.label.as_deref()?.trim();
      if !label_matches_target(label, target_identity, query_identity) {
        return None;
      }
      let bounds = node.bounds?;
      let kind = classify_sidebar_text(label, bounds.x);
      if kind == SidebarCandidateKind::PlaylistItem {
        return None;
      }
      Some(MisclassifiedSidebarText {
        label: label.to_string(),
        kind: format!("{kind:?}"),
      })
    })
    .collect()
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_sidebar_target_probe(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  sidebar_bounds: ViewBounds,
  inputs: &crate::Inputs,
  observation_index: usize,
  target_label: &str,
  query: &str,
  artifact_dir: &std::path::Path,
  artifact_stem: &str,
  scroll_context: SidebarTargetProbeScrollContext,
  previous_sidebar_crop: &mut Option<RgbaImage>,
) -> Result<SidebarTargetProbeOutcome, String> {
  let capture = session
    .window()
    .capture(window)
    .map_err(|error| format!("sidebar target probe capture failed: {error}"))?;
  let sidebar_ratio = crate::bounds_to_ratio(sidebar_bounds, &capture);
  let ocr_options =
    build_sidebar_target_probe_ocr_options(&inputs.ocr_options, target_label, query);
  let sidebar_recognition = session
    .vision()
    .recognize_text_in_capture_with_options(&capture, sidebar_ratio, ocr_options.clone())
    .map_err(|error| format!("sidebar target probe OCR failed: {error}"))?;
  let sidebar_region_count = sidebar_recognition.regions.len();
  let ocr_context = SidebarTargetProbeOcrContext {
    profile: resolve_probe_ocr_profile_after_sidebar(sidebar_region_count).to_string(),
    options: ocr_options.clone(),
  };
  let recognition = if sidebar_region_count > 0 {
    crate::recognition_in_window_space(sidebar_recognition, &capture)
  } else {
    let full_window = RatioRect::new(0.0, 0.0, 1.0, 1.0);
    let fallback_recognition = session
      .vision()
      .recognize_text_in_capture_with_options(&capture, full_window, ocr_options)
      .map_err(|error| format!("sidebar target probe full-window OCR failed: {error}"))?;
    crate::recognition_in_window_space(fallback_recognition, &capture)
  };
  let parse_viewport = probe_parse_viewport_bounds(sidebar_bounds, &ocr_context.profile);
  let observation = crate::view_parsers::sidebar::parse::parse_sidebar_viewport(
    observation_index,
    parse_viewport,
    &recognition,
  );
  let sidebar_crop = crate::crop_image(&capture.image, sidebar_bounds, capture.scale_factor);
  let scroll_motion = previous_sidebar_crop
    .as_ref()
    .map(|previous| MotionDetectionPolicy::default().compare(previous, &sidebar_crop));
  *previous_sidebar_crop = Some(sidebar_crop.clone());

  let capture_context = build_probe_capture_context(
    capture_view_bounds(&capture),
    capture.scale_factor,
    sidebar_bounds,
    sidebar_ratio,
    &recognition,
    &observation,
    (sidebar_crop.width(), sidebar_crop.height()),
    scroll_motion,
    &ocr_context,
    parse_viewport,
  );
  let mut probe = analyze_sidebar_target_probe(&observation, target_label, query);
  let artifact_paths = write_sidebar_target_probe_artifacts(
    artifact_dir,
    artifact_stem,
    &capture.image,
    &sidebar_crop,
    &recognition,
    &observation,
    &probe,
    &scroll_context,
    &capture_context,
  )?;
  probe.artifact_path = Some(artifact_paths.probe_json.clone());

  Ok(SidebarTargetProbeOutcome {
    probe,
    capture_context,
    artifact_paths,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::view_parsers::sidebar::parse::parse_sidebar_viewport;
  use crate::view_parsers::sidebar::test_support::fake_recognition;
  use auv_driver::RatioRect;

  fn sample_sidebar_bounds() -> ViewBounds {
    ViewBounds::new(0.0, 469.8, 320.0, 338.2)
  }

  fn sample_probe_ocr_context() -> SidebarTargetProbeOcrContext {
    SidebarTargetProbeOcrContext {
      profile: PROBE_SIDEBAR_ENHANCED_V1.to_string(),
      options: build_sidebar_target_probe_ocr_options(
        &TextRecognitionOptions::default(),
        "16",
        "16",
      ),
    }
  }

  #[test]
  fn build_sidebar_target_probe_ocr_options_includes_target_and_query_custom_words() {
    let options = build_sidebar_target_probe_ocr_options(
      &TextRecognitionOptions::default().with_custom_words(["绚香"]),
      "16",
      "16",
    );

    assert_eq!(
      options.custom_words,
      vec!["绚香".to_string(), "16".to_string()]
    );
  }

  #[test]
  fn build_sidebar_target_probe_ocr_options_preserves_cli_languages() {
    let base = TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
    let options = build_sidebar_target_probe_ocr_options(&base, "16", "16");

    assert_eq!(
      options.recognition_languages,
      Some(vec!["ja-JP".to_string()])
    );
  }

  #[test]
  fn build_sidebar_target_probe_ocr_options_sets_default_languages_when_absent() {
    let options =
      build_sidebar_target_probe_ocr_options(&TextRecognitionOptions::default(), "16", "16");

    assert_eq!(
      options.recognition_languages,
      Some(vec!["zh-Hans".to_string(), "en-US".to_string()])
    );
  }

  #[test]
  fn resolve_probe_ocr_profile_prefers_fallback_when_sidebar_empty() {
    assert_eq!(
      resolve_probe_ocr_profile_after_sidebar(0),
      PROBE_FULL_WINDOW_FALLBACK_V1
    );
    assert_eq!(
      resolve_probe_ocr_profile_after_sidebar(1),
      PROBE_SIDEBAR_ENHANCED_V1
    );
  }

  #[test]
  fn probe_parse_viewport_bounds_extends_bottom_on_full_window_fallback() {
    let sidebar_bounds = sample_sidebar_bounds();
    let expanded = probe_parse_viewport_bounds(sidebar_bounds, PROBE_FULL_WINDOW_FALLBACK_V1);

    assert_eq!(expanded.x, sidebar_bounds.x);
    assert_eq!(expanded.y, sidebar_bounds.y);
    assert_eq!(expanded.width, sidebar_bounds.width);
    assert_eq!(
      expanded.height,
      sidebar_bounds.height + PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING
    );
  }

  #[test]
  fn probe_parse_viewport_bounds_unchanged_on_sidebar_profile() {
    let sidebar_bounds = sample_sidebar_bounds();
    let viewport = probe_parse_viewport_bounds(sidebar_bounds, PROBE_SIDEBAR_ENHANCED_V1);

    assert_eq!(viewport, sidebar_bounds);
  }

  #[test]
  fn ls_parse_viewport_bounds_uses_full_window_padding_for_empty_numeric_query() {
    let sidebar_bounds = sample_sidebar_bounds();
    let expanded = ls_parse_viewport_bounds_for_sidebar_ocr(sidebar_bounds, 0, true);

    assert_eq!(
      expanded.height,
      sidebar_bounds.height + PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING
    );
  }

  #[test]
  fn ls_parse_viewport_bounds_unchanged_when_sidebar_has_regions() {
    let sidebar_bounds = sample_sidebar_bounds();
    let viewport = ls_parse_viewport_bounds_for_sidebar_ocr(sidebar_bounds, 3, true);

    assert_eq!(viewport, sidebar_bounds);
  }

  #[test]
  fn probe_parse_includes_playlist_row_below_sidebar_bottom() {
    let sidebar_bounds = sample_sidebar_bounds();
    let recognition = fake_recognition(vec![
      ("4", 71.0, 609.0, 11.0, 13.0),
      ("收藏的歌单1へ", 33.0, 809.0, 88.0, 16.0),
    ]);
    let strict = parse_sidebar_viewport(0, sidebar_bounds, &recognition);
    let expanded = parse_sidebar_viewport(
      0,
      probe_parse_viewport_bounds(sidebar_bounds, PROBE_FULL_WINDOW_FALLBACK_V1),
      &recognition,
    );

    assert_eq!(strict.evidence_nodes.len(), 1);
    assert_eq!(strict.evidence_nodes[0].label.as_deref(), Some("4"));
    assert!(expanded.evidence_nodes.len() >= 2);
    assert!(
      expanded
        .candidates
        .iter()
        .any(|candidate| candidate.label.as_deref() == Some("收藏的歌单1へ"))
    );
  }

  #[test]
  fn probe_parse_viewport_keeps_player_bar_outside() {
    let recognition = fake_recognition(vec![("Reverberation", 98.0, 994.0, 160.0, 20.0)]);
    let sidebar_bounds = ViewBounds::new(0.0, 443.0, 344.0, 528.0);
    let observation = parse_sidebar_viewport(0, sidebar_bounds, &recognition);

    assert!(observation.evidence_nodes.is_empty());
  }

  #[test]
  fn build_probe_capture_context_reports_ocr_vs_evidence_split() {
    let sidebar_bounds = sample_sidebar_bounds();
    let recognition = fake_recognition(vec![("16", 70.0, 42.0, 14.0, 11.0)]);
    let observation = parse_sidebar_viewport(0, sidebar_bounds, &recognition);
    let ocr_context = sample_probe_ocr_context();
    let parse_viewport = probe_parse_viewport_bounds(sidebar_bounds, &ocr_context.profile);
    let capture_context = build_probe_capture_context(
      ViewBounds::new(0.0, 0.0, 1512.0, 890.0),
      2.0,
      sidebar_bounds,
      RatioRect::new(0.0, 0.527, 0.21, 0.38),
      &recognition,
      &observation,
      (640, 676),
      None,
      &ocr_context,
      parse_viewport,
    );

    assert_eq!(capture_context.ocr_region_count, 1);
    assert_eq!(capture_context.evidence_count, 0);
    assert_eq!(capture_context.crop_pixel_size, (640, 676));
    assert_eq!(capture_context.ocr_profile, PROBE_SIDEBAR_ENHANCED_V1);
    assert_eq!(capture_context.ocr_custom_word_count, 1);
  }

  #[test]
  fn write_sidebar_target_probe_artifact_includes_capture_context() {
    let sidebar_bounds = sample_sidebar_bounds();
    let recognition = fake_recognition(vec![("16", 70.0, 512.0, 14.0, 11.0)]);
    let observation = parse_sidebar_viewport(0, sidebar_bounds, &recognition);
    let probe = analyze_sidebar_target_probe(&observation, "16", "16");
    let ocr_context = sample_probe_ocr_context();
    let parse_viewport = probe_parse_viewport_bounds(sidebar_bounds, &ocr_context.profile);
    let capture_context = build_probe_capture_context(
      ViewBounds::new(0.0, 0.0, 1512.0, 890.0),
      2.0,
      sidebar_bounds,
      RatioRect::new(0.0, 0.527, 0.21, 0.38),
      &recognition,
      &observation,
      (8, 8),
      None,
      &ocr_context,
      parse_viewport,
    );
    let scroll_context = SidebarTargetProbeScrollContext {
      phase: "rescan".to_string(),
      attempt: 0,
      scroll_anchor: (160.0, 723.45),
      preceding_scroll: Some(preceding_scroll_context(
        "scroll-sidebar-top-11",
        960.0,
        "background_preferred",
        120,
        Some("window_targeted_wheel".to_string()),
        Some("AX scroll is not implemented in this slice".to_string()),
      )),
    };
    let artifact_dir =
      std::env::temp_dir().join(format!("auv-probe-artifact-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&artifact_dir);
    let window_image = RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
    let sidebar_crop = RgbaImage::from_pixel(8, 8, image::Rgba([4, 5, 6, 255]));

    let artifact_paths = write_sidebar_target_probe_artifacts(
      &artifact_dir,
      "rescan-reobserve-00",
      &window_image,
      &sidebar_crop,
      &recognition,
      &observation,
      &probe,
      &scroll_context,
      &capture_context,
    )
    .expect("artifact write");

    let payload: serde_json::Value =
      serde_json::from_str(&std::fs::read_to_string(&artifact_paths.probe_json).expect("read"))
        .expect("json");
    assert!(payload.get("capture_context").is_some());
    assert!(payload.get("scroll_context").is_some());
    assert!(payload.get("artifact_paths").is_some());
    assert_eq!(
      payload["capture_context"]["ocr_region_count"],
      serde_json::json!(1)
    );
    assert_eq!(
      payload["capture_context"]["ocr_profile"],
      serde_json::json!(PROBE_SIDEBAR_ENHANCED_V1)
    );
    assert!(
      payload["capture_context"]
        .get("parse_viewport_bounds")
        .is_some()
    );
    let _ = std::fs::remove_dir_all(&artifact_dir);
  }

  #[test]
  fn sidebar_target_probe_diagnostic_includes_ocr_region_count() {
    let sidebar_bounds = sample_sidebar_bounds();
    let recognition = fake_recognition(vec![("16", 70.0, 512.0, 14.0, 11.0)]);
    let observation = parse_sidebar_viewport(0, sidebar_bounds, &recognition);
    let probe = analyze_sidebar_target_probe(&observation, "16", "16");
    let ocr_context = sample_probe_ocr_context();
    let parse_viewport = probe_parse_viewport_bounds(sidebar_bounds, &ocr_context.profile);
    let outcome = SidebarTargetProbeOutcome {
      probe,
      capture_context: build_probe_capture_context(
        ViewBounds::new(0.0, 0.0, 1512.0, 890.0),
        2.0,
        sidebar_bounds,
        RatioRect::new(0.0, 0.527, 0.21, 0.38),
        &recognition,
        &observation,
        (640, 676),
        Some(MotionEvidence {
          estimated_shift_y: 0,
          normalized_diff: 0.0,
          no_motion: true,
        }),
        &ocr_context,
        parse_viewport,
      ),
      artifact_paths: SidebarTargetProbeArtifactPaths {
        probe_json: "/tmp/rescan-reobserve-00.json".to_string(),
        window_png: "/tmp/rescan-reobserve-00-window.png".to_string(),
        sidebar_crop_png: "/tmp/rescan-reobserve-00-sidebar-crop.png".to_string(),
        recognition_json: "/tmp/rescan-reobserve-00-recognition.json".to_string(),
      },
    };

    let message = sidebar_target_probe_diagnostic_message("rescan", 0, &outcome);
    let payload: serde_json::Value = serde_json::from_str(&message).expect("diagnostic json");
    assert_eq!(payload["ocr_region_count"], serde_json::json!(1));
    assert_eq!(
      payload["ocr_profile"],
      serde_json::json!(PROBE_SIDEBAR_ENHANCED_V1)
    );
    assert_eq!(payload["ocr_custom_word_count"], serde_json::json!(1));
    assert_eq!(payload["parse_viewport_bottom"], serde_json::json!(808.0));
    assert_eq!(
      payload["ocr_regions_below_sidebar_bottom"],
      serde_json::json!(0)
    );
    assert_eq!(payload["crop_w"], serde_json::json!(640));
    assert_eq!(payload["crop_h"], serde_json::json!(676));
    assert_eq!(payload["scroll_motion_no_motion"], serde_json::json!(true));
    assert_eq!(
      payload["sidebar_crop_png"],
      serde_json::json!("/tmp/rescan-reobserve-00-sidebar-crop.png")
    );
  }

  #[test]
  fn label_matches_target_requires_exact_identity() {
    assert!(label_matches_target("3", "3", "3"));
    assert!(!label_matches_target("43", "3", "3"));
    assert!(!label_matches_target("13", "3", "3"));
  }
}
