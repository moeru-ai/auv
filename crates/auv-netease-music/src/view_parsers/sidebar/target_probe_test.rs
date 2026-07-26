use super::*;
use crate::recognition_test_data::fake_recognition;
use crate::view_parsers::sidebar::parse::parse_sidebar_viewport;

fn sample_sidebar_bounds() -> ViewBounds {
  ViewBounds::new(0.0, 469.8, 320.0, 338.2)
}

#[test]
fn build_sidebar_target_probe_ocr_options_includes_target_and_query_custom_words() {
  let options = build_sidebar_target_probe_ocr_options(&TextRecognitionOptions::default().with_custom_words(["绚香"]), "16", "16");

  assert_eq!(options.custom_words, vec!["绚香".to_string(), "16".to_string()]);
}

#[test]
fn build_sidebar_target_probe_ocr_options_preserves_cli_languages() {
  let base = TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
  let options = build_sidebar_target_probe_ocr_options(&base, "16", "16");

  assert_eq!(options.recognition_languages, Some(vec!["ja-JP".to_string()]));
}

#[test]
fn build_sidebar_target_probe_ocr_options_sets_default_languages_when_absent() {
  let options = build_sidebar_target_probe_ocr_options(&TextRecognitionOptions::default(), "16", "16");

  assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
}

#[test]
fn resolve_probe_ocr_profile_prefers_fallback_when_sidebar_empty() {
  assert_eq!(resolve_probe_ocr_profile_after_sidebar(0), PROBE_FULL_WINDOW_FALLBACK_V1);
  assert_eq!(resolve_probe_ocr_profile_after_sidebar(1), PROBE_SIDEBAR_ENHANCED_V1);
}

#[test]
fn probe_parse_viewport_bounds_extends_bottom_on_full_window_fallback() {
  let sidebar_bounds = sample_sidebar_bounds();
  let expanded = probe_parse_viewport_bounds(sidebar_bounds, PROBE_FULL_WINDOW_FALLBACK_V1);

  assert_eq!(expanded.x, sidebar_bounds.x);
  assert_eq!(expanded.y, sidebar_bounds.y);
  assert_eq!(expanded.width, sidebar_bounds.width);
  assert_eq!(expanded.height, sidebar_bounds.height + PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING);
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

  assert_eq!(expanded.height, sidebar_bounds.height + PROBE_FULL_WINDOW_VIEWPORT_BOTTOM_PADDING);
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
  let expanded = parse_sidebar_viewport(0, probe_parse_viewport_bounds(sidebar_bounds, PROBE_FULL_WINDOW_FALLBACK_V1), &recognition);

  assert_eq!(strict.evidence_nodes.len(), 1);
  assert_eq!(strict.evidence_nodes[0].label.as_deref(), Some("4"));
  assert!(expanded.evidence_nodes.len() >= 2);
  assert!(expanded.candidates.iter().any(|candidate| candidate.label.as_deref() == Some("收藏的歌单1へ")));
}

#[test]
fn probe_parse_viewport_keeps_player_bar_outside() {
  let recognition = fake_recognition(vec![("Reverberation", 98.0, 994.0, 160.0, 20.0)]);
  let sidebar_bounds = ViewBounds::new(0.0, 443.0, 344.0, 528.0);
  let observation = parse_sidebar_viewport(0, sidebar_bounds, &recognition);

  assert!(observation.evidence_nodes.is_empty());
}

#[test]
fn label_matches_target_requires_exact_identity() {
  assert!(label_matches_target("3", "3", "3"));
  assert!(!label_matches_target("43", "3", "3"));
  assert!(!label_matches_target("13", "3", "3"));
}
