use super::*;
use auv_driver_common::CoordinateSpace;

#[test]
fn crop_capture_to_window_uses_window_extents_inside_display_capture() {
  let mut image = image::RgbaImage::new(10, 10);
  image.put_pixel(3, 4, image::Rgba([1, 2, 3, 4]));
  let capture = Capture {
    origin: None,
    image,
    bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    scale_factor: 1.0,
    backend: "test".to_string(),
    fallback_reason: None,
  };

  let cropped = crop_capture_to_window(&capture, Rect::new(3.0, 4.0, 2.0, 2.0)).unwrap();

  assert_eq!(cropped.width(), 2);
  assert_eq!(cropped.height(), 2);
  assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
}

fn capture_target() -> Window {
  Window {
    reference: WindowRef {
      id: "atspi::1.42/org/a11y/atspi/accessible/1".into(),
    },
    title: Some("Editor".into()),
    app_name: None,
    application_id: None,
    process_id: None,
    frame: Rect::new(3.0, 4.0, 2.0, 2.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
    state: Default::default(),
  }
}

#[test]
fn capture_target_rejects_another_reference_with_matching_title_and_frame() {
  // ROOT CAUSE:
  //
  // If the requested reference was absent, capture used the caller's frame
  // without resolving that reference. Another window's pixels could receive
  // the missing reference. The fix requires an exact live reference match.
  let target = capture_target();
  let mut other = target.clone();
  other.reference.id = "atspi::1.43/org/a11y/atspi/accessible/1".into();

  let error = resolve_capture_target(&[other], &target.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
  assert!(error.to_string().contains(&target.reference.id));
}

#[test]
fn capture_target_rejects_invisible_window() {
  let mut target = capture_target();
  target.is_visible = false;

  let error = resolve_capture_target(&[target.clone()], &target.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
}

#[test]
fn capture_target_uses_current_frame_for_crop_and_bounds() {
  // A caller's Window is a snapshot. Capture must refresh its frame by reference.
  let stale = capture_target();
  let mut current = stale.clone();
  current.frame = Rect::new(6.0, 7.0, 3.0, 2.0);
  let windows = [current.clone()];
  let resolved = resolve_capture_target(&windows, &stale.reference, None).unwrap();
  let mut image = image::RgbaImage::new(10, 10);
  image.put_pixel(6, 7, image::Rgba([1, 2, 3, 255]));
  let display = Capture {
    origin: None,
    image,
    bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
    scale_factor: 1.0,
    backend: "test".into(),
    fallback_reason: None,
  };

  let crop = crop_capture_to_window(&display, resolved.frame).unwrap();

  assert_eq!(resolved, &current);
  assert_eq!(crop.dimensions(), (3, 2));
  assert_eq!(*crop.get_pixel(0, 0), image::Rgba([1, 2, 3, 255]));
}

#[test]
fn capture_target_rejects_current_origin_shared_with_another_window() {
  let stale = capture_target();
  let mut current = stale.clone();
  current.frame.origin.x = 6.0;
  let mut other = current.clone();
  other.reference.id = "atspi::1.43/org/a11y/atspi/accessible/1".into();

  let error = resolve_capture_target(&[current, other], &stale.reference, None).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("shares AT-SPI origin"));
}

#[test]
fn capture_target_rejects_window_closed_during_capture() {
  let target = capture_target();

  let error = resolve_capture_target(&[], &target.reference, Some(target.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::NotFound { .. }));
}

#[test]
fn capture_target_rejects_window_moved_during_capture() {
  let before = capture_target();
  let mut after = before.clone();
  after.frame.origin.x += 1.0;

  let error = resolve_capture_target(&[after], &before.reference, Some(before.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("changed frame during display capture"));
}

#[test]
fn capture_target_rejects_window_resized_during_capture() {
  let before = capture_target();
  let mut after = before.clone();
  after.frame.size.width += 1.0;

  let error = resolve_capture_target(&[after], &before.reference, Some(before.frame)).unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert!(error.to_string().contains("changed frame during display capture"));
}

#[test]
fn capture_target_accepts_unchanged_window_after_capture() {
  let target = capture_target();
  let windows = [target.clone()];

  let resolved = resolve_capture_target(&windows, &target.reference, Some(target.frame)).unwrap();

  assert_eq!(resolved, &target);
}

#[test]
fn window_capture_rechecks_the_same_target_after_reading_display_pixels() {
  use std::cell::RefCell;
  let events = RefCell::new(Vec::new());
  let mut window = capture_target();
  window.frame = Rect::new(2.0, 3.0, 2.0, 2.0);
  let image = image::RgbaImage::from_fn(10, 10, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
  let captured = capture_window(
    &window,
    || {
      events.borrow_mut().push("list");
      Ok(vec![window.clone()])
    },
    || {
      events.borrow_mut().push("capture");
      Ok(DisplayCapture {
        display: auv_driver_common::Display {
          id: "display".into(),
          name: None,
          frame: Rect::new(0.0, 0.0, 10.0, 10.0),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor: 1.0,
          is_primary: true,
          is_builtin: None,
        },
        capture: Capture {
          origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::new(0.0, 0.0))),
          image,
          bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
          scale_factor: 1.0,
          backend: "portal-test".into(),
          fallback_reason: None,
        },
      })
    },
  )
  .unwrap();
  assert_eq!(*events.borrow(), ["list", "capture", "list"]);
  assert_eq!(captured.bounds, window.frame);
  assert_eq!(captured.image.dimensions(), (2, 2));
  assert_eq!(*captured.image.get_pixel(0, 0), image::Rgba([2, 3, 0, 255]));
  assert_eq!(
    captured.origin,
    Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0)))
  );
  assert_eq!(captured.backend, "atspi.extents+portal-test.crop");
  assert_eq!(captured.fallback_reason.as_deref(), Some("window pixels were cropped from display capture using AT-SPI screen extents"));
}
