use super::*;

#[test]
fn window_text_target_preserves_identity_and_logical_coordinates_through_serialization() {
  use crate::geometry::{CoordinateSpace, Positional};

  // OCR already converts image pixels to logical coordinates. Changing origin
  // must preserve extent and confidence, rather than scaling them a second time.
  let recognition = TextRecognition {
    text: "target".into(),
    regions: vec![RecognizedText {
      text: "target".into(),
      bounds: Rect::new(-180.5, 320.25, 40.0, 18.0),
      confidence: Some(0.9),
    }],
  }
  .relative_to(Point::new(-200.0, 300.0));
  let window = WindowRef {
    id: "observed-window".into(),
  };
  let target = recognition.regions.into_iter().next().unwrap().in_window(&window);
  let encoded = serde_json::to_string(&target).unwrap();
  let decoded: Positioned<RecognizedText> = serde_json::from_str(&encoded).unwrap();

  assert_eq!(decoded.position().coordinate_space, CoordinateSpace::Window(window.id));
  assert_eq!(decoded.position().point, Point::new(39.5, 29.25));
  assert_eq!(decoded.value.bounds, Rect::new(19.5, 20.25, 40.0, 18.0));
  assert_eq!(decoded.value.confidence, Some(0.9));
  assert_eq!(decoded.value.text, "target");
}

#[test]
fn text_recognition_finds_case_insensitive_contains_match() {
  let recognition = TextRecognition {
    text: "Cure For Me\nAURORA".to_string(),
    regions: vec![
      RecognizedText {
        text: "Cure For Me".to_string(),
        bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
        confidence: Some(0.9),
      },
      RecognizedText {
        text: "AURORA".to_string(),
        bounds: Rect::new(50.0, 60.0, 70.0, 80.0),
        confidence: Some(0.8),
      },
    ],
  };

  let matched = recognition.best_contains("cure for").expect("text should match");

  assert_eq!(matched.text, "Cure For Me");
  assert_eq!(matched.action_point(), Point::new(25.0, 40.0));
}

#[test]
fn text_recognition_options_preserve_provider_hints() {
  let options = TextRecognitionOptions::default().with_custom_words(["绚香", "AURORA"]).with_recognition_languages(["zh-Hans", "en-US"]);

  assert_eq!(options.custom_words, vec!["绚香", "AURORA"]);
  assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
}

#[test]
fn ocr_matches_share_action_point_and_best_match_contract() {
  let matches = OcrMatches {
    matches: vec![OcrMatch {
      text: "Play".to_string(),
      confidence: 0.92,
      bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
    }],
  };

  let matched = matches.best_match().expect("one match");

  assert_eq!(matched.action_point(), Point::new(25.0, 40.0));
}
