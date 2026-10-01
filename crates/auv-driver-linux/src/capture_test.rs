use super::*;

#[cfg(target_os = "linux")]
fn screenshot_target() -> display::DisplayTarget {
  display::DisplayTarget {
    display: Display {
      id: "DP-1".into(),
      name: None,
      frame: Rect::new(0.0, 0.0, 2752.0, 1152.0),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 1.25,
      is_primary: true,
      is_builtin: None,
    },
  }
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_rejects_partial_image_before_binding_screen_coordinates() {
  // ROOT CAUSE:
  //
  // If the interactive Screenshot portal returned a selected area, AUV bound
  // its pixels to the full display because the response contained only a URI.
  // Before the fix, a live 377x48 image became a 2752x1152 display capture.
  // The fix rejects pixels that do not match the known display dimensions.
  let result = capture_display_from_captured(
    Some(screenshot_target()),
    CapturedImage {
      image: image::RgbaImage::new(377, 48),
      backend: PORTAL_CAPTURE_BACKEND.into(),
      fallback_reason: None,
    },
  );

  let error = result.err().expect("a selected area must not become a screen-bound capture");
  assert!(error.to_string().contains("377x48"), "{error}");
  assert!(error.to_string().contains("3440x1440"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_rejects_missing_display_geometry() {
  let result = capture_display_from_captured(
    None,
    CapturedImage {
      image: image::RgbaImage::new(377, 48),
      backend: PORTAL_CAPTURE_BACKEND.into(),
      fallback_reason: None,
    },
  );

  let error = result.err().expect("image dimensions cannot establish a screen origin");
  assert!(error.to_string().contains("known display geometry"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_rejects_partial_image_with_matching_aspect_ratio() {
  let error = capture_display_from_captured(
    Some(screenshot_target()),
    CapturedImage {
      image: image::RgbaImage::new(344, 144),
      backend: PORTAL_CAPTURE_BACKEND.into(),
      fallback_reason: None,
    },
  )
  .err()
  .expect("matching aspect ratios do not prove full-screen coverage");

  assert!(error.to_string().contains("3440x1440"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_preserves_scaled_display_origin_and_crop_pixels() {
  let mut target = screenshot_target();
  target.display.frame = Rect::new(-80.0, 20.0, 80.0, 40.0);
  let mut pixels = image::RgbaImage::new(100, 50);
  pixels.put_pixel(25, 10, image::Rgba([1, 2, 3, 255]));
  let captured = capture_display_from_captured(
    Some(target.clone()),
    CapturedImage {
      image: pixels,
      backend: PORTAL_CAPTURE_BACKEND.into(),
      fallback_reason: Some("primary capture failed".into()),
    },
  )
  .expect("a complete screenshot retains its display mapping");

  assert_eq!(captured.display, target.display);
  assert_eq!(captured.capture.bounds, target.display.frame);
  assert_eq!(captured.capture.scale_factor, 1.25);
  assert_eq!(captured.capture.origin, Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::new(-80.0, 20.0))));
  assert_eq!(captured.capture.fallback_reason.as_deref(), Some("primary capture failed"));
  let crop = crop_portal_screenshot_to_region(captured.capture.image, captured.capture.bounds, Rect::new(-60.0, 28.0, 8.0, 8.0))
    .expect("the crop uses the display origin and scale");
  assert_eq!(crop.dimensions(), (10, 10));
  assert_eq!(*crop.get_pixel(0, 0), image::Rgba([1, 2, 3, 255]));
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_rejects_multiple_outputs_before_requesting_pixels() {
  let target = screenshot_target();
  let mut other = target.clone();
  other.display.id = "DP-2".into();
  other.display.frame.origin.x = 2752.0;
  let error = screenshot_source(&[target.clone(), other], &target).unwrap_err();

  assert!(error.to_string().contains("exactly one known display, found 2"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn screenshot_fallback_rejects_changed_display_geometry() {
  let target = screenshot_target();
  let mut changed = target.clone();
  changed.display.frame.origin.x = 100.0;
  let error = screenshot_source(&[changed], &target).unwrap_err();

  assert_eq!(error.to_string(), "display geometry changed during Screenshot fallback");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a live single-output Wayland desktop and Screenshot portal consent"]
fn screenshot_fallback_live_single_display() {
  let targets = list_targets().expect("live output metadata");
  let target = display::resolve_target(&targets, None).expect("live display");
  let primary = backend("forced primary failure for live Screenshot validation");
  let captured =
    capture_fallback(PORTAL_SCREENCAST_BACKEND, &primary, || capture_area(target.display.frame, &target)).expect("real Screenshot fallback");
  let captured =
    capture_display_from_captured(Some(target), with_primary_capture_failure(captured, PORTAL_SCREENCAST_BACKEND, &primary.to_string()))
      .expect("live screenshot has proven display dimensions");
  assert!(captured.capture.fallback_reason.as_deref().unwrap().contains("forced primary failure"));
  let directory = tempfile::tempdir().unwrap();
  let path =
    std::env::var_os("AUV_SCREENSHOT_TEST_PNG").map(std::path::PathBuf::from).unwrap_or_else(|| directory.path().join("screenshot.png"));
  captured.capture.image.save(&path).unwrap();
  eprintln!(
    "display={:?} bounds={:?} pixels={:?} scale={} backend={} fallback_reason={:?} png={}",
    captured.display.id,
    captured.capture.bounds,
    captured.capture.image.dimensions(),
    captured.capture.scale_factor,
    captured.capture.backend,
    captured.capture.fallback_reason,
    path.display()
  );
}

#[test]
fn portal_crop_maps_logical_bounds_to_screenshot_pixels() {
  let mut image = image::RgbaImage::new(100, 50);
  image.put_pixel(20, 10, image::Rgba([1, 2, 3, 4]));

  // ROOT CAUSE:
  //
  // If the portal returned an image in a different pixel size than GNOME's
  // logical display bounds, direct coordinate cropping rejected valid regions.
  //
  // Before the fix, a logical 200x100 display could not crop from a 100x50
  // portal image. The fix maps source bounds to image pixels before cropping.
  let cropped = crop_portal_screenshot_to_region(image, Rect::new(0.0, 0.0, 200.0, 100.0), Rect::new(40.0, 20.0, 20.0, 20.0))
    .expect("portal crop maps through source bounds");

  assert_eq!(cropped.width(), 10);
  assert_eq!(cropped.height(), 10);
  assert_eq!(*cropped.get_pixel(0, 0), image::Rgba([1, 2, 3, 4]));
}

#[test]
fn fallback_failure_preserves_the_primary_capture_error() {
  let primary = backend("screencast timed out");

  let error = capture_fallback::<()>("portal.screencast", &primary, || Err(backend("screenshot returned no URI")))
    .expect_err("both capture paths fail");

  assert_eq!(error.to_string(), "portal.screencast failed: screencast timed out; fallback capture also failed: screenshot returned no URI");
}
