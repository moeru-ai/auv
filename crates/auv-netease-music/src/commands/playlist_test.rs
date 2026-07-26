use super::*;

#[test]
fn playlist_select_requests_bottom_padding_scroll_for_targets_inside_bottom_safe_band() {
  let sidebar = ViewBounds::new(0.0, 0.0, 320.0, 860.0);
  let unsafe_target = ViewBounds::new(72.0, 800.0, 154.0, 14.0);
  let safe_target = ViewBounds::new(72.0, 620.0, 154.0, 14.0);

  assert!(playlist_select_bottom_padding_scroll_needed(unsafe_target, sidebar));
  assert!(!playlist_select_bottom_padding_scroll_needed(safe_target, sidebar));
}

fn sample_playlist_select_window() -> auv_driver::Size {
  auv_driver::Size::new(1057.0, 752.0)
}

fn sample_playlist_select_sidebar() -> ViewBounds {
  ViewBounds::new(0.0, 220.0, 346.0, 720.0)
}

#[test]
fn playlist_select_verification_title_matches_in_main_pane() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![
    ("曲。播客。有声书。歌单。专辑。", 380.0, 48.0, 280.0, 18.0),
    ("最近播放", 420.0, 198.0, 120.0, 28.0),
  ]);

  let title =
    playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "最近播放");

  assert_eq!(title.as_deref(), Some("最近播放"));
}

#[test]
fn playlist_select_verification_title_rejects_top_nav_band() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![("曲。播客。有声书。歌单。专辑。", 380.0, 48.0, 280.0, 18.0)]);

  let title =
    playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "最近播放");

  assert!(title.is_none());
}

#[test]
fn playlist_select_verification_title_prefers_label_over_unrelated() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![
    ("其他推荐歌单", 420.0, 240.0, 140.0, 24.0),
    ("最近播放", 420.0, 198.0, 120.0, 28.0),
  ]);

  let title =
    playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "最近播放");

  assert_eq!(title.as_deref(), Some("最近播放"));
}

#[test]
fn playlist_select_verification_title_matches_single_digit_exact() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![
    ("曲。播客。有声书。歌单。专辑。", 380.0, 48.0, 280.0, 18.0),
    ("3", 420.0, 198.0, 12.0, 28.0),
  ]);

  let title = playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "3");

  assert_eq!(title.as_deref(), Some("3"));
}

#[test]
fn playlist_select_verification_title_rejects_only_contains_digit_collision() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![("43", 420.0, 198.0, 24.0, 28.0)]);

  let title = playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "3");

  assert!(title.is_none());
}

#[test]
fn playlist_select_verification_title_prefers_exact_digit_when_collision_present() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![
    ("43", 420.0, 198.0, 24.0, 28.0),
    ("3", 420.0, 240.0, 12.0, 28.0),
  ]);

  let title = playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "3");

  assert_eq!(title.as_deref(), Some("3"));
}

#[test]
fn playlist_select_verification_title_rejects_nav_band_single_digit() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![("3", 380.0, 48.0, 12.0, 18.0)]);

  let title = playlist_select_verification_title(&recognition, sample_playlist_select_window(), sample_playlist_select_sidebar(), "3");

  assert!(title.is_none());
}

#[test]
fn build_playlist_select_verification_ocr_options_boosts_single_digit_target() {
  let inputs = Inputs::with_defaults();
  let options = build_playlist_select_verification_ocr_options(&inputs, "3");

  assert!(options.custom_words.iter().any(|word| word == "3"));
  assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
}

#[test]
fn build_playlist_select_verification_ocr_options_leaves_non_digit_unchanged() {
  let base = auv_driver::vision::TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
  let inputs = Inputs {
    ocr_options: base.clone(),
    ..Inputs::with_defaults()
  };
  let options = build_playlist_select_verification_ocr_options(&inputs, "最近播放");

  assert_eq!(options, base);
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
  let title = playlist_select_verification_ratio(PlaylistSelectTitleOcrTier::TitleBand, sidebar, window);
  let hero = playlist_select_verification_ratio(PlaylistSelectTitleOcrTier::HeroHeader, sidebar, window);
  let main = playlist_select_verification_ratio(PlaylistSelectTitleOcrTier::MainBand, sidebar, window);
  let full = playlist_select_verification_ratio(PlaylistSelectTitleOcrTier::FullWindow, sidebar, window);

  assert_eq!((title.y, title.height), (0.12, 0.22));
  assert_eq!((main.y, main.height), (0.10, 0.45));
  assert_eq!(full, auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0));
  let y_start = hero.y * window.height;
  let y_end = y_start + hero.height * window.height;
  assert!(y_start < 139.0);
  assert!(y_end > 139.0);
  assert!(y_end <= 165.0);
}

#[test]
fn playlist_select_verification_detail_chrome_passes_with_play_all() {
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![("▶ 播放全部", 446.0, 233.0, 73.0, 19.0)]);

  assert!(playlist_select_verification_detail_chrome_present(
    &recognition,
    sample_playlist_select_window_1812(),
    sample_playlist_select_sidebar_1812(),
  ));
}

#[test]
fn playlist_select_verification_detail_chrome_passes_with_song_and_comment() {
  use crate::recognition_test_data::fake_recognition;

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
  use crate::recognition_test_data::fake_recognition;

  let recognition = fake_recognition(vec![("歌曲", 242.0, 290.0, 35.0, 18.0)]);

  assert!(!playlist_select_verification_detail_chrome_present(
    &recognition,
    sample_playlist_select_window_1812(),
    sample_playlist_select_sidebar_1812(),
  ));
}

#[test]
fn playlist_select_verification_sidebar_row_echo_passes_with_play_all_chrome() {
  use crate::recognition_test_data::fake_recognition;

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
  use crate::recognition_test_data::fake_recognition;

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
  use crate::recognition_test_data::fake_recognition;

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
  use crate::recognition_test_data::fake_recognition;

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
  use crate::recognition_test_data::fake_recognition;

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
