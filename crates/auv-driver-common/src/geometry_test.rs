use super::*;

#[test]
fn projection_basis_serializes_generic_provenance() {
  let basis = ProjectionBasis::new(
    "basis-frame-1",
    1_000,
    ProjectionSourceSpace::World,
    CoordinateSpace::Window("window-1".to_string()),
    ProjectionDerivationFamily::CameraMatrix,
  )
  .with_confidence(0.75)
  .with_match_radius_px(12.0);

  let value = serde_json::to_value(&basis).expect("serialize projection basis");

  assert_eq!(value["basis_id"], serde_json::json!("basis-frame-1"));
  assert_eq!(value["source_space"]["kind"], serde_json::json!("world"));
  assert_eq!(value["derivation_family"], serde_json::json!("camera_matrix"));
  assert_eq!(value["match_radius_px"], serde_json::json!(12.0));
}

#[test]
fn relative_rect_is_only_inside_the_unit_square() {
  assert!(RelativeRect::new(0.0, 0.0, 1.0, 1.0).is_normalized());
  assert!(RelativeRect::new(0.25, 0.1, 0.5, 0.8).is_normalized());
  for rect in [
    RelativeRect::new(0.5, 0.0, 0.6, 1.0),
    RelativeRect::new(-0.1, 0.0, 0.5, 0.5),
    RelativeRect::new(0.0, 0.0, 0.0, 0.5),
    RelativeRect::new(0.0, 0.0, f64::NAN, 0.5),
  ] {
    assert!(!rect.is_normalized(), "{rect:?}");
  }
}

#[test]
fn rect_helpers_build_strips_insets_and_points_in_the_same_space() {
  let frame = Rect::new(100.0, 50.0, 800.0, 600.0);
  assert_eq!(frame.below(40.0, 10.0), Rect::new(100.0, 660.0, 800.0, 40.0));
  assert_eq!(frame.above(30.0, 0.0), Rect::new(100.0, 20.0, 800.0, 30.0));
  assert_eq!(frame.left_of(20.0, 5.0), Rect::new(75.0, 50.0, 20.0, 600.0));
  assert_eq!(frame.right_of(20.0, 5.0), Rect::new(905.0, 50.0, 20.0, 600.0));
  assert_eq!(frame.inset(10.0), Rect::new(110.0, 60.0, 780.0, 580.0));
  assert_eq!(
    frame.inset(Insets {
      top: 34.0,
      ..Default::default()
    }),
    Rect::new(100.0, 84.0, 800.0, 566.0)
  );
  assert_eq!(frame.offset(-100.0, 10.0), Rect::new(0.0, 60.0, 800.0, 600.0));
  assert_eq!(frame.at(0.5, 0.5), frame.center());
  assert!(frame.contains_point(Point::new(900.0, 650.0)));
  assert!(!frame.contains_rect(Rect::new(890.0, 60.0, 20.0, 10.0)));
  assert_eq!(frame.intersect(Rect::new(850.0, 0.0, 100.0, 100.0)), Some(Rect::new(850.0, 50.0, 50.0, 50.0)));
  assert_eq!(frame.intersect(Rect::new(900.0, 50.0, 10.0, 10.0)), None);
}

#[test]
fn region_takes_two_of_start_end_and_size_per_axis_with_percentages() {
  let frame = Rect::new(100.0, 50.0, 800.0, 600.0);
  // Top 10% down, 80 points tall; 20 points in from left and right.
  let region = frame
    .region(Edges {
      top: Some(Length::Percent(10.0)),
      height: Some(80.0.into()),
      left: Some(20.0.into()),
      right: Some(20.0.into()),
      ..Default::default()
    })
    .expect("two edges per axis");
  assert_eq!(region, Rect::new(120.0, 110.0, 760.0, 80.0));
  // A missing start with an end and a size sits against the end.
  let bottom = frame
    .region(Edges {
      bottom: Some(0.0.into()),
      height: Some(Length::Percent(25.0)),
      ..Default::default()
    })
    .unwrap();
  assert_eq!(bottom, Rect::new(100.0, 500.0, 800.0, 150.0));
  // Nothing given fills the rectangle.
  assert_eq!(frame.region(Edges::default()).unwrap(), frame);
  let over = frame.region(Edges {
    left: Some(0.0.into()),
    right: Some(0.0.into()),
    width: Some(10.0.into()),
    ..Default::default()
  });
  assert!(over.is_err(), "all three on one axis over-constrain it");
}

// ROOT CAUSE:
//
// If screen.captureRegion received an OCR box (385.5, 152.5, 160.5 x 13.5),
// the macOS and Windows capture backends failed with "region x must be an
// integer in backend capture units", because they address whole units.
//
// The fix covers the region with whole units instead of rejecting it.
#[test]
fn round_out_covers_a_fractional_rect_with_whole_units() {
  assert_eq!(Rect::new(385.5, 152.5, 160.5, 13.5).round_out(), Rect::new(385.0, 152.0, 161.0, 14.0));
  assert_eq!(Rect::new(-0.25, 10.0, 0.5, 2.0).round_out(), Rect::new(-1.0, 10.0, 2.0, 2.0));
  assert_eq!(Rect::new(4.0, 6.0, 8.0, 2.0).round_out(), Rect::new(4.0, 6.0, 8.0, 2.0));
}

#[test]
fn union_encloses_both_rects() {
  assert_eq!(Rect::new(10.0, 20.0, 30.0, 10.0).union(Rect::new(50.0, 18.0, 20.0, 14.0)), Rect::new(10.0, 18.0, 60.0, 14.0));
}

#[test]
fn rect_contains_point_boundary_conditions_and_out_of_bounds() {
  let rect = Rect::new(50.0, 100.0, 400.0, 300.0);

  // Exact 4 corners are inclusive
  assert!(rect.contains_point(Point::new(50.0, 100.0))); // top-left
  assert!(rect.contains_point(Point::new(450.0, 100.0))); // top-right
  assert!(rect.contains_point(Point::new(50.0, 400.0))); // bottom-left
  assert!(rect.contains_point(Point::new(450.0, 400.0))); // bottom-right

  // Exact 4 edges are inclusive
  assert!(rect.contains_point(Point::new(50.0, 250.0))); // left edge
  assert!(rect.contains_point(Point::new(450.0, 250.0))); // right edge
  assert!(rect.contains_point(Point::new(250.0, 100.0))); // top edge
  assert!(rect.contains_point(Point::new(250.0, 400.0))); // bottom edge

  // Epsilon outside edges must be rejected
  assert!(!rect.contains_point(Point::new(49.999, 250.0)));
  assert!(!rect.contains_point(Point::new(450.001, 250.0)));
  assert!(!rect.contains_point(Point::new(250.0, 99.999)));
  assert!(!rect.contains_point(Point::new(250.0, 400.001)));

  // Negative coordinates and distant coordinates must be rejected
  assert!(!rect.contains_point(Point::new(-50.0, 200.0)));
  assert!(!rect.contains_point(Point::new(200.0, -100.0)));
  assert!(!rect.contains_point(Point::new(10_000.0, 10_000.0)));
}

#[test]
fn point_typed_wrappers_conversions_and_invariance() {
  let raw = Point::new(123.45, 678.9);

  let sp = ScreenPoint::from(raw);
  assert_eq!(sp.point(), raw);
  assert_eq!(Point::from(sp), raw);

  let wp = WindowPoint::from(raw);
  assert_eq!(wp.point(), raw);
  assert_eq!(Point::from(wp), raw);
}

#[test]
fn relative_rect_boundary_and_non_finite_rejection() {
  // Boundaries of unit square
  assert!(RelativeRect::new(0.0, 0.0, 1.0, 1.0).is_normalized());
  assert!(RelativeRect::new(0.5, 0.5, 0.5, 0.5).is_normalized());

  // Non-finite coordinates must be rejected
  assert!(!RelativeRect::new(f64::NAN, 0.0, 0.5, 0.5).is_normalized());
  assert!(!RelativeRect::new(0.0, f64::INFINITY, 0.5, 0.5).is_normalized());
  assert!(!RelativeRect::new(0.0, 0.0, f64::NEG_INFINITY, 0.5).is_normalized());

  // Zero and negative dimensions must be rejected
  assert!(!RelativeRect::new(0.0, 0.0, 0.0, 1.0).is_normalized());
  assert!(!RelativeRect::new(0.0, 0.0, 1.0, 0.0).is_normalized());
  assert!(!RelativeRect::new(0.0, 0.0, -0.5, 0.5).is_normalized());

  // Sum exceeding 1.0 must be rejected
  assert!(!RelativeRect::new(0.8, 0.0, 0.3, 0.5).is_normalized());
  assert!(!RelativeRect::new(0.0, 0.7, 0.5, 0.4).is_normalized());
}
