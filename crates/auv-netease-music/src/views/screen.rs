use auv_driver::vision::TextRecognition;

// NOTICE: This is a learned window-local logical point for the song-detail
// back affordance, matching the current live NetEase macOS client observation.
const PLAYING_SONG_DETAIL_RESTORE_POINT: auv_driver::Point = auv_driver::Point::new(82.602, 16.336);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenState {
  Default,
  PlayingSongDetail,
  BlockingModal,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenView {
  state: ScreenState,
  restore_point: Option<auv_driver::Point>,
}

impl ScreenView {
  fn new(state: ScreenState, restore_point: Option<auv_driver::Point>) -> Self {
    Self {
      state,
      restore_point,
    }
  }

  /// Build a view when the screen was not classified by this observation.
  pub fn unknown() -> Self {
    Self::new(ScreenState::Unknown, None)
  }

  #[cfg(test)]
  pub(crate) fn for_tests(state: ScreenState, restore_point: Option<auv_driver::Point>) -> Self {
    Self::new(state, restore_point)
  }

  pub fn state(&self) -> ScreenState {
    self.state
  }

  pub fn is_default(&self) -> bool {
    self.state == ScreenState::Default
  }

  pub fn is_playing_song_detail(&self) -> bool {
    self.state == ScreenState::PlayingSongDetail
  }

  pub fn is_blocking_modal(&self) -> bool {
    self.state == ScreenState::BlockingModal
  }

  pub fn restore_point(&self) -> Option<auv_driver::Point> {
    self.restore_point
  }
}

pub fn classify_screen(recognition: &TextRecognition, window_size: auv_driver::Size) -> ScreenView {
  if is_blocking_modal(recognition) {
    return ScreenView::new(ScreenState::BlockingModal, None);
  }

  if has_left_sidebar_marker(recognition, window_size) {
    return ScreenView::new(ScreenState::Default, None);
  }

  if is_playing_song_detail(recognition, window_size) {
    return ScreenView::new(
      ScreenState::PlayingSongDetail,
      Some(PLAYING_SONG_DETAIL_RESTORE_POINT),
    );
  }

  ScreenView::new(ScreenState::Unknown, None)
}

pub fn song_detail_source(
  recognition: &TextRecognition,
  window_size: auv_driver::Size,
) -> Option<String> {
  let mut upper_right_regions = recognition
    .regions
    .iter()
    .filter(|region| {
      region.bounds.origin.x >= window_size.width * 0.55
        && region.bounds.origin.y <= window_size.height * 0.30
    })
    .collect::<Vec<_>>();
  upper_right_regions.sort_by(|left, right| {
    left
      .bounds
      .origin
      .y
      .partial_cmp(&right.bounds.origin.y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| {
        left
          .bounds
          .origin
          .x
          .partial_cmp(&right.bounds.origin.x)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
  });

  for region in &upper_right_regions {
    if let Some(source) = inline_source_value(&region.text) {
      return Some(source);
    }
  }

  for label in upper_right_regions
    .iter()
    .filter(|region| is_source_label(&region.text))
  {
    let label_center_y = label.bounds.origin.y + label.bounds.size.height * 0.5;
    let value = upper_right_regions
      .iter()
      .filter(|region| region.text != label.text || region.bounds != label.bounds)
      .filter(|region| !is_source_label(&region.text))
      .filter(|region| {
        let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
        (center_y - label_center_y).abs() <= 28.0
          && region.bounds.origin.x >= label.bounds.origin.x + label.bounds.size.width
      })
      .min_by(|left, right| {
        left
          .bounds
          .origin
          .x
          .partial_cmp(&right.bounds.origin.x)
          .unwrap_or(std::cmp::Ordering::Equal)
      })?;
    let value = value.text.trim();
    if !value.is_empty() {
      return Some(value.to_string());
    }
  }

  None
}

fn is_blocking_modal(recognition: &TextRecognition) -> bool {
  contains_text(recognition, "取消")
    && (contains_text(recognition, "打开") || contains_text(recognition, "存储"))
}

fn has_left_sidebar_marker(recognition: &TextRecognition, window_size: auv_driver::Size) -> bool {
  let left_boundary = window_size.width * 0.38;
  recognition.regions.iter().any(|region| {
    region.bounds.origin.x < left_boundary && crate::is_sidebar_marker(region.text.trim())
  })
}

fn is_playing_song_detail(recognition: &TextRecognition, window_size: auv_driver::Size) -> bool {
  if contains_text(recognition, "评论") && contains_text(recognition, "收藏") {
    return true;
  }

  if has_aligned_detail_tabs(recognition, window_size) {
    return true;
  }

  song_detail_source(recognition, window_size).is_some()
    && (contains_text(recognition, "歌词")
      || contains_text(recognition, "百科")
      || contains_text(recognition, "相似推荐"))
}

fn has_aligned_detail_tabs(recognition: &TextRecognition, window_size: auv_driver::Size) -> bool {
  let min_x = window_size.width * 0.45;
  let min_y = window_size.height * 0.14;
  let max_y = window_size.height * 0.38;
  let mut tabs = recognition
    .regions
    .iter()
    .filter(|region| {
      region.bounds.origin.x >= min_x
        && region.bounds.origin.y >= min_y
        && region.bounds.origin.y <= max_y
        && matches!(
          region.text.trim(),
          text if text.contains("歌词") || text.contains("百科") || text.contains("相似推荐")
        )
    })
    .collect::<Vec<_>>();
  tabs.sort_by(|left, right| {
    left
      .bounds
      .origin
      .x
      .partial_cmp(&right.bounds.origin.x)
      .unwrap_or(std::cmp::Ordering::Equal)
  });

  tabs.iter().enumerate().any(|(index, left)| {
    let left_center_y = left.bounds.origin.y + left.bounds.size.height * 0.5;
    tabs.iter().skip(index + 1).any(|right| {
      let right_center_y = right.bounds.origin.y + right.bounds.size.height * 0.5;
      (left_center_y - right_center_y).abs() <= 18.0
    })
  })
}

fn contains_text(recognition: &TextRecognition, query: &str) -> bool {
  recognition
    .regions
    .iter()
    .any(|region| region.text.contains(query))
}

fn inline_source_value(text: &str) -> Option<String> {
  let text = text.trim();
  for separator in ["来源：", "来源:", "来源 "] {
    if let Some((_, value)) = text.split_once(separator) {
      let value = value.trim();
      if !value.is_empty() {
        return Some(value.to_string());
      }
    }
  }
  None
}

fn is_source_label(text: &str) -> bool {
  matches!(text.trim().trim_end_matches([':', '：']), "来源")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn classify_screen_detects_default_from_left_sidebar_marker() {
    let view = classify_screen(
      &fake_recognition(vec![("发现音乐", 42.0, 96.0, 92.0, 24.0)]),
      auv_driver::Size::new(1200.0, 800.0),
    );

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
    assert_eq!(
      view.restore_point(),
      Some(auv_driver::Point::new(82.602, 16.336))
    );
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
    let view = classify_screen(
      &fake_recognition(vec![("私人雷达", 620.0, 122.0, 120.0, 28.0)]),
      auv_driver::Size::new(1200.0, 800.0),
    );

    assert_eq!(view.state(), ScreenState::Unknown);
    assert_eq!(view.restore_point(), None);
  }

  #[test]
  fn song_detail_source_reads_inline_upper_right_source_label() {
    let source = song_detail_source(
      &fake_recognition(vec![("来源：每日推荐", 850.0, 118.0, 160.0, 24.0)]),
      auv_driver::Size::new(1200.0, 800.0),
    );

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
      text: regions
        .iter()
        .map(|(text, _, _, _, _)| *text)
        .collect::<Vec<_>>()
        .join("\n"),
      regions: regions
        .into_iter()
        .map(
          |(text, x, y, width, height)| auv_driver::vision::RecognizedText {
            text: text.to_string(),
            bounds: auv_driver::Rect::new(x, y, width, height),
            confidence: Some(0.9),
          },
        )
        .collect(),
    }
  }
}
