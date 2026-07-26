use auv_driver::geometry::{CoordinateSpace, Rect};
use auv_driver::window::{Window, WindowRef};
use auv_game_osu::{PlayfieldProjection, ProjectionArtifact, ProjectionBounds, ProjectionDerivationMethod};

fn test_window(width: f64, height: f64) -> Window {
  Window {
    reference: WindowRef {
      id: "window-1".to_string(),
    },
    title: Some("osu!".to_string()),
    app_name: Some("osu!".to_string()),
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(100.0, 200.0, width, height),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn projection_maps_center_for_matching_aspect_ratio() {
  let window = test_window(1024.0, 768.0);
  let projection = PlayfieldProjection::for_window(&window, 4.0).expect("projection");

  let (x, y) = projection.to_window_point(256.0, 192.0);
  assert_eq!(x, 512.0);
  assert_eq!(y, 384.0);
  assert!((projection.match_radius_px - 72.96).abs() < 0.01);
}

#[test]
fn projection_letterboxes_wider_windows() {
  let window = test_window(1280.0, 720.0);
  let projection = PlayfieldProjection::for_window(&window, 4.0).expect("projection");

  let (left, top) = projection.to_window_point(0.0, 0.0);
  let (right, bottom) = projection.to_window_point(512.0, 384.0);

  assert_eq!(left, 160.0);
  assert_eq!(top, 0.0);
  assert_eq!(right, 1120.0);
  assert_eq!(bottom, 720.0);
}

#[test]
fn projection_uses_capture_dimensions_when_they_differ_from_window() {
  let projection = PlayfieldProjection::for_capture(1512.0, 949.0, 5.0).expect("projection");

  assert!((projection.scale_x - 2.4713541666666665).abs() < 1e-9);
  assert!((projection.scale_y - 2.4713541666666665).abs() < 1e-9);
  assert!((projection.offset_x - 123.33333333333337).abs() < 0.001);
  assert!((projection.offset_y - 0.0).abs() < 0.001);
  assert!((projection.match_radius_px - 79.083336).abs() < 0.01);
}

#[test]
fn projection_artifact_rejects_non_finite_values() {
  let artifact = ProjectionArtifact {
    source_window_bounds: ProjectionBounds {
      x: 0.0,
      y: 0.0,
      width: 1280.0,
      height: 720.0,
    },
    capture_bounds: None,
    capture_width: None,
    capture_height: None,
    capture_scale_factor: None,
    scale_x: f64::NAN,
    scale_y: 1.0,
    offset_x: 0.0,
    offset_y: 0.0,
    match_radius_px: 20.0,
    derivation_method: ProjectionDerivationMethod::LayoutRule,
    verification_reference: None,
  };

  let error = artifact.to_eval_projection().expect_err("must fail");
  assert!(error.contains("scale_x must be finite"));
}

#[test]
fn projection_artifact_rejects_positive_scale_that_underflows_to_zero_f32() {
  let mut artifact = sample_projection_artifact_with_capture();
  artifact.scale_x = f64::from(f32::from_bits(1)) / 2.0;

  let error = artifact.to_eval_projection().expect_err("underflowing scale must fail");
  assert!(error.contains("scale_x"));
}

#[test]
fn projection_constructor_rejects_scale_that_underflows_to_zero_f32() {
  let scale = f64::from(f32::from_bits(1)) / 2.0;
  let error = PlayfieldProjection::for_capture(512.0 * scale, 384.0 * scale, 4.0).expect_err("underflowing scale must fail");

  assert!(error.contains("scale"));
}

#[test]
fn projection_constructor_rejects_non_positive_radius() {
  let error = PlayfieldProjection::for_capture(1024.0, 768.0, 13.0).expect_err("negative radius must fail");

  assert!(error.contains("match radius"));
}

#[test]
fn projection_rejects_non_positive_window_size() {
  let window = test_window(0.0, 720.0);
  let error = PlayfieldProjection::for_window(&window, 4.0).expect_err("must fail");
  assert!(error.contains("positive finite size"));
}

fn sample_projection_artifact_with_capture() -> ProjectionArtifact {
  let projection = PlayfieldProjection::for_capture(1024.0, 768.0, 4.0).expect("projection");
  ProjectionArtifact {
    source_window_bounds: ProjectionBounds {
      x: 0.0,
      y: 0.0,
      width: 1024.0,
      height: 768.0,
    },
    capture_bounds: Some(ProjectionBounds {
      x: 0.0,
      y: 0.0,
      width: 1024.0,
      height: 768.0,
    }),
    capture_width: Some(1024),
    capture_height: Some(768),
    capture_scale_factor: Some(1.0),
    scale_x: projection.scale_x,
    scale_y: projection.scale_y,
    offset_x: projection.offset_x,
    offset_y: projection.offset_y,
    match_radius_px: projection.match_radius_px,
    derivation_method: ProjectionDerivationMethod::LayoutRule,
    verification_reference: None,
  }
}
