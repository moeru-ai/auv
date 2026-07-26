use super::*;

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
