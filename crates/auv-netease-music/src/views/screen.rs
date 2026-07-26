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
    return ScreenView::new(ScreenState::PlayingSongDetail, Some(PLAYING_SONG_DETAIL_RESTORE_POINT));
  }

  ScreenView::new(ScreenState::Unknown, None)
}

pub fn song_detail_source(recognition: &TextRecognition, window_size: auv_driver::Size) -> Option<String> {
  let mut upper_right_regions = recognition
    .regions
    .iter()
    .filter(|region| region.bounds.origin.x >= window_size.width * 0.55 && region.bounds.origin.y <= window_size.height * 0.30)
    .collect::<Vec<_>>();
  upper_right_regions.sort_by(|left, right| {
    left
      .bounds
      .origin
      .y
      .partial_cmp(&right.bounds.origin.y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal))
  });

  for region in &upper_right_regions {
    if let Some(source) = inline_source_value(&region.text) {
      return Some(source);
    }
  }

  for label in upper_right_regions.iter().filter(|region| is_source_label(&region.text)) {
    let label_center_y = label.bounds.origin.y + label.bounds.size.height * 0.5;
    let value = upper_right_regions
      .iter()
      .filter(|region| region.text != label.text || region.bounds != label.bounds)
      .filter(|region| !is_source_label(&region.text))
      .filter(|region| {
        let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
        (center_y - label_center_y).abs() <= 28.0 && region.bounds.origin.x >= label.bounds.origin.x + label.bounds.size.width
      })
      .min_by(|left, right| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal))?;
    let value = value.text.trim();
    if !value.is_empty() {
      return Some(value.to_string());
    }
  }

  None
}

fn is_blocking_modal(recognition: &TextRecognition) -> bool {
  recognition.best_contains("取消").is_some() && (recognition.best_contains("打开").is_some() || recognition.best_contains("存储").is_some())
}

fn has_left_sidebar_marker(recognition: &TextRecognition, window_size: auv_driver::Size) -> bool {
  let left_boundary = window_size.width * 0.38;
  recognition.regions.iter().any(|region| region.bounds.origin.x < left_boundary && crate::is_sidebar_marker(region.text.trim()))
}

fn is_playing_song_detail(recognition: &TextRecognition, window_size: auv_driver::Size) -> bool {
  if recognition.best_contains("评论").is_some() && recognition.best_contains("收藏").is_some() {
    return true;
  }

  if has_aligned_detail_tabs(recognition, window_size) {
    return true;
  }

  song_detail_source(recognition, window_size).is_some()
    && (recognition.best_contains("歌词").is_some()
      || recognition.best_contains("百科").is_some()
      || recognition.best_contains("相似推荐").is_some())
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
  tabs.sort_by(|left, right| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal));

  tabs.iter().enumerate().any(|(index, left)| {
    let left_center_y = left.bounds.origin.y + left.bounds.size.height * 0.5;
    tabs.iter().skip(index + 1).any(|right| {
      let right_center_y = right.bounds.origin.y + right.bounds.size.height * 0.5;
      (left_center_y - right_center_y).abs() <= 18.0
    })
  })
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
#[path = "screen_test.rs"]
mod tests;
