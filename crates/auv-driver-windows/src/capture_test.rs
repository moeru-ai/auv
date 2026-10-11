use super::*;

fn target(index: usize, id: &str, frame: Rect, is_primary: bool) -> DisplayTarget {
  DisplayTarget {
    index,
    display: Display {
      id: id.to_string(),
      name: Some(format!("display_{index}")),
      frame,
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary,
      is_builtin: None,
    },
  }
}

#[test]
fn capture_scale_describes_image_pixels_per_desktop_coordinate() {
  let image = image::RgbaImage::new(3840, 2160);
  let bounds = Rect::new(0.0, 0.0, 3840.0, 2160.0);

  // The monitor may report 150% UI scaling, but xcap's image and its
  // desktop-coordinate frame are both 3840 pixels wide on this machine.
  assert_eq!(capture_scale_factor(&image, bounds), 1.0);
}

#[cfg(target_os = "windows")]
#[test]
fn capture_retries_only_the_observed_transient_invalid_handle() {
  let mut attempts = 0;
  let image = retry_invalid_handle(|| {
    attempts += 1;
    if attempts == 1 {
      Err(backend("failed to capture display: The handle is invalid. (0x80070006)"))
    } else {
      Ok(42)
    }
  })
  .unwrap();

  assert_eq!(image, 42);
  assert_eq!(attempts, 2);

  let mut attempts = 0;
  let error = retry_invalid_handle::<()>(|| {
    attempts += 1;
    Err(backend("failed to capture display: access denied"))
  })
  .unwrap_err();

  assert!(error.to_string().contains("access denied"));
  assert_eq!(attempts, 1);
}

#[test]
fn resolve_display_target_prefers_primary_without_selector() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), false),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), true),
  ];

  let resolved = resolve_display_target(&targets, None).expect("primary should resolve");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_display_target_matches_selector_by_id_or_name() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  assert_eq!(resolve_display_target(&targets, Some("b")).unwrap().index, 1);
  assert_eq!(resolve_display_target(&targets, Some("display_1")).unwrap().index, 1);
  assert!(resolve_display_target(&targets, Some("missing")).is_err());
}

#[test]
fn resolve_display_for_region_selects_containing_display() {
  let targets = vec![
    target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    target(1, "b", Rect::new(100.0, 0.0, 100.0, 100.0), false),
  ];

  let region = Rect::new(110.0, 10.0, 20.0, 20.0);
  let resolved = resolve_display_for_region(&targets, None, region).expect("region is within display b");

  assert_eq!(resolved.index, 1);
}

#[test]
fn resolve_display_for_region_rejects_region_spanning_displays() {
  let targets = vec![target(0, "a", Rect::new(0.0, 0.0, 100.0, 100.0), true)];

  let region = Rect::new(50.0, 50.0, 100.0, 10.0);

  assert!(resolve_display_for_region(&targets, None, region).is_err());
}

#[test]
fn integral_capture_dimension_rejects_fractional_and_negative() {
  assert!(integral_capture_dimension("x", 10.5).is_err());
  assert!(integral_capture_dimension("x", -1.0).is_err());
  assert_eq!(integral_capture_dimension("x", 12.0).unwrap(), 12);
}

#[test]
fn integral_positive_capture_dimension_rejects_zero() {
  assert!(integral_positive_capture_dimension("width", 0.0).is_err());
  assert_eq!(integral_positive_capture_dimension("width", 4.0).unwrap(), 4);
}

// ROOT CAUSE:
//
// If a Windows 10/11 window has its invisible resize border, `window.findText` placed OCR
// anchors up to 7 px away from the text and scaled them 3.8% too small.
//
// `PrintWindow` renders the whole `GetWindowRect` rectangle, but `Window.frame` and
// `Capture.bounds` are the DWM extended frame, which leaves that border out. The pixels started
// at the window rect's origin while the bounds claimed the frame's origin, 7 px to the right, and
// `scale_factor` was image width over frame width (380 / 366).
//
// Before the fix, the whole bitmap was kept. The fix keeps only the pixels of the visible frame,
// so the image and the bounds describe the same rectangle. Numbers are from
// `evidence/2026-10-10-windows-background-session/geometry.txt`: window rect [1980, 960, 2360,
// 1260], frame 1987,960 366x293.
#[test]
fn visible_crop_drops_the_invisible_resize_border() {
  let window_rect = Rect::new(1980.0, 960.0, 380.0, 300.0);
  let frame = Rect::new(1987.0, 960.0, 366.0, 293.0);

  let crop = visible_crop(window_rect, (380, 300), frame).expect("the frame lies inside the bitmap");

  assert_eq!(
    crop,
    PixelCrop {
      x: 7,
      y: 0,
      width: 366,
      height: 293
    }
  );
}

#[test]
fn visible_crop_keeps_everything_when_the_frame_is_the_window_rect() {
  let rect = Rect::new(100.0, 200.0, 640.0, 480.0);

  assert_eq!(
    visible_crop(rect, (640, 480), rect),
    Some(PixelCrop {
      x: 0,
      y: 0,
      width: 640,
      height: 480
    })
  );
}

#[test]
fn visible_crop_stays_inside_the_bitmap_when_the_window_moved_between_reads() {
  // The frame was read a moment after the rect and the window had moved 5 px left: part of it
  // now lies outside the bitmap and is cut off instead of reading past the buffer.
  let window_rect = Rect::new(100.0, 100.0, 200.0, 100.0);
  let frame = Rect::new(95.0, 100.0, 200.0, 100.0);

  let crop = visible_crop(window_rect, (200, 100), frame).expect("most of the frame is inside");

  assert_eq!(
    crop,
    PixelCrop {
      x: 0,
      y: 0,
      width: 195,
      height: 100
    }
  );
}

#[test]
fn visible_crop_refuses_a_frame_that_misses_the_bitmap() {
  let window_rect = Rect::new(0.0, 0.0, 100.0, 100.0);

  assert_eq!(visible_crop(window_rect, (100, 100), Rect::new(500.0, 500.0, 100.0, 100.0)), None);
  assert_eq!(visible_crop(window_rect, (100, 100), Rect::new(0.0, 0.0, 0.0, 0.0)), None);
}
