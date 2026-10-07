use super::*;
use image::{Rgba, RgbaImage};

#[test]
fn classify_playback_control_state_distinguishes_pause_from_play_icon() {
  let pause = playback_control_test_image(PlaybackControlState::PauseVisible);
  let play = playback_control_test_image(PlaybackControlState::PlayVisible);

  assert_eq!(classify_bottom_playback_control_state(&pause, 1.0), PlaybackControlState::PauseVisible);
  assert_eq!(classify_bottom_playback_control_state(&play, 1.0), PlaybackControlState::PlayVisible);
}

#[test]
fn classify_playback_control_state_reads_native_resolution_captures() {
  let pause = double(&playback_control_test_image(PlaybackControlState::PauseVisible));
  let play = double(&playback_control_test_image(PlaybackControlState::PlayVisible));

  assert_eq!(classify_bottom_playback_control_state(&pause, 2.0), PlaybackControlState::PauseVisible);
  assert_eq!(classify_bottom_playback_control_state(&play, 2.0), PlaybackControlState::PlayVisible);
}

fn double(image: &RgbaImage) -> RgbaImage {
  image::imageops::resize(image, image.width() * 2, image.height() * 2, image::imageops::FilterType::Nearest)
}

fn playback_control_test_image(state: PlaybackControlState) -> RgbaImage {
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
