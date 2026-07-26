use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use crate::scroll::policies::detection_motion::MotionDetectionPolicy;
use crate::scroll::policies::detection_motion::MotionEvidence;
use crate::view_parsers::sidebar::classify_sidebar_text;
use crate::{SidebarCandidateKind, SidebarViewportObservation, ViewBounds, normalize_identity};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SidebarTargetProbeOutcome {
  pub probe: SidebarTargetProbe,
  pub capture_context: SidebarTargetProbeCaptureContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SidebarTargetProbeArtifact {
  probe: SidebarTargetProbe,
  candidates: Vec<SidebarTargetProbeCandidateSummary>,
  scroll_context: SidebarTargetProbeScrollContext,
  capture_context: SidebarTargetProbeCaptureContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SidebarTargetProbeCandidateSummary {
  id: String,
  kind: String,
  label: Option<String>,
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
    recognition_languages: base
      .recognition_languages
      .clone()
      .or_else(|| Some(PROBE_DEFAULT_RECOGNITION_LANGUAGES.iter().map(|language| (*language).to_string()).collect())),
  }
}

pub(crate) fn resolve_probe_ocr_profile_after_sidebar(sidebar_region_count: usize) -> &'static str {
  if sidebar_region_count > 0 {
    PROBE_SIDEBAR_ENHANCED_V1
  } else {
    PROBE_FULL_WINDOW_FALLBACK_V1
  }
}

pub(crate) fn probe_parse_viewport_bounds(sidebar_bounds: ViewBounds, ocr_profile: &str) -> ViewBounds {
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
    ocr_regions_below_sidebar_bottom: count_ocr_regions_below_sidebar_bottom(recognition, sidebar_bounds, parse_viewport_bounds),
  }
}

pub(crate) fn analyze_sidebar_target_probe(observation: &SidebarViewportObservation, target_label: &str, query: &str) -> SidebarTargetProbe {
  let target_identity = normalize_identity(target_label);
  let query_identity = normalize_identity(query);
  let playlist_items =
    observation.candidates.iter().filter(|candidate| candidate.kind == SidebarCandidateKind::PlaylistItem).collect::<Vec<_>>();

  let result = playlist_items.iter().find_map(|candidate| {
    let label = candidate.label.as_deref()?;
    let bounds = candidate.bounds?;
    label_matches_target(label, &target_identity, &query_identity).then_some(bounds)
  });

  let miss_reason = result.is_none().then(|| {
    if observation.evidence_nodes.is_empty() {
      return SidebarTargetMissReason::NoEvidenceNodes;
    }

    if playlist_items.is_empty() {
      let visible_labels = observation.evidence_nodes.iter().filter_map(|node| node.label.clone()).collect();
      return SidebarTargetMissReason::NoPlaylistItems { visible_labels };
    }

    let playlist_labels = playlist_items.iter().filter_map(|candidate| candidate.label.clone()).collect();
    let ocr_contains_target = ocr_labels_containing_target(&observation.evidence_nodes, &target_identity, &query_identity);
    let misclassified = misclassified_target_evidence(&observation.evidence_nodes, &target_identity, &query_identity);
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
  }
}

pub(crate) fn publish_sidebar_target_probe_artifacts(
  window_image: &RgbaImage,
  sidebar_crop: &RgbaImage,
  recognition: &TextRecognition,
  observation: &SidebarViewportObservation,
  probe: &SidebarTargetProbe,
  scroll_context: &SidebarTargetProbeScrollContext,
  capture_context: &SidebarTargetProbeCaptureContext,
) {
  // NOTICE(a6c-7): probe image + recognition artifacts for ROI vs motion bisection.
  let payload = sidebar_target_probe_artifact(observation, probe, scroll_context, capture_context);
  auv_tracing::in_span!("auv.netease.sidebar_target_probe.evidence", || {
    crate::telemetry::png_artifact("auv.netease.sidebar_target_probe.window_capture", window_image);
    crate::telemetry::png_artifact("auv.netease.sidebar_target_probe.sidebar_crop", sidebar_crop);
    crate::telemetry::json_artifact("auv.netease.sidebar_target_probe.recognition", recognition);
    crate::telemetry::json_artifact("auv.netease.sidebar_target_probe.result", &payload);
  });
}

fn sidebar_target_probe_artifact(
  observation: &SidebarViewportObservation,
  probe: &SidebarTargetProbe,
  scroll_context: &SidebarTargetProbeScrollContext,
  capture_context: &SidebarTargetProbeCaptureContext,
) -> SidebarTargetProbeArtifact {
  SidebarTargetProbeArtifact {
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
  }
}

pub(crate) fn sidebar_target_probe_diagnostic_message(phase: &str, attempt: usize, outcome: &SidebarTargetProbeOutcome) -> String {
  let probe = &outcome.probe;
  let capture_context = &outcome.capture_context;
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
  })
  .to_string()
}

fn truncate_ocr_preview(text: &str) -> String {
  if text.chars().count() <= OCR_TEXT_PREVIEW_LIMIT {
    return text.to_string();
  }
  text.chars().take(OCR_TEXT_PREVIEW_LIMIT).collect::<String>() + "..."
}

fn label_matches_target(label: &str, target_identity: &str, _query_identity: &str) -> bool {
  normalize_identity(label) == target_identity
}

fn ocr_labels_containing_target(evidence_nodes: &[crate::ViewEvidenceNode], target_identity: &str, query_identity: &str) -> Vec<String> {
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
  scroll_context: SidebarTargetProbeScrollContext,
  previous_sidebar_crop: &mut Option<RgbaImage>,
) -> Result<SidebarTargetProbeOutcome, String> {
  let capture = session.window().capture(window).map_err(|error| format!("sidebar target probe capture failed: {error}"))?;
  let sidebar_ratio = crate::bounds_to_ratio(sidebar_bounds, &capture);
  let ocr_options = build_sidebar_target_probe_ocr_options(&inputs.ocr_options, target_label, query);
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
  let observation = crate::view_parsers::sidebar::parse::parse_sidebar_viewport(observation_index, parse_viewport, &recognition);
  let sidebar_crop = crate::crop_image(&capture.image, sidebar_bounds, capture.scale_factor);
  let scroll_motion = previous_sidebar_crop.as_ref().map(|previous| MotionDetectionPolicy::default().compare(previous, &sidebar_crop));
  *previous_sidebar_crop = Some(sidebar_crop.clone());

  let capture_context = build_probe_capture_context(
    ViewBounds::new(capture.bounds.origin.x, capture.bounds.origin.y, capture.bounds.size.width, capture.bounds.size.height),
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
  let probe = analyze_sidebar_target_probe(&observation, target_label, query);
  publish_sidebar_target_probe_artifacts(
    &capture.image,
    &sidebar_crop,
    &recognition,
    &observation,
    &probe,
    &scroll_context,
    &capture_context,
  );

  Ok(SidebarTargetProbeOutcome {
    probe,
    capture_context,
  })
}

#[cfg(test)]
#[path = "target_probe_test.rs"]
mod tests;
