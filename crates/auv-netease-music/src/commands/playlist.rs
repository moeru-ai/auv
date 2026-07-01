use std::fmt;

use auv_view::{ParserDiagnostic, ScanAppContext, ScanWindowContext, ViewBounds};
use serde::{Deserialize, Serialize};

use crate::{
  Inputs, PlaybackControlState, PlaylistSelectTarget, decode_playlist_sidebar_scan_json,
  run_live_scan_until_query,
};

const PLAYLIST_SELECT_BOTTOM_SAFE_PADDING: f64 = 128.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSelectResult {
  pub command: String,
  pub query: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub target: PlaylistSelectTarget,
  pub steps: Vec<PlaylistSelectStep>,
  pub verification: PlaylistSelectVerification,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reacquire: Option<crate::view_memory::PlaylistReacquireSummary>,
}

impl PlaylistSelectResult {
  pub fn to_human_readable(&self) -> PlaylistSelectHumanSummary<'_> {
    PlaylistSelectHumanSummary { result: self }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSelectStep {
  pub name: String,
  pub target_bounds: Option<ViewBounds>,
  pub delivery_path: Option<String>,
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSelectVerification {
  pub status: String,
  pub method: String,
  pub observed_title: Option<String>,
  pub artifact: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub recognition_artifact: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sidebar_echo_recognition_artifact: Option<String>,
  pub note: Option<String>,
}

const PLAYLIST_SELECT_VERIFICATION_OCR_TITLE_BAND: &str = "main_title_ocr_title_band_v1";
const PLAYLIST_SELECT_VERIFICATION_OCR_HERO_HEADER: &str = "main_title_ocr_hero_header_v1";
const PLAYLIST_SELECT_VERIFICATION_OCR_MAIN_BAND: &str = "main_title_ocr_main_band_v1";
const PLAYLIST_SELECT_VERIFICATION_OCR_FULL_WINDOW: &str = "main_title_ocr_full_window_v1";
const PLAYLIST_SELECT_VERIFICATION_METHOD_SIDEBAR_ECHO: &str = "sidebar_row_echo_detail_chrome_v1";
const PLAYLIST_SELECT_VERIFICATION_SIDEBAR_ECHO_LIMIT: &str =
  "verification_used_sidebar_row_echo_for_numeric_title";
const PLAYLIST_SELECT_VERIFICATION_ROW_ECHO_MARGIN: f64 = 16.0;
const PLAYLIST_SELECT_TARGET_FROM_SCAN_CACHE_MARKER: &str =
  "playlist_select_target_from_scan_cache_v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaylistSelectTargetResolveSource {
  ScanCache,
  LiveScan,
}

fn playlist_select_target_resolve_source(
  gate_enabled: bool,
  cache: Option<&crate::PlaylistSidebarScan>,
  query: &str,
) -> PlaylistSelectTargetResolveSource {
  if gate_enabled {
    if let Some(scan) = cache {
      if scan.select_target(query).is_ok() {
        return PlaylistSelectTargetResolveSource::ScanCache;
      }
    }
  }
  PlaylistSelectTargetResolveSource::LiveScan
}

fn try_load_playlist_scan_cache_optional(inputs: &Inputs) -> Option<crate::PlaylistSidebarScan> {
  let cache_path = inputs.artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE);
  let json = std::fs::read_to_string(&cache_path).ok()?;
  decode_playlist_sidebar_scan_json(&json).ok()
}

pub struct PlaylistSelectHumanSummary<'a> {
  result: &'a PlaylistSelectResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistPlayResult {
  pub command: String,
  pub query: String,
  pub select: PlaylistSelectResult,
  pub steps: Vec<PlaylistPlayStep>,
  pub verification: PlaylistPlayVerification,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
  pub artifacts: Vec<String>,
}

impl PlaylistPlayResult {
  pub fn to_human_readable(&self) -> PlaylistPlayHumanSummary<'_> {
    PlaylistPlayHumanSummary { result: self }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistPlayStep {
  pub name: String,
  pub target_label: Option<String>,
  pub target_bounds: Option<ViewBounds>,
  pub delivery_path: Option<String>,
  pub fallback_reason: Option<String>,
  pub artifact: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistPlayVerification {
  pub status: String,
  pub method: String,
  pub control_state: Option<PlaybackControlState>,
  pub observed_bottom_text: Option<String>,
  pub artifact: Option<String>,
  pub note: Option<String>,
}

pub struct PlaylistPlayHumanSummary<'a> {
  result: &'a PlaylistPlayResult,
}

impl fmt::Display for PlaylistSelectHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let result = self.result;
    writeln!(f, "NetEase playlist select")?;
    writeln!(f, "query: {}", result.query)?;
    writeln!(f, "target: {}", result.target.label)?;
    writeln!(
      f,
      "verification: {}{}",
      result.verification.status,
      result
        .verification
        .observed_title
        .as_deref()
        .map(|title| format!(" observed_title={title}"))
        .unwrap_or_default()
    )?;
    if result.known_limits.is_empty() {
      writeln!(f, "known_limits: (none)")?;
    } else {
      writeln!(f, "known_limits:")?;
      for limit in &result.known_limits {
        writeln!(f, "  - {limit}")?;
      }
    }
    if result.diagnostics.is_empty() {
      write!(f, "diagnostics: (none)")
    } else {
      writeln!(f, "diagnostics:")?;
      for diagnostic in &result.diagnostics {
        writeln!(f, "  - {}: {}", diagnostic.code, diagnostic.message)?;
      }
      Ok(())
    }
  }
}

impl fmt::Display for PlaylistPlayHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let result = self.result;
    writeln!(f, "NetEase playlist play")?;
    writeln!(f, "query: {}", result.query)?;
    writeln!(f, "target: {}", result.select.target.label)?;
    writeln!(
      f,
      "verification: {} control={}",
      result.verification.status,
      result
        .verification
        .control_state
        .map(|state| format!("{state:?}"))
        .unwrap_or_else(|| "-".to_string())
    )?;
    if result.known_limits.is_empty() {
      writeln!(f, "known_limits: (none)")?;
    } else {
      writeln!(f, "known_limits:")?;
      for limit in &result.known_limits {
        writeln!(f, "  - {limit}")?;
      }
    }
    if result.diagnostics.is_empty() {
      write!(f, "diagnostics: (none)")
    } else {
      writeln!(f, "diagnostics:")?;
      for diagnostic in &result.diagnostics {
        writeln!(f, "  - {}: {}", diagnostic.code, diagnostic.message)?;
      }
      Ok(())
    }
  }
}

fn playlist_select_click_options() -> auv_driver::ClickOptions {
  auv_driver::ClickOptions {
    policy: auv_driver::InputPolicy::BackgroundPreferred,
    click: auv_driver::Click::Single,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
  }
}

fn playlist_play_click_options() -> auv_driver::ClickOptions {
  auv_driver::ClickOptions {
    policy: auv_driver::InputPolicy::BackgroundPreferred,
    click: auv_driver::Click::Single,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
  }
}

fn playlist_play_status_from_bottom_probe(
  control_state: PlaybackControlState,
  before_bottom_text: Option<&str>,
  observed_bottom_text: Option<&str>,
) -> &'static str {
  if control_state != PlaybackControlState::PauseVisible {
    return "failed";
  }

  let before = before_bottom_text.and_then(normalized_non_empty);
  let observed = observed_bottom_text.and_then(normalized_non_empty);
  match (before, observed) {
    (Some(before), Some(observed)) if before == observed => "failed",
    (Some(_), None) => "failed",
    _ => "passed",
  }
}

fn normalized_non_empty(input: &str) -> Option<String> {
  let normalized = crate::normalize_identity(input);
  (!normalized.is_empty()).then_some(normalized)
}

fn playlist_select_bottom_padding_scroll_needed(
  target_bounds: ViewBounds,
  sidebar_bounds: ViewBounds,
) -> bool {
  target_bounds.y + target_bounds.height
    > sidebar_bounds.y + sidebar_bounds.height - PLAYLIST_SELECT_BOTTOM_SAFE_PADDING
}

fn playlist_select_verification_main_pane_guard(
  bounds: ViewBounds,
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> bool {
  let main_pane_min_x = sidebar_bounds.x + sidebar_bounds.width * 0.85;
  let nav_band_max_y = window_size.height * 0.12;
  let main_content_max_y = window_size.height * 0.55;
  bounds.x >= main_pane_min_x && bounds.y > nav_band_max_y && bounds.y < main_content_max_y
}

fn playlist_select_verification_region_matches_target(
  target_label: &str,
  region_text: &str,
) -> bool {
  use crate::views::query_match::{PlaylistLabelMatchTier, playlist_label_match_tier};

  let target_identity = crate::normalize_identity(target_label);
  let label_identity = crate::normalize_identity(region_text);
  match playlist_label_match_tier(&label_identity, &target_identity) {
    PlaylistLabelMatchTier::Exact => true,
    PlaylistLabelMatchTier::Contains => {
      !crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label)
    }
    PlaylistLabelMatchTier::None => false,
  }
}

fn playlist_select_verification_title(
  recognition: &auv_driver::vision::TextRecognition,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
  target_label: &str,
) -> Option<String> {
  recognition
    .regions
    .iter()
    .filter(|region| {
      playlist_select_verification_main_pane_guard(
        ViewBounds::new(
          region.bounds.origin.x,
          region.bounds.origin.y,
          region.bounds.size.width,
          region.bounds.size.height,
        ),
        sidebar_bounds,
        window_size,
      )
    })
    .filter(|region| playlist_select_verification_region_matches_target(target_label, &region.text))
    .min_by(|left, right| {
      left
        .bounds
        .origin
        .y
        .partial_cmp(&right.bounds.origin.y)
        .unwrap_or(std::cmp::Ordering::Equal)
    })
    .map(|region| region.text.trim().to_string())
}

fn build_playlist_select_verification_ocr_options(
  inputs: &Inputs,
  target_label: &str,
) -> auv_driver::vision::TextRecognitionOptions {
  if crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label) {
    crate::view_parsers::sidebar::target_probe::build_sidebar_target_probe_ocr_options(
      &inputs.ocr_options,
      target_label,
      target_label,
    )
  } else {
    inputs.ocr_options.clone()
  }
}

fn playlist_select_verification_horizontal_ratio(
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> (f64, f64) {
  let window_width = window_size.width.max(1.0);
  let x_start = ((sidebar_bounds.x + sidebar_bounds.width) / window_width).clamp(0.24, 0.45);
  let width = (1.0 - x_start - 0.02).clamp(0.40, 0.76);
  (x_start, width)
}

fn playlist_select_verification_title_band_ratio(
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  let (x_start, width) = playlist_select_verification_horizontal_ratio(sidebar_bounds, window_size);
  // NOTICE(a6c-11): narrow band aligned with main_pane_guard nav floor (12% height).
  auv_driver::RatioRect::new(x_start, 0.12, width, 0.22)
}

fn playlist_select_verification_hero_header_ratio(
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  let (x_start, width) = playlist_select_verification_horizontal_ratio(sidebar_bounds, window_size);
  // NOTICE(a6c-12): hero header above metadata line (1812 live y≈139 on 890px window).
  auv_driver::RatioRect::new(x_start, 0.08, width, 0.10)
}

fn playlist_select_verification_main_band_ratio(
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  let (x_start, width) = playlist_select_verification_horizontal_ratio(sidebar_bounds, window_size);
  auv_driver::RatioRect::new(x_start, 0.10, width, 0.45)
}

fn playlist_select_verification_full_window_ratio(
  _sidebar_bounds: ViewBounds,
  _window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0)
}

fn playlist_select_verification_recognition_artifact_path(
  verification_png: &std::path::Path,
) -> std::path::PathBuf {
  let stem = verification_png
    .file_stem()
    .map(|stem| stem.to_string_lossy().into_owned())
    .unwrap_or_else(|| "playlist-select-post-click".to_string());
  verification_png.with_file_name(format!("{stem}-recognition.json"))
}

fn playlist_select_verification_sidebar_echo_recognition_artifact_path(
  verification_png: &std::path::Path,
) -> std::path::PathBuf {
  let stem = verification_png
    .file_stem()
    .map(|stem| stem.to_string_lossy().into_owned())
    .unwrap_or_else(|| "playlist-select-post-click".to_string());
  verification_png.with_file_name(format!("{stem}-sidebar-echo-recognition.json"))
}

fn playlist_select_verification_view_bounds_to_ratio(
  bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  let window_width = window_size.width.max(1.0);
  let window_height = window_size.height.max(1.0);
  auv_driver::RatioRect::new(
    bounds.x / window_width,
    bounds.y / window_height,
    bounds.width / window_width,
    bounds.height / window_height,
  )
}

fn playlist_select_verification_region_overlaps_row_bounds(
  region_bounds: ViewBounds,
  row_bounds: ViewBounds,
  margin: f64,
) -> bool {
  let expanded = ViewBounds::new(
    row_bounds.x - margin,
    row_bounds.y - margin,
    row_bounds.width + margin * 2.0,
    row_bounds.height + margin * 2.0,
  );
  region_bounds.x < expanded.x + expanded.width
    && region_bounds.x + region_bounds.width > expanded.x
    && region_bounds.y < expanded.y + expanded.height
    && region_bounds.y + region_bounds.height > expanded.y
}

fn playlist_select_verification_detail_chrome_present(
  recognition: &auv_driver::vision::TextRecognition,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
) -> bool {
  let play_all = crate::normalize_identity("播放全部");
  let song = crate::normalize_identity("歌曲");
  let comment = crate::normalize_identity("评论");
  let mut has_song = false;
  let mut has_comment = false;
  let mut has_play_all = false;

  for region in &recognition.regions {
    let region_bounds = ViewBounds::new(
      region.bounds.origin.x,
      region.bounds.origin.y,
      region.bounds.size.width,
      region.bounds.size.height,
    );
    if !playlist_select_verification_main_pane_guard(region_bounds, sidebar_bounds, window_size) {
      continue;
    }
    let normalized = crate::normalize_identity(&region.text);
    if normalized.contains(&play_all) {
      has_play_all = true;
    }
    if normalized.contains(&song) {
      has_song = true;
    }
    if normalized.contains(&comment) {
      has_comment = true;
    }
  }

  has_play_all || (has_song && has_comment)
}

fn playlist_select_verification_count_main_pane_guard_regions(
  recognition: &auv_driver::vision::TextRecognition,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
) -> usize {
  recognition
    .regions
    .iter()
    .filter(|region| {
      playlist_select_verification_main_pane_guard(
        ViewBounds::new(
          region.bounds.origin.x,
          region.bounds.origin.y,
          region.bounds.size.width,
          region.bounds.size.height,
        ),
        sidebar_bounds,
        window_size,
      )
    })
    .count()
}

fn playlist_select_verification_sidebar_row_echo_from_recognition(
  sidebar_recognition: &auv_driver::vision::TextRecognition,
  main_recognition: &auv_driver::vision::TextRecognition,
  row_bounds: ViewBounds,
  target_label: &str,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
) -> Option<String> {
  if !crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label) {
    return None;
  }
  if !playlist_select_verification_detail_chrome_present(
    main_recognition,
    window_size,
    sidebar_bounds,
  ) {
    return None;
  }

  sidebar_recognition
    .regions
    .iter()
    .filter(|region| {
      playlist_select_verification_region_overlaps_row_bounds(
        ViewBounds::new(
          region.bounds.origin.x,
          region.bounds.origin.y,
          region.bounds.size.width,
          region.bounds.size.height,
        ),
        row_bounds,
        PLAYLIST_SELECT_VERIFICATION_ROW_ECHO_MARGIN,
      )
    })
    .find(|region| playlist_select_verification_region_matches_target(target_label, &region.text))
    .map(|region| region.text.trim().to_string())
}

fn playlist_select_verification_note(
  ocr_tiers_tried: &str,
  final_tier: &str,
  region_count: usize,
  main_pane_guard_pass: usize,
  sidebar_echo_attempted: bool,
  sidebar_echo_pass: bool,
) -> String {
  format!(
    "ocr_tiers_tried={ocr_tiers_tried}; final_tier={final_tier}; region_count={region_count}; main_pane_guard_pass={main_pane_guard_pass}; sidebar_echo_attempted={sidebar_echo_attempted}; sidebar_echo_pass={sidebar_echo_pass}"
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn playlist_select_uses_background_preferred_window_click_by_default() {
    let options = playlist_select_click_options();

    assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundPreferred);
    assert_eq!(
      options.window_strategy,
      auv_driver::WindowClickStrategy::ChromiumCompatible
    );
  }

  #[test]
  fn playlist_play_uses_background_preferred_window_click_by_default() {
    let options = playlist_play_click_options();

    assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundPreferred);
    assert_eq!(
      options.window_strategy,
      auv_driver::WindowClickStrategy::ChromiumCompatible
    );
  }

  #[test]
  fn playlist_select_requests_bottom_padding_scroll_for_targets_inside_bottom_safe_band() {
    let sidebar = ViewBounds::new(0.0, 0.0, 320.0, 860.0);
    let unsafe_target = ViewBounds::new(72.0, 800.0, 154.0, 14.0);
    let safe_target = ViewBounds::new(72.0, 620.0, 154.0, 14.0);

    assert!(playlist_select_bottom_padding_scroll_needed(
      unsafe_target,
      sidebar
    ));
    assert!(!playlist_select_bottom_padding_scroll_needed(
      safe_target,
      sidebar
    ));
  }

  #[test]
  fn playlist_play_verification_rejects_unchanged_existing_playback() {
    let status = playlist_play_status_from_bottom_probe(
      PlaybackControlState::PauseVisible,
      Some("old song"),
      Some("old song"),
    );

    assert_eq!(status, "failed");
  }

  fn sample_playlist_select_window() -> auv_driver::Size {
    auv_driver::Size::new(1057.0, 752.0)
  }

  fn sample_playlist_select_sidebar() -> ViewBounds {
    ViewBounds::new(0.0, 220.0, 346.0, 720.0)
  }

  #[test]
  fn playlist_select_verification_title_matches_in_main_pane() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![
      ("曲。播客。有声书。歌单。专辑。", 380.0, 48.0, 280.0, 18.0),
      ("最近播放", 420.0, 198.0, 120.0, 28.0),
    ]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "最近播放",
    );

    assert_eq!(title.as_deref(), Some("最近播放"));
  }

  #[test]
  fn playlist_select_verification_title_rejects_top_nav_band() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![(
      "曲。播客。有声书。歌单。专辑。",
      380.0,
      48.0,
      280.0,
      18.0,
    )]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "最近播放",
    );

    assert!(title.is_none());
  }

  #[test]
  fn playlist_select_verification_title_prefers_label_over_unrelated() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![
      ("其他推荐歌单", 420.0, 240.0, 140.0, 24.0),
      ("最近播放", 420.0, 198.0, 120.0, 28.0),
    ]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "最近播放",
    );

    assert_eq!(title.as_deref(), Some("最近播放"));
  }

  #[test]
  fn playlist_select_verification_title_matches_single_digit_exact() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![
      ("曲。播客。有声书。歌单。专辑。", 380.0, 48.0, 280.0, 18.0),
      ("3", 420.0, 198.0, 12.0, 28.0),
    ]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "3",
    );

    assert_eq!(title.as_deref(), Some("3"));
  }

  #[test]
  fn playlist_select_verification_title_rejects_only_contains_digit_collision() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![("43", 420.0, 198.0, 24.0, 28.0)]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "3",
    );

    assert!(title.is_none());
  }

  #[test]
  fn playlist_select_verification_title_prefers_exact_digit_when_collision_present() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![
      ("43", 420.0, 198.0, 24.0, 28.0),
      ("3", 420.0, 240.0, 12.0, 28.0),
    ]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "3",
    );

    assert_eq!(title.as_deref(), Some("3"));
  }

  #[test]
  fn playlist_select_verification_title_rejects_nav_band_single_digit() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![("3", 380.0, 48.0, 12.0, 18.0)]);

    let title = playlist_select_verification_title(
      &recognition,
      sample_playlist_select_window(),
      sample_playlist_select_sidebar(),
      "3",
    );

    assert!(title.is_none());
  }

  #[test]
  fn build_playlist_select_verification_ocr_options_boosts_single_digit_target() {
    let inputs = Inputs {
      artifact_dir: std::path::PathBuf::from("/tmp/artifacts"),
      ..Inputs::with_defaults()
    };
    let options = build_playlist_select_verification_ocr_options(&inputs, "3");

    assert!(options.custom_words.iter().any(|word| word == "3"));
    assert_eq!(
      options.recognition_languages,
      Some(vec!["zh-Hans".to_string(), "en-US".to_string()])
    );
  }

  #[test]
  fn build_playlist_select_verification_ocr_options_leaves_non_digit_unchanged() {
    let base =
      auv_driver::vision::TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
    let inputs = Inputs {
      ocr_options: base.clone(),
      artifact_dir: std::path::PathBuf::from("/tmp/artifacts"),
      ..Inputs::with_defaults()
    };
    let options = build_playlist_select_verification_ocr_options(&inputs, "最近播放");

    assert_eq!(options, base);
  }

  #[test]
  fn playlist_select_verification_recognition_artifact_path_uses_png_stem() {
    let path = playlist_select_verification_recognition_artifact_path(std::path::Path::new(
      "/tmp/artifacts/playlist-select-post-click.png",
    ));

    assert_eq!(
      path,
      std::path::PathBuf::from("/tmp/artifacts/playlist-select-post-click-recognition.json")
    );
  }

  #[test]
  fn playlist_select_verification_sidebar_echo_artifact_path_uses_png_stem() {
    let path = playlist_select_verification_sidebar_echo_recognition_artifact_path(
      std::path::Path::new("/tmp/artifacts/playlist-select-post-click.png"),
    );

    assert_eq!(
      path,
      std::path::PathBuf::from(
        "/tmp/artifacts/playlist-select-post-click-sidebar-echo-recognition.json"
      )
    );
  }

  fn sample_playlist_select_window_1812() -> auv_driver::Size {
    auv_driver::Size::new(1512.0, 890.0)
  }

  fn sample_playlist_select_sidebar_1812() -> ViewBounds {
    ViewBounds::new(0.0, 267.0, 362.88, 541.0)
  }

  #[test]
  fn playlist_select_verification_hero_header_ratio_covers_metadata_line() {
    let window = sample_playlist_select_window_1812();
    let sidebar = sample_playlist_select_sidebar_1812();
    let ratio = playlist_select_verification_hero_header_ratio(sidebar, window);

    let y_start = ratio.y * window.height;
    let y_end = y_start + ratio.height * window.height;
    assert!(y_start < 139.0);
    assert!(y_end > 139.0);
    assert!(y_end <= 165.0);
  }

  #[test]
  fn playlist_select_verification_detail_chrome_passes_with_play_all() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![("▶ 播放全部", 446.0, 233.0, 73.0, 19.0)]);

    assert!(playlist_select_verification_detail_chrome_present(
      &recognition,
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    ));
  }

  #[test]
  fn playlist_select_verification_detail_chrome_passes_with_song_and_comment() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![
      ("歌曲", 400.0, 290.0, 35.0, 18.0),
      ("评论 收藏者", 460.0, 288.0, 108.0, 21.0),
    ]);

    assert!(playlist_select_verification_detail_chrome_present(
      &recognition,
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    ));
  }

  #[test]
  fn playlist_select_verification_detail_chrome_fails_with_song_only() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let recognition = fake_recognition(vec![("歌曲", 242.0, 290.0, 35.0, 18.0)]);

    assert!(!playlist_select_verification_detail_chrome_present(
      &recognition,
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    ));
  }

  #[test]
  fn playlist_select_verification_sidebar_row_echo_passes_with_play_all_chrome() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let sidebar = fake_recognition(vec![("3", 70.0, 657.0, 10.0, 13.0)]);
    let main = fake_recognition(vec![("▶ 播放全部", 446.0, 233.0, 73.0, 19.0)]);
    let row_bounds = ViewBounds::new(70.0, 657.0, 10.0, 13.0);

    let title = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar,
      &main,
      row_bounds,
      "3",
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    );

    assert_eq!(title.as_deref(), Some("3"));
  }

  #[test]
  fn playlist_select_verification_sidebar_row_echo_passes_with_song_comment_chrome() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let sidebar = fake_recognition(vec![("3", 70.0, 657.0, 10.0, 13.0)]);
    let main = fake_recognition(vec![
      ("歌曲", 400.0, 290.0, 35.0, 18.0),
      ("评论 收藏者", 460.0, 288.0, 108.0, 21.0),
    ]);
    let row_bounds = ViewBounds::new(70.0, 657.0, 10.0, 13.0);

    let title = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar,
      &main,
      row_bounds,
      "3",
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    );

    assert_eq!(title.as_deref(), Some("3"));
  }

  #[test]
  fn playlist_select_verification_sidebar_row_echo_fails_with_song_only() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let sidebar = fake_recognition(vec![("3", 70.0, 657.0, 10.0, 13.0)]);
    let main = fake_recognition(vec![("歌曲", 242.0, 290.0, 35.0, 18.0)]);
    let row_bounds = ViewBounds::new(70.0, 657.0, 10.0, 13.0);

    let title = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar,
      &main,
      row_bounds,
      "3",
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    );

    assert!(title.is_none());
  }

  #[test]
  fn playlist_select_verification_sidebar_row_echo_uses_row_bounds_not_stale_target() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let sidebar = fake_recognition(vec![("3", 70.0, 500.0, 10.0, 13.0)]);
    let main = fake_recognition(vec![("▶ 播放全部", 446.0, 233.0, 73.0, 19.0)]);
    let click_bounds = ViewBounds::new(70.0, 657.0, 10.0, 13.0);

    let title = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar,
      &main,
      click_bounds,
      "3",
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    );

    assert!(title.is_none());
  }

  #[test]
  fn playlist_select_verification_sidebar_row_echo_skipped_for_cjk() {
    use crate::view_parsers::sidebar::test_support::fake_recognition;

    let sidebar = fake_recognition(vec![("最近播放", 70.0, 657.0, 80.0, 13.0)]);
    let main = fake_recognition(vec![("▶ 播放全部", 446.0, 233.0, 73.0, 19.0)]);
    let row_bounds = ViewBounds::new(70.0, 657.0, 80.0, 13.0);

    let title = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar,
      &main,
      row_bounds,
      "最近播放",
      sample_playlist_select_window_1812(),
      sample_playlist_select_sidebar_1812(),
    );

    assert!(title.is_none());
  }

  #[test]
  fn playlist_select_verification_note_includes_tiers_and_echo_flags() {
    let note = playlist_select_verification_note(
      "title,hero,main,full",
      PLAYLIST_SELECT_VERIFICATION_METHOD_SIDEBAR_ECHO,
      13,
      4,
      true,
      true,
    );

    assert!(note.contains("ocr_tiers_tried=title,hero,main,full"));
    assert!(note.contains("final_tier=sidebar_row_echo_detail_chrome_v1"));
    assert!(note.contains("region_count=13"));
    assert!(note.contains("main_pane_guard_pass=4"));
    assert!(note.contains("sidebar_echo_attempted=true"));
    assert!(note.contains("sidebar_echo_pass=true"));
  }
  fn scan_with_playlist_labels(labels: &[&str]) -> crate::PlaylistSidebarScan {
    use crate::view_parsers::sidebar::parse_sidebar_viewport;
    use crate::view_parsers::sidebar::reconstruct::reconstruct_playlist_sidebar;
    use crate::view_parsers::sidebar::test_support::fake_recognition;
    use crate::{ScanAppContext, ScanWindowContext, ViewRegionRecord};

    let mut rows = vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)];
    for (index, label) in labels.iter().enumerate() {
      let y = 74.0 + index as f64 * 30.0;
      rows.push((*label, 32.0, y, 120.0, 20.0));
    }
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(rows),
    );

    reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![page0],
    )
  }

  #[test]
  fn playlist_select_target_resolve_source_uses_cache_when_gate_and_unique_match() {
    let scan = scan_with_playlist_labels(&["43", "3"]);
    assert_eq!(
      playlist_select_target_resolve_source(true, Some(&scan), "3"),
      PlaylistSelectTargetResolveSource::ScanCache
    );
  }

  #[test]
  fn playlist_select_target_resolve_source_live_scan_when_gate_off() {
    let scan = scan_with_playlist_labels(&["3"]);
    assert_eq!(
      playlist_select_target_resolve_source(false, Some(&scan), "3"),
      PlaylistSelectTargetResolveSource::LiveScan
    );
  }

  #[test]
  fn playlist_select_target_resolve_source_live_scan_when_cache_missing() {
    assert_eq!(
      playlist_select_target_resolve_source(true, None, "3"),
      PlaylistSelectTargetResolveSource::LiveScan
    );
  }

  #[test]
  fn playlist_select_target_resolve_source_live_scan_when_select_target_ambiguous() {
    let scan = scan_with_playlist_labels(&["43", "13"]);
    assert_eq!(
      playlist_select_target_resolve_source(true, Some(&scan), "3"),
      PlaylistSelectTargetResolveSource::LiveScan
    );
  }

  #[test]
  fn playlist_select_target_resolve_source_live_scan_when_select_target_miss() {
    let scan = scan_with_playlist_labels(&["43", "13"]);
    assert_eq!(
      playlist_select_target_resolve_source(true, Some(&scan), "99"),
      PlaylistSelectTargetResolveSource::LiveScan
    );
  }

  #[test]
  fn playlist_select_target_resolve_source_ignores_memory_presence() {
    // `playlist_select_target_resolve_source` only reads gate + scan cache + query;
    // view-memory file presence is not an input to this resolver.
    let scan = scan_with_playlist_labels(&["3"]);
    assert_eq!(
      playlist_select_target_resolve_source(true, Some(&scan), "3"),
      PlaylistSelectTargetResolveSource::ScanCache
    );
  }
}

#[cfg(not(target_os = "macos"))]
pub fn run_playlist_select(_inputs: &Inputs, _query: &str) -> Result<PlaylistSelectResult, String> {
  Err("live NetEase playlist select is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_playlist_play(_inputs: &Inputs, _query: &str) -> Result<PlaylistPlayResult, String> {
  Err("live NetEase playlist play is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_playlist_play_candidate_id(
  _inputs: &Inputs,
  _candidate_id: &str,
) -> Result<PlaylistPlayResult, String> {
  Err("live NetEase playlist play is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_playlist_select(inputs: &Inputs, query: &str) -> Result<PlaylistSelectResult, String> {
  let (scan, target, target_from_scan_cache) = resolve_playlist_target_for_query(inputs, query)?;
  run_playlist_select_resolved(inputs, query, scan, target, target_from_scan_cache)
}

#[cfg(target_os = "macos")]
fn resolve_playlist_target_for_query(
  inputs: &Inputs,
  query: &str,
) -> Result<(crate::PlaylistSidebarScan, PlaylistSelectTarget, bool), String> {
  std::fs::create_dir_all(&inputs.artifact_dir).map_err(|error| {
    format!(
      "failed to create {}: {error}",
      inputs.artifact_dir.display()
    )
  })?;

  let gate_enabled = crate::view_memory::enabled();
  let cache = try_load_playlist_scan_cache_optional(inputs);
  match playlist_select_target_resolve_source(gate_enabled, cache.as_ref(), query) {
    PlaylistSelectTargetResolveSource::ScanCache => {
      let scan = cache.expect("ScanCache path requires loaded cache");
      let target = scan.select_target(query)?;
      Ok((scan, target, true))
    }
    PlaylistSelectTargetResolveSource::LiveScan => {
      let scan = run_live_scan_until_query(inputs, query)?;
      let target = scan.select_target(query)?;
      Ok((scan, target, false))
    }
  }
}

#[cfg(target_os = "macos")]
fn run_playlist_select_resolved(
  inputs: &Inputs,
  query: &str,
  scan: crate::PlaylistSidebarScan,
  target: PlaylistSelectTarget,
  target_from_scan_cache: bool,
) -> Result<PlaylistSelectResult, String> {
  use crate::LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER;
  use crate::delivery_path_label;
  use crate::view_parsers::sidebar::region::{broad_sidebar_probe_bounds, sidebar_scroll_anchor};
  use crate::view_parsers::sidebar::{
    SidebarTargetProbeScrollContext, SidebarTargetSeekStep, capture_sidebar_target_probe,
    next_sidebar_target_seek_step, preceding_scroll_context, sidebar_rescan_target_seek_budget,
    sidebar_target_probe_diagnostic_message, top_seek_scroll_budget,
  };
  use auv_driver::selector::{App, Window};
  use auv_driver::{
    ActivationPolicy, Click, Driver, InputPolicy, PrepareForInputOptions, Scroll, ScrollOptions,
    Size, WindowPoint,
  };
  use auv_driver_macos::MacosDriver;

  let target_bounds = target.bounds.ok_or_else(|| {
    format!(
      "playlist target {:?} did not carry live bounds; rerun playlist ls/select",
      target.label
    )
  })?;
  let target_observation_index = target.observation_index.unwrap_or(0);

  let driver = MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(app))
    .map_err(|error| format!("failed to resolve NetEase window: {error}"))?;
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let sidebar_bounds = scan
    .sidebar_region()
    .bounds
    .unwrap_or_else(|| broad_sidebar_probe_bounds(window_size));
  let sidebar_baseline_width = Some(sidebar_bounds.width.round().max(1.0) as u32);
  let sidebar_anchor = sidebar_scroll_anchor(sidebar_bounds);
  let mut steps = Vec::new();
  let mut diagnostics = scan.diagnostics().to_vec();
  let mut known_limits = scan.known_limits().to_vec();
  if target_from_scan_cache {
    known_limits.insert(0, PLAYLIST_SELECT_TARGET_FROM_SCAN_CACHE_MARKER.to_string());
  }
  let mut reacquire_summary = None;
  let mut skip_rescan_replay = false;
  let mut click_bounds = target_bounds;

  if crate::view_memory::enabled() {
    if let Some(memory) = crate::view_memory::load_memory_raw(inputs) {
      let reacquire_bounds = memory
        .scope_snapshot
        .region_bounds_window_local
        .width
        .is_finite()
        .then_some(memory.scope_snapshot.region_bounds_window_local)
        .unwrap_or(sidebar_bounds);
      let reacquire_anchor = sidebar_scroll_anchor(reacquire_bounds);
      let read_config = auv_view::memory::MemoryReadConfig {
        now_millis: crate::view_memory::system_time_millis(),
        ..Default::default()
      };
      match crate::view_parsers::sidebar::reacquire::try_reacquire_for_target(
        inputs,
        &session,
        &window,
        reacquire_bounds,
        reacquire_anchor,
        &memory,
        &target,
        &read_config,
        sidebar_baseline_width,
      ) {
        crate::view_memory::PlaylistReacquireAttempt::Hit { bounds, summary } => {
          click_bounds = bounds;
          skip_rescan_replay = true;
          reacquire_summary = Some(summary);
          steps.push(PlaylistSelectStep {
            name: "reacquire-target".to_string(),
            target_bounds: Some(click_bounds),
            delivery_path: None,
            fallback_reason: None,
          });
        }
        crate::view_memory::PlaylistReacquireAttempt::Stale { summary } => {
          let stale_reason = summary.stale_reason.as_deref().unwrap_or("unknown");
          known_limits.push(format!(
            "view-memory stale at reacquire ({stale_reason}) — falling back to rescan replay",
          ));
          reacquire_summary = Some(summary);
        }
        crate::view_memory::PlaylistReacquireAttempt::Miss { summary } => {
          known_limits.push(
            "view-memory reacquire missed target — falling back to rescan replay".to_string(),
          );
          reacquire_summary = Some(summary);
        }
      }
    } else {
      known_limits.push("view-memory file missing or unreadable — using rescan replay".to_string());
    }
  }

  if !skip_rescan_replay {
    // NOTICE(netease-playlist-select-reacquire): rescan replay path when view-memory
    // is disabled, not loaded, or reacquire misses. Rewinds to top then scroll-seeks
    // the target label instead of replaying a stale observation_index page count.
    // NOTICE(a6c-5): top rewind step size matches live top seek; motion stop deferred.
    let top_scroll_delta = inputs.scroll_amount * LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER;
    let top_scrolls = top_seek_scroll_budget(inputs.max_scrolls);
    let mut last_scroll_context = None;
    for index in 0..top_scrolls {
      match session.window().scroll(
        &window,
        sidebar_anchor,
        Scroll::new(0.0, top_scroll_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      ) {
        Ok(result) => {
          last_scroll_context = Some(preceding_scroll_context(
            format!("scroll-sidebar-top-{index}"),
            top_scroll_delta,
            "background_preferred",
            inputs.scroll_settle_ms,
            Some(delivery_path_label(result.selected_path).to_string()),
            result.fallback_reason.clone(),
          ));
          steps.push(PlaylistSelectStep {
            name: format!("scroll-sidebar-top-{index}"),
            target_bounds: Some(sidebar_bounds),
            delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
            fallback_reason: result.fallback_reason,
          });
        }
        Err(error) => {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_top_scroll_failed".to_string(),
            message: error.to_string(),
            node_id: target.candidate_id.clone(),
          });
          known_limits.push("playlist select top seek stopped after scroll failure".to_string());
          break;
        }
      }
    }

    let seek_budget =
      sidebar_rescan_target_seek_budget(inputs.max_scrolls, target_observation_index);
    let mut rescan_target_found = false;
    let mut last_rescan_probe_summary = None;
    let mut previous_sidebar_crop = None;
    for index in 0..seek_budget {
      let scroll_context = SidebarTargetProbeScrollContext {
        phase: "rescan".to_string(),
        attempt: index,
        scroll_anchor: (sidebar_anchor.0.x, sidebar_anchor.0.y),
        preceding_scroll: last_scroll_context.clone(),
      };
      let outcome = match capture_sidebar_target_probe(
        &session,
        &window,
        sidebar_bounds,
        inputs,
        index,
        &target.label,
        query,
        &inputs.artifact_dir,
        &format!("rescan-reobserve-{index:02}"),
        scroll_context,
        &mut previous_sidebar_crop,
      ) {
        Ok(outcome) => outcome,
        Err(error) => {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_rescan_reobserve_failed".to_string(),
            message: error,
            node_id: target.candidate_id.clone(),
          });
          known_limits.push(
            "playlist select rescan replay could not reobserve target before click".to_string(),
          );
          break;
        }
      };
      diagnostics.push(ParserDiagnostic {
        code: "playlist_select_rescan_reobserve_probe".to_string(),
        message: sidebar_target_probe_diagnostic_message("rescan", index, &outcome),
        node_id: target.candidate_id.clone(),
      });
      last_rescan_probe_summary = Some(sidebar_target_probe_diagnostic_message(
        "rescan", index, &outcome,
      ));
      let found = outcome.probe.result.is_some();
      match next_sidebar_target_seek_step(index, seek_budget, found) {
        Some(SidebarTargetSeekStep::Found(_)) => {
          click_bounds = outcome.probe.result.expect("found step requires bounds");
          steps.push(PlaylistSelectStep {
            name: "reobserve-playlist-after-rescan-replay".to_string(),
            target_bounds: Some(click_bounds),
            delivery_path: None,
            fallback_reason: None,
          });
          rescan_target_found = true;
          break;
        }
        Some(SidebarTargetSeekStep::ScrollNext(_)) => {
          let result = session
            .window()
            .scroll(
              &window,
              sidebar_anchor,
              Scroll::new(0.0, -inputs.scroll_amount),
              ScrollOptions {
                policy: InputPolicy::ForegroundPreferred,
                settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
                ..ScrollOptions::default()
              },
            )
            .map_err(|error| format!("playlist select page scroll failed: {error}"))?;
          last_scroll_context = Some(preceding_scroll_context(
            format!("scroll-sidebar-target-page-{index}"),
            -inputs.scroll_amount,
            "foreground_preferred",
            inputs.scroll_settle_ms,
            Some(delivery_path_label(result.selected_path).to_string()),
            result.fallback_reason.clone(),
          ));
          steps.push(PlaylistSelectStep {
            name: format!("scroll-sidebar-target-page-{index}"),
            target_bounds: Some(sidebar_bounds),
            delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
            fallback_reason: result.fallback_reason,
          });
        }
        None => break,
      }
    }

    if !rescan_target_found {
      diagnostics.push(ParserDiagnostic {
        code: "playlist_select_rescan_reobserve_missed_target".to_string(),
        message: format!(
          "target {:?} was not visible after rescan replay; last_probe={}",
          target.label,
          last_rescan_probe_summary.unwrap_or_else(|| "none".to_string())
        ),
        node_id: target.candidate_id.clone(),
      });
      known_limits
        .push("playlist select rescan replay could not reobserve target before click".to_string());
    }
  }

  for attempt in 0..2 {
    if !playlist_select_bottom_padding_scroll_needed(click_bounds, sidebar_bounds) {
      break;
    }

    let result = session
      .window()
      .scroll(
        &window,
        sidebar_anchor,
        Scroll::new(0.0, -inputs.scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| format!("playlist select bottom padding scroll failed: {error}"))?;
    let bottom_padding_scroll = preceding_scroll_context(
      format!("scroll-sidebar-bottom-padding-{attempt}"),
      -inputs.scroll_amount,
      "background_preferred",
      inputs.scroll_settle_ms,
      Some(delivery_path_label(result.selected_path).to_string()),
      result.fallback_reason.clone(),
    );
    steps.push(PlaylistSelectStep {
      name: format!("scroll-sidebar-bottom-padding-{attempt}"),
      target_bounds: Some(sidebar_bounds),
      delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
      fallback_reason: result.fallback_reason,
    });

    let mut previous_sidebar_crop = None;
    match capture_sidebar_target_probe(
      &session,
      &window,
      sidebar_bounds,
      inputs,
      attempt,
      &target.label,
      query,
      &inputs.artifact_dir,
      &format!("bottom-padding-reobserve-{attempt:02}"),
      SidebarTargetProbeScrollContext {
        phase: "bottom_padding".to_string(),
        attempt,
        scroll_anchor: (sidebar_anchor.0.x, sidebar_anchor.0.y),
        preceding_scroll: Some(bottom_padding_scroll),
      },
      &mut previous_sidebar_crop,
    ) {
      Ok(outcome) => {
        diagnostics.push(ParserDiagnostic {
          code: "playlist_select_bottom_padding_reobserve_probe".to_string(),
          message: sidebar_target_probe_diagnostic_message("bottom_padding", attempt, &outcome),
          node_id: target.candidate_id.clone(),
        });
        if let Some(bounds) = outcome.probe.result {
          click_bounds = bounds;
          steps.push(PlaylistSelectStep {
            name: format!("reobserve-playlist-after-bottom-padding-{attempt}"),
            target_bounds: Some(click_bounds),
            delivery_path: None,
            fallback_reason: None,
          });
        } else {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_bottom_padding_reobserve_missed_target".to_string(),
            message: format!(
              "target {:?} was not visible after bottom padding scroll; probe={}",
              target.label,
              sidebar_target_probe_diagnostic_message("bottom_padding", attempt, &outcome)
            ),
            node_id: target.candidate_id.clone(),
          });
          known_limits.push(
            "playlist select bottom padding could not reacquire target before click".to_string(),
          );
          break;
        }
      }
      Err(error) => {
        diagnostics.push(ParserDiagnostic {
          code: "playlist_select_bottom_padding_reobserve_failed".to_string(),
          message: error,
          node_id: target.candidate_id.clone(),
        });
        known_limits.push(
          "playlist select bottom padding could not reacquire target before click".to_string(),
        );
        break;
      }
    }
  }

  let click_point = WindowPoint::new(
    click_bounds.x + click_bounds.width * 0.5,
    click_bounds.y + click_bounds.height * 0.5,
  );
  let click = session
    .window()
    .click(&window, click_point, playlist_select_click_options())
    .map_err(|error| format!("playlist select click failed: {error}"))?;
  if inputs.scroll_settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
  }
  steps.push(PlaylistSelectStep {
    name: "click-playlist".to_string(),
    target_bounds: Some(click_bounds),
    delivery_path: Some(delivery_path_label(click.selected_path).to_string()),
    fallback_reason: click.fallback_reason,
  });

  let verification_artifact = inputs.artifact_dir.join("playlist-select-post-click.png");
  let mut verification = verify_playlist_select_title(
    &session,
    &window,
    window_size,
    sidebar_bounds,
    click_bounds,
    inputs,
    &verification_artifact,
    &target.label,
  )?;

  if verification.status != "passed" {
    known_limits.push(
      "background playlist row click did not verify; retried with foreground click".to_string(),
    );
    let screen_point = session
      .window()
      .to_screen_point(&window, click_point)
      .map_err(|error| format!("playlist select foreground point projection failed: {error}"))?;
    let lease = session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("playlist select foreground preparation failed: {error}"))?;
    let click_result = session
      .input()
      .click_at(screen_point.point(), Click::Single);
    let restore_result = session.window().restore_input(lease);
    click_result.map_err(|error| format!("playlist select foreground click failed: {error}"))?;
    restore_result
      .map_err(|error| format!("playlist select foreground restore failed: {error}"))?;
    if inputs.scroll_settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
    }
    steps.push(PlaylistSelectStep {
      name: "click-playlist-foreground-retry".to_string(),
      target_bounds: Some(click_bounds),
      delivery_path: Some("foreground_system_events".to_string()),
      fallback_reason: Some("window-targeted click did not verify selection".to_string()),
    });
    verification = verify_playlist_select_title(
      &session,
      &window,
      window_size,
      sidebar_bounds,
      click_bounds,
      inputs,
      &verification_artifact,
      &target.label,
    )?;
  }

  if verification.method == PLAYLIST_SELECT_VERIFICATION_METHOD_SIDEBAR_ECHO {
    known_limits.push(PLAYLIST_SELECT_VERIFICATION_SIDEBAR_ECHO_LIMIT.to_string());
  }

  Ok(PlaylistSelectResult {
    command: "playlist.select".to_string(),
    query: query.to_string(),
    app: scan.app().clone(),
    window: scan.window().clone(),
    target,
    steps,
    verification,
    diagnostics,
    known_limits,
    reacquire: reacquire_summary,
  })
}

#[cfg(target_os = "macos")]
fn verify_playlist_select_title(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
  row_bounds: ViewBounds,
  inputs: &Inputs,
  verification_artifact: &std::path::Path,
  target_label: &str,
) -> Result<PlaylistSelectVerification, String> {
  let capture = session
    .window()
    .capture(window)
    .map_err(|error| format!("playlist select verification capture failed: {error}"))?;
  capture.image.save(verification_artifact).map_err(|error| {
    format!(
      "failed to save {}: {error}",
      verification_artifact.display()
    )
  })?;

  let recognition_json =
    playlist_select_verification_recognition_artifact_path(verification_artifact);
  let sidebar_echo_json =
    playlist_select_verification_sidebar_echo_recognition_artifact_path(verification_artifact);
  let ocr_options = build_playlist_select_verification_ocr_options(inputs, target_label);
  let ocr_tiers: [(
    &str,
    fn(ViewBounds, auv_driver::Size) -> auv_driver::RatioRect,
  ); 4] = [
    (
      PLAYLIST_SELECT_VERIFICATION_OCR_TITLE_BAND,
      playlist_select_verification_title_band_ratio,
    ),
    (
      PLAYLIST_SELECT_VERIFICATION_OCR_HERO_HEADER,
      playlist_select_verification_hero_header_ratio,
    ),
    (
      PLAYLIST_SELECT_VERIFICATION_OCR_MAIN_BAND,
      playlist_select_verification_main_band_ratio,
    ),
    (
      PLAYLIST_SELECT_VERIFICATION_OCR_FULL_WINDOW,
      playlist_select_verification_full_window_ratio,
    ),
  ];

  let mut method = PLAYLIST_SELECT_VERIFICATION_OCR_FULL_WINDOW;
  let mut observed_title = None;
  let mut last_recognition = None;

  for (tier_method, ratio_for_tier) in ocr_tiers {
    method = tier_method;
    let ocr_ratio = ratio_for_tier(sidebar_bounds, window_size);
    let recognition = session
      .vision()
      .recognize_text_in_capture_with_options(&capture, ocr_ratio, ocr_options.clone())
      .map_err(|error| format!("playlist select verification OCR failed: {error}"))?;
    let recognition = crate::recognition_in_window_space(recognition, &capture);
    last_recognition = Some(recognition.clone());
    // NOTICE(a6c-4b): top-nav OCR in the upper band is not playlist detail title.
    observed_title =
      playlist_select_verification_title(&recognition, window_size, sidebar_bounds, target_label);
    if observed_title.is_some() {
      break;
    }
  }

  let recognition = last_recognition.ok_or_else(|| {
    "playlist select verification OCR produced no recognition payload".to_string()
  })?;
  let mut sidebar_echo_recognition_artifact = None;
  let mut sidebar_echo_attempted = false;
  let mut sidebar_echo_pass = false;

  if observed_title.is_none()
    && crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label)
  {
    sidebar_echo_attempted = true;
    let sidebar_ratio =
      playlist_select_verification_view_bounds_to_ratio(sidebar_bounds, window_size);
    let sidebar_recognition = session
      .vision()
      .recognize_text_in_capture_with_options(&capture, sidebar_ratio, ocr_options.clone())
      .map_err(|error| format!("playlist select verification sidebar echo OCR failed: {error}"))?;
    let sidebar_recognition = crate::recognition_in_window_space(sidebar_recognition, &capture);
    std::fs::write(
      &sidebar_echo_json,
      serde_json::to_string_pretty(&sidebar_recognition)
        .map_err(|error| format!("failed to serialize sidebar echo recognition: {error}"))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", sidebar_echo_json.display()))?;
    sidebar_echo_recognition_artifact = Some(sidebar_echo_json.display().to_string());

    if let Some(echo_title) = playlist_select_verification_sidebar_row_echo_from_recognition(
      &sidebar_recognition,
      &recognition,
      row_bounds,
      target_label,
      window_size,
      sidebar_bounds,
    ) {
      observed_title = Some(echo_title);
      method = PLAYLIST_SELECT_VERIFICATION_METHOD_SIDEBAR_ECHO;
      sidebar_echo_pass = true;
    }
  }

  std::fs::write(
    &recognition_json,
    serde_json::to_string_pretty(&recognition)
      .map_err(|error| format!("failed to serialize recognition: {error}"))?,
  )
  .map_err(|error| format!("failed to write {}: {error}", recognition_json.display()))?;

  let verified = observed_title.is_some();
  let region_count = recognition.regions.len();
  let main_pane_guard_pass = playlist_select_verification_count_main_pane_guard_regions(
    &recognition,
    window_size,
    sidebar_bounds,
  );
  let note = playlist_select_verification_note(
    "title,hero,main,full",
    method,
    region_count,
    main_pane_guard_pass,
    sidebar_echo_attempted,
    sidebar_echo_pass,
  );

  Ok(PlaylistSelectVerification {
    status: if verified { "passed" } else { "failed" }.to_string(),
    method: method.to_string(),
    observed_title,
    artifact: Some(verification_artifact.display().to_string()),
    recognition_artifact: Some(recognition_json.display().to_string()),
    sidebar_echo_recognition_artifact,
    note: Some(note),
  })
}

#[cfg(target_os = "macos")]
pub fn run_playlist_play(inputs: &Inputs, query: &str) -> Result<PlaylistPlayResult, String> {
  let (scan, target, target_from_scan_cache) = resolve_playlist_target_for_query(inputs, query)?;
  run_playlist_play_resolved(inputs, query, scan, target, target_from_scan_cache)
}

#[cfg(target_os = "macos")]
pub fn run_playlist_play_candidate_id(
  inputs: &Inputs,
  candidate_id: &str,
) -> Result<PlaylistPlayResult, String> {
  let scan = load_playlist_scan_cache(inputs)?;
  let target = scan.select_target_by_candidate_id(candidate_id)?;
  run_playlist_play_resolved(inputs, candidate_id, scan, target, true)
}

#[cfg(target_os = "macos")]
fn load_playlist_scan_cache(inputs: &Inputs) -> Result<crate::PlaylistSidebarScan, String> {
  let cache_path = inputs.artifact_dir.join(crate::PLAYLIST_SCAN_CACHE_FILE);
  let json = std::fs::read_to_string(&cache_path).map_err(|error| {
    format!(
      "failed to read playlist scan cache {}: {error}; run `playlist ls <query> --json` first with the same --artifact-dir",
      cache_path.display()
    )
  })?;
  decode_playlist_sidebar_scan_json(&json)
}

#[cfg(target_os = "macos")]
fn run_playlist_play_resolved(
  inputs: &Inputs,
  query: &str,
  scan: crate::PlaylistSidebarScan,
  target: PlaylistSelectTarget,
  target_from_scan_cache: bool,
) -> Result<PlaylistPlayResult, String> {
  use crate::commands::daily_recommended::best_text_match;
  use crate::delivery_path_label;
  use auv_driver::selector::{App, Window};
  use auv_driver::{
    ActivationPolicy, Click, Driver, PrepareForInputOptions, RatioRect, Size, WindowPoint,
  };
  use auv_driver_macos::MacosDriver;

  let select = run_playlist_select_resolved(inputs, query, scan, target, target_from_scan_cache)?;
  if select.verification.status != "passed" {
    return Err(format!(
      "playlist select verification failed before play: observed_title={:?}",
      select.verification.observed_title
    ));
  }

  std::fs::create_dir_all(&inputs.artifact_dir).map_err(|error| {
    format!(
      "failed to create {}: {error}",
      inputs.artifact_dir.display()
    )
  })?;

  let driver = MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(app))
    .map_err(|error| format!("failed to resolve NetEase window: {error}"))?;
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let mut steps = Vec::new();
  let mut artifacts = Vec::new();
  let diagnostics = select.diagnostics.clone();
  let mut known_limits = select.known_limits.clone();

  let capture = session
    .window()
    .capture(&window)
    .map_err(|error| format!("playlist play-all capture failed: {error}"))?;
  let play_all_artifact = inputs.artifact_dir.join("playlist-play-all-target.png");
  capture
    .image
    .save(&play_all_artifact)
    .map_err(|error| format!("failed to save {}: {error}", play_all_artifact.display()))?;
  artifacts.push(play_all_artifact.display().to_string());
  let recognition = session
    .vision()
    .recognize_text_in_capture_with_options(
      &capture,
      RatioRect::new(0.0, 0.0, 1.0, 1.0),
      inputs.ocr_options.clone(),
    )
    .map_err(|error| format!("playlist play-all OCR failed: {error}"))?;
  let recognition = crate::recognition_in_window_space(recognition, &capture);
  let before_bottom_text = recognize_playlist_bottom_text(&session, &capture, inputs);
  let Some(target) = best_text_match(&recognition, "播放全部", window_size, |bounds, size| {
    bounds.x > size.width * 0.18 && bounds.y > size.height * 0.12 && bounds.y < size.height * 0.55
  }) else {
    return Err("playlist play-all text \"播放全部\" was not found".to_string());
  };
  let target_bounds = ViewBounds::new(
    target.bounds.origin.x,
    target.bounds.origin.y,
    target.bounds.size.width,
    target.bounds.size.height,
  );
  let point = target.action_point();
  let click = session
    .window()
    .click(
      &window,
      WindowPoint::new(point.x, point.y),
      playlist_play_click_options(),
    )
    .map_err(|error| format!("playlist play-all click failed: {error}"))?;
  if inputs.scroll_settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
  }
  steps.push(PlaylistPlayStep {
    name: "click-play-all".to_string(),
    target_label: Some(target.text),
    target_bounds: Some(target_bounds),
    delivery_path: Some(delivery_path_label(click.selected_path).to_string()),
    fallback_reason: click.fallback_reason,
    artifact: Some(play_all_artifact.display().to_string()),
  });

  let mut verification = capture_playlist_play_verification(
    &session,
    &window,
    inputs,
    &mut artifacts,
    "playlist-play-post-click-playback-state",
    before_bottom_text.as_deref(),
  )?;
  if verification.status != "passed" {
    known_limits.push(
      "window-targeted Play All click did not verify playback; retried with foreground click"
        .to_string(),
    );
    let screen_point = session
      .window()
      .to_screen_point(&window, WindowPoint::new(point.x, point.y))
      .map_err(|error| format!("playlist play-all foreground point projection failed: {error}"))?;
    let lease = session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("playlist play-all foreground preparation failed: {error}"))?;
    let click_result = session
      .input()
      .click_at(screen_point.point(), Click::Single);
    let restore_result = session.window().restore_input(lease);
    click_result.map_err(|error| format!("playlist play-all foreground click failed: {error}"))?;
    restore_result
      .map_err(|error| format!("playlist play-all foreground restore failed: {error}"))?;
    if inputs.scroll_settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
    }
    steps.push(PlaylistPlayStep {
      name: "click-play-all-foreground-retry".to_string(),
      target_label: Some("播放全部".to_string()),
      target_bounds: Some(target_bounds),
      delivery_path: Some("foreground_system_events".to_string()),
      fallback_reason: Some("window-targeted click did not verify playback".to_string()),
      artifact: Some(play_all_artifact.display().to_string()),
    });
    verification = capture_playlist_play_verification(
      &session,
      &window,
      inputs,
      &mut artifacts,
      "playlist-play-post-foreground-click-playback-state",
      before_bottom_text.as_deref(),
    )?;
  }
  if verification.status != "passed" {
    known_limits.push(
      "playlist play-all click did not change the bottom player from its pre-click state"
        .to_string(),
    );
  }

  Ok(PlaylistPlayResult {
    command: "playlist.play".to_string(),
    query: query.to_string(),
    select,
    steps,
    verification,
    diagnostics,
    known_limits,
    artifacts,
  })
}

#[cfg(target_os = "macos")]
fn capture_playlist_play_verification(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  inputs: &Inputs,
  artifacts: &mut Vec<String>,
  artifact_stem: &str,
  before_bottom_text: Option<&str>,
) -> Result<PlaylistPlayVerification, String> {
  use crate::views::player::classify_bottom_playback_control_state;

  let capture = session
    .window()
    .capture(window)
    .map_err(|error| format!("playlist play verification capture failed: {error}"))?;
  let screenshot = inputs.artifact_dir.join(format!("{artifact_stem}.png"));
  capture
    .image
    .save(&screenshot)
    .map_err(|error| format!("failed to save {}: {error}", screenshot.display()))?;
  artifacts.push(screenshot.display().to_string());
  let control_state = classify_bottom_playback_control_state(&capture.image);
  let bottom_text = recognize_playlist_bottom_text(session, &capture, inputs);
  let verification_json = inputs.artifact_dir.join(format!("{artifact_stem}.json"));
  let status = playlist_play_status_from_bottom_probe(
    control_state,
    before_bottom_text,
    bottom_text.as_deref(),
  );
  let payload = serde_json::json!({
    "method": "bottom_control_icon_with_player_change",
    "status": status,
    "control_state": control_state,
    "before_bottom_text": before_bottom_text,
    "observed_bottom_text": bottom_text,
    "screenshot": screenshot.display().to_string(),
  });
  std::fs::write(
    &verification_json,
    serde_json::to_string_pretty(&payload)
      .map_err(|error| format!("failed to serialize playlist play verification: {error}"))?,
  )
  .map_err(|error| format!("failed to write {}: {error}", verification_json.display()))?;
  artifacts.push(verification_json.display().to_string());

  Ok(PlaylistPlayVerification {
    status: status.to_string(),
    method: "bottom_control_icon_with_player_change".to_string(),
    control_state: Some(control_state),
    observed_bottom_text: bottom_text,
    artifact: Some(verification_json.display().to_string()),
    note: Some(
      "verification checks the bottom playback control and rejects unchanged pre-click playback"
        .to_string(),
    ),
  })
}

#[cfg(target_os = "macos")]
fn recognize_playlist_bottom_text(
  session: &auv_driver_macos::MacosDriverSession,
  capture: &auv_driver::Capture,
  inputs: &Inputs,
) -> Option<String> {
  use auv_driver::RatioRect;

  session
    .vision()
    .recognize_text_in_capture_with_options(
      capture,
      RatioRect::new(0.0, 0.88, 0.46, 0.12),
      inputs.ocr_options.clone(),
    )
    .ok()
    .map(|recognition| recognition.text.trim().to_string())
    .filter(|text| !text.is_empty())
}
