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
