use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackControlState {
  PlayVisible,
  PauseVisible,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
  Present,
  Absent,
  Unknown,
}

/// Read-only bottom-player facade backed by reconstructed or verified playback state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerView {
  state: PlayerState,
  control_state: Option<PlaybackControlState>,
  observed_text: Option<String>,
}

impl PlayerView {
  /// Build a view for a caller-proven absent player bar.
  pub fn absent() -> Self {
    Self {
      state: PlayerState::Absent,
      control_state: None,
      observed_text: None,
    }
  }

  /// Build a view when the player was not classified by this observation.
  pub fn unknown() -> Self {
    Self {
      state: PlayerState::Unknown,
      control_state: None,
      observed_text: None,
    }
  }

  /// Build a view from the current bottom playback control state.
  pub fn from_control_state(control_state: PlaybackControlState) -> Self {
    let state = match control_state {
      PlaybackControlState::PlayVisible | PlaybackControlState::PauseVisible => {
        PlayerState::Present
      }
      PlaybackControlState::Unknown => PlayerState::Unknown,
    };

    Self {
      state,
      control_state: Some(control_state),
      observed_text: None,
    }
  }

  pub fn from_bottom_bar_text(observed_text: impl Into<String>) -> Self {
    let observed_text = observed_text.into();
    if observed_text.trim().is_empty() {
      return Self::unknown();
    }

    Self {
      state: PlayerState::Present,
      control_state: None,
      observed_text: Some(observed_text),
    }
  }

  /// Attach optional OCR text observed around the player bar.
  pub fn with_observed_text(mut self, observed_text: impl Into<String>) -> Self {
    self.observed_text = Some(observed_text.into());
    self
  }

  pub fn state(&self) -> PlayerState {
    self.state
  }

  pub fn exists(&self) -> bool {
    self.state == PlayerState::Present
  }

  /// NetEase shows a pause affordance while playback is active.
  pub fn is_playing(&self) -> bool {
    self.control_state == Some(PlaybackControlState::PauseVisible)
  }

  pub fn control_state(&self) -> Option<PlaybackControlState> {
    self.control_state
  }

  pub fn observed_text(&self) -> Option<&str> {
    self.observed_text.as_deref()
  }

  /// Return a window-local point that should open the current song detail view.
  pub fn song_detail_click_point(
    &self,
    window_size: auv_driver::Size,
  ) -> Option<auv_driver::Point> {
    if !self.exists() {
      return None;
    }

    // NOTICE(netease-playback-bar-open-detail): click the artwork/title hot
    // zone on the left side of the bottom playback bar. Wider blank areas in
    // the bar do not open the song detail view, and centered controls toggle
    // playback instead.
    Some(auv_driver::Point::new(
      window_size.width * 0.075,
      window_size.height - 38.0,
    ))
  }
}

pub(crate) fn classify_bottom_playback_control_state(image: &RgbaImage) -> PlaybackControlState {
  if image.width() < 80 || image.height() < 80 {
    return PlaybackControlState::Unknown;
  }

  let center_x = image.width() as i32 / 2;
  let center_y = image.height() as i32 - 38;
  let half_width = 24i32;
  let half_height = 22i32;
  let left = (center_x - half_width).max(0);
  let right = (center_x + half_width).min(image.width() as i32 - 1);
  let top = (center_y - half_height).max(0);
  let bottom = (center_y + half_height).min(image.height() as i32 - 1);
  let width = (right - left + 1).max(0) as usize;
  if width == 0 {
    return PlaybackControlState::Unknown;
  }

  let mut occupied_columns = vec![false; width];
  for y in top..=bottom {
    for x in left..=right {
      let pixel = image.get_pixel(x as u32, y as u32);
      if pixel[3] > 120 && pixel[0] > 220 && pixel[1] > 220 && pixel[2] > 220 {
        occupied_columns[(x - left) as usize] = true;
      }
    }
  }

  let mut clusters = Vec::new();
  let mut start = None;
  for (index, occupied) in occupied_columns.iter().copied().enumerate() {
    match (start, occupied) {
      (None, true) => start = Some(index),
      (Some(first), false) => {
        if index.saturating_sub(first) >= 2 {
          clusters.push((first, index - 1));
        }
        start = None;
      }
      _ => {}
    }
  }
  if let Some(first) = start {
    let last = occupied_columns.len() - 1;
    if last.saturating_sub(first) + 1 >= 2 {
      clusters.push((first, last));
    }
  }

  match clusters.len() {
    0 => PlaybackControlState::Unknown,
    1 => PlaybackControlState::PlayVisible,
    _ => PlaybackControlState::PauseVisible,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use image::{Rgba, RgbaImage};

  #[test]
  fn pause_control_means_present_and_playing() {
    let view = PlayerView::from_control_state(PlaybackControlState::PauseVisible);

    assert_eq!(view.state(), PlayerState::Present);
    assert!(view.exists());
    assert!(view.is_playing());
  }

  #[test]
  fn play_control_means_present_but_not_playing() {
    let view = PlayerView::from_control_state(PlaybackControlState::PlayVisible);

    assert_eq!(view.state(), PlayerState::Present);
    assert!(view.exists());
    assert!(!view.is_playing());
  }

  #[test]
  fn unknown_control_does_not_claim_player_exists() {
    let view = PlayerView::from_control_state(PlaybackControlState::Unknown);

    assert_eq!(view.state(), PlayerState::Unknown);
    assert!(!view.exists());
    assert!(!view.is_playing());
  }

  #[test]
  fn unknown_player_has_no_control_state_when_not_observed() {
    let view = PlayerView::unknown();

    assert_eq!(view.state(), PlayerState::Unknown);
    assert_eq!(view.control_state(), None);
    assert!(!view.exists());
  }

  #[test]
  fn absent_player_has_no_control_state() {
    let view = PlayerView::absent();

    assert_eq!(view.state(), PlayerState::Absent);
    assert_eq!(view.control_state(), None);
  }

  #[test]
  fn present_player_exposes_song_detail_click_point() {
    let view = PlayerView::from_control_state(PlaybackControlState::PauseVisible);

    assert_eq!(
      view.song_detail_click_point(auv_driver::Size::new(1200.0, 800.0)),
      Some(auv_driver::Point::new(90.0, 762.0))
    );
  }

  #[test]
  fn absent_or_unknown_player_has_no_song_detail_click_point() {
    assert_eq!(
      PlayerView::absent().song_detail_click_point(auv_driver::Size::new(1200.0, 800.0)),
      None
    );
    assert_eq!(
      PlayerView::unknown().song_detail_click_point(auv_driver::Size::new(1200.0, 800.0)),
      None
    );
  }

  #[test]
  fn bottom_bar_text_marks_player_present_without_control_state() {
    let view = PlayerView::from_bottom_bar_text("Eos\nginkinha");

    assert!(view.exists());
    assert!(!view.is_playing());
    assert_eq!(view.control_state(), None);
    assert_eq!(view.observed_text(), Some("Eos\nginkinha"));
    assert_eq!(
      view.song_detail_click_point(auv_driver::Size::new(1200.0, 800.0)),
      Some(auv_driver::Point::new(90.0, 762.0))
    );
  }

  #[test]
  fn classify_playback_control_state_distinguishes_pause_from_play_icon() {
    let pause = playback_control_fixture(PlaybackControlState::PauseVisible);
    let play = playback_control_fixture(PlaybackControlState::PlayVisible);

    assert_eq!(
      classify_bottom_playback_control_state(&pause),
      PlaybackControlState::PauseVisible
    );
    assert_eq!(
      classify_bottom_playback_control_state(&play),
      PlaybackControlState::PlayVisible
    );
  }

  fn playback_control_fixture(state: PlaybackControlState) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(200, 120, Rgba([14, 15, 24, 255]));
    match state {
      PlaybackControlState::PauseVisible => {
        paint_control_columns(&mut image, &[92..=96, 104..=108]);
      }
      PlaybackControlState::PlayVisible => {
        paint_control_columns(&mut image, &[96..=108]);
      }
      PlaybackControlState::Unknown => {}
    }
    image
  }

  fn paint_control_columns(image: &mut RgbaImage, columns: &[std::ops::RangeInclusive<u32>]) {
    for column in columns {
      for x in column.clone() {
        for y in 72..=94 {
          image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
      }
    }
  }
}
