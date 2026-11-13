use auv_driver_capture_common::Capture;
use auv_driver_common::vision::{RecognizedText, TextRecognition};
use auv_driver_common::{CoordinateSpace, Point, Position, Positional, Positioned, Rect, WindowPoint, WindowRef};

fn window_capture() -> Capture {
  Capture {
    origin: Some(Position::in_window(
      &WindowRef {
        id: "observed-window".into(),
      },
      WindowPoint::new(0.0, 0.0),
    )),
    image: image::RgbaImage::new(800, 600),
    bounds: Rect::new(-200.0, 300.0, 400.0, 300.0),
    scale_factor: 2.0,
    backend: "geometry-test".into(),
    fallback_reason: None,
  }
}

fn observed_text(capture: &Capture) -> TextRecognition {
  TextRecognition {
    origin: capture.recognition_origin(),
    text: "target".into(),
    regions: vec![RecognizedText {
      text: "target".into(),
      bounds: Rect::new(-180.5, 320.25, 40.0, 18.0),
      confidence: Some(0.9),
    }],
  }
}

#[test]
fn capture_rebasing_preserves_target_identity_and_is_idempotent() {
  let capture = window_capture();
  let source = observed_text(&capture);
  let before = source.positioned_regions().unwrap().next().unwrap();
  let recognition = source.relative_to(&capture).unwrap();
  assert_eq!(recognition.clone().relative_to(&capture).unwrap(), recognition);
  let target = recognition.positioned_regions().unwrap().next().unwrap();
  let decoded: Positioned<RecognizedText> = serde_json::from_str(&serde_json::to_string(&target).unwrap()).unwrap();
  assert_eq!(decoded.position().unwrap(), before.position().unwrap());
  assert_eq!(decoded.position().unwrap().coordinate_space, CoordinateSpace::Window("observed-window".into()));
  assert_eq!(decoded.position().unwrap().point, Point::new(39.5, 29.25));
  assert_eq!(decoded.value.bounds, Rect::new(19.5, 20.25, 40.0, 18.0));
  assert_eq!(decoded.value.confidence, Some(0.9));
}

#[test]
fn nonzero_capture_origin_and_other_positional_anchors_preserve_action_point() {
  let mut capture = window_capture();
  capture.origin.as_mut().unwrap().point = Point::new(50.0, 60.0);
  let recognition = observed_text(&capture).relative_to(&capture).unwrap();
  let target = recognition.positioned_regions().unwrap().next().unwrap();
  assert_eq!(target.position().unwrap().point, Point::new(89.5, 89.25));
  let rebased = recognition.relative_to(&target).unwrap();
  assert_eq!(rebased.regions[0].bounds.center(), Point::new(0.0, 0.0));
  assert_eq!(rebased.positioned_regions().unwrap().next().unwrap().position, target.position);
}

#[test]
fn rebasing_rejects_unbound_and_cross_space_origins() {
  let capture = window_capture();
  let recognition = observed_text(&capture);
  for coordinate_space in [
    CoordinateSpace::Screen,
    CoordinateSpace::Display("display".into()),
    CoordinateSpace::Window("other-window".into()),
  ] {
    assert!(
      recognition
        .clone()
        .relative_to(&Position {
          point: Point::new(0.0, 0.0),
          coordinate_space
        })
        .is_err()
    );
  }
  let mut detached = capture.clone();
  detached.origin = None;
  assert!(recognition.clone().relative_to(&detached).is_err());
  let unbound = observed_text(&detached);
  assert!(unbound.clone().relative_to(&capture).is_err());
  assert!(unbound.positioned_regions().is_err());
  let older: TextRecognition = serde_json::from_str(r#"{"text":"","regions":[]}"#).unwrap();
  assert!(older.origin.is_none());
  let mut invalid = capture.position().unwrap();
  invalid.point.x = f64::NAN;
  assert!(recognition.relative_to(&invalid).is_err());
}
