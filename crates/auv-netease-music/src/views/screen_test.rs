use super::*;

#[test]
fn classify_screen_detects_default_from_left_sidebar_marker() {
  let view = classify_screen(&fake_recognition(vec![("发现音乐", 42.0, 96.0, 92.0, 24.0)]), auv_driver::Size::new(1200.0, 800.0));

  assert_eq!(view.state(), ScreenState::Default);
  assert!(view.is_default());
  assert_eq!(view.restore_point(), None);
}

#[test]
fn classify_screen_detects_playing_song_detail_and_restore_point() {
  let view = classify_screen(
    &fake_recognition(vec![
      ("评论", 760.0, 182.0, 80.0, 28.0),
      ("收藏", 880.0, 182.0, 80.0, 28.0),
    ]),
    auv_driver::Size::new(1646.0, 1053.0),
  );

  assert_eq!(view.state(), ScreenState::PlayingSongDetail);
  assert!(view.is_playing_song_detail());
  assert_eq!(view.restore_point(), Some(auv_driver::Point::new(82.602, 16.336)));
}

#[test]
fn classify_screen_detects_song_detail_from_source_and_lyrics_tabs() {
  let view = classify_screen(
    &fake_recognition(vec![
      ("来源：每日歌曲推荐", 850.0, 118.0, 160.0, 24.0),
      ("歌词", 700.0, 246.0, 48.0, 24.0),
      ("百科", 760.0, 246.0, 48.0, 24.0),
      ("相似推荐", 820.0, 246.0, 86.0, 24.0),
    ]),
    auv_driver::Size::new(1200.0, 800.0),
  );

  assert_eq!(view.state(), ScreenState::PlayingSongDetail);
}

#[test]
fn classify_screen_detects_song_detail_from_aligned_detail_tabs_without_source() {
  // ROOT CAUSE:
  //
  // If OCR missed the low-contrast source label on an already-open song
  // detail screen, the playback status probe classified the screen as
  // unknown and clicked the playback bar again.
  //
  // The invariant is that the aligned detail tabs are enough screen evidence;
  // source extraction should not gate detail-screen detection.
  let view = classify_screen(
    &fake_recognition(vec![
      ("歌词", 700.0, 246.0, 48.0, 24.0),
      ("百科", 760.0, 246.0, 48.0, 24.0),
      ("相似推荐", 820.0, 246.0, 86.0, 24.0),
    ]),
    auv_driver::Size::new(1200.0, 800.0),
  );

  assert_eq!(view.state(), ScreenState::PlayingSongDetail);
}

#[test]
fn classify_screen_detects_blocking_modal_before_default() {
  let view = classify_screen(
    &fake_recognition(vec![
      ("推荐", 42.0, 96.0, 52.0, 24.0),
      ("打开", 760.0, 720.0, 80.0, 32.0),
      ("取消", 860.0, 720.0, 80.0, 32.0),
    ]),
    auv_driver::Size::new(1200.0, 800.0),
  );

  assert_eq!(view.state(), ScreenState::BlockingModal);
  assert!(view.is_blocking_modal());
  assert_eq!(view.restore_point(), None);
}

#[test]
fn classify_screen_returns_unknown_without_screen_markers() {
  let view = classify_screen(&fake_recognition(vec![("私人雷达", 620.0, 122.0, 120.0, 28.0)]), auv_driver::Size::new(1200.0, 800.0));

  assert_eq!(view.state(), ScreenState::Unknown);
  assert_eq!(view.restore_point(), None);
}

#[test]
fn song_detail_source_reads_inline_upper_right_source_label() {
  let source =
    song_detail_source(&fake_recognition(vec![("来源：每日推荐", 850.0, 118.0, 160.0, 24.0)]), auv_driver::Size::new(1200.0, 800.0));

  assert_eq!(source.as_deref(), Some("每日推荐"));
}

#[test]
fn song_detail_source_reads_adjacent_upper_right_source_value() {
  let source = song_detail_source(
    &fake_recognition(vec![
      ("来源", 850.0, 118.0, 48.0, 24.0),
      ("我喜欢的音乐", 910.0, 118.0, 128.0, 24.0),
    ]),
    auv_driver::Size::new(1200.0, 800.0),
  );

  assert_eq!(source.as_deref(), Some("我喜欢的音乐"));
}

fn fake_recognition(regions: Vec<(&str, f64, f64, f64, f64)>) -> TextRecognition {
  TextRecognition {
    text: regions.iter().map(|(text, _, _, _, _)| *text).collect::<Vec<_>>().join("\n"),
    regions: regions
      .into_iter()
      .map(|(text, x, y, width, height)| auv_driver::vision::RecognizedText {
        text: text.to_string(),
        bounds: auv_driver::Rect::new(x, y, width, height),
        confidence: Some(0.9),
      })
      .collect(),
  }
}
