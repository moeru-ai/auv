use super::*;

#[test]
fn text_recognition_finds_case_insensitive_contains_match() {
  let recognition = TextRecognition {
    origin: None,
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

// ROOT CAUSE:
//
// If the query and the OCR text spell punctuation differently, find_contains
// missed visible text because it only lowercased both sides. Vision on macOS
// reports `“Report — Final”` as `"Report - Final"`, so a query copied from the
// screen with typographic punctuation found nothing; Tesseract splits
// `Huashan.jpg` into `Huashan. jpg`.
//
// The fix folds quotes, dashes, whitespace and the OCR spacing around `.`/`,`
// on both sides before matching, without joining sentence or list boundaries.
#[test]
fn find_contains_matches_across_ocr_punctuation_variants() {
  let recognition = TextRecognition {
    origin: None,
    text: String::new(),
    regions: [
      "Open \"Report - Final\"",
      "File: Huashan. jpg",
      "Search for ‘youtube’ now",
    ]
    .into_iter()
    .map(|text| RecognizedText {
      text: text.into(),
      bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
      confidence: None,
    })
    .collect(),
  };
  for query in ["“Report — Final”", "huashan.jpg", "'youtube'"] {
    assert_eq!(recognition.find_contains(query).len(), 1, "{query}");
  }
  assert!(recognition.find_contains("  ").is_empty());
}

#[test]
fn normalize_match_text_keeps_sentence_and_list_boundaries() {
  assert_eq!(normalize_match_text("File: Huashan. jpg"), "file: huashan.jpg");
  assert_eq!(normalize_match_text("Open  “Report — Final” ,now"), "open \"report - final\",now");
  assert_eq!(normalize_match_text("For example. Commands"), "for example. commands");
  assert_eq!(normalize_match_text("Chapter 1. 5 items"), "chapter 1. 5 items");
  assert_eq!(normalize_match_text("a, b"), "a, b");
  assert_eq!(normalize_match_text("设 置 中心"), "设置中心");
  assert_eq!(normalize_match_text("第 1 页 ， 共 12 首"), "第1页，共12首");
  assert_eq!(normalize_match_text("설정 열기"), "설정 열기");
  assert_eq!(normalize_match_text("打开 File"), "打开 file");
}

#[test]
fn word_spans_bound_a_match_by_its_words_and_join_cjk_glyphs() {
  let word = |text: &str, x: f64| RecognizedWord {
    text: text.into(),
    bounds: Rect::new(x, 0.0, 10.0, 10.0),
    confidence: Some(0.5),
  };
  let menu = [word("File", 0.0), word("Edit", 20.0), word("View", 40.0)];
  let cjk = [word("设", 0.0), word("置", 12.0)];
  let matches = match_word_spans([&menu[..], &cjk[..]], "edit");
  assert_eq!(matches.matches.len(), 1);
  assert_eq!(matches.matches[0].bounds, Rect::new(20.0, 0.0, 10.0, 10.0));
  let matches = match_word_spans([&menu[..], &cjk[..]], "设置");
  assert_eq!(matches.matches.len(), 1);
  assert_eq!(matches.matches[0].bounds, Rect::new(0.0, 0.0, 22.0, 10.0));
  // Confidence averages the words that report one.
  assert_eq!(matches.matches[0].confidence, 0.5);
  let unrated = [RecognizedWord {
    confidence: None,
    ..word("Edit", 0.0)
  }];
  assert_eq!(match_word_spans([&unrated[..]], "edit").matches[0].confidence, 0.0);
}

#[test]
fn word_spans_keep_the_narrower_of_nested_matches() {
  let word = |text: &str, x: f64, width: f64| RecognizedWord {
    text: text.into(),
    bounds: Rect::new(x, 0.0, width, 10.0),
    confidence: None,
  };
  // "Save" matches alone and, from the earlier start, as "Save As"'s prefix
  // span; the wider span that contains the word-only match is dropped.
  let line = [word("Save", 0.0, 20.0), word("As", 24.0, 10.0)];
  let matches = match_word_spans([&line[..]], "save");
  assert_eq!(matches.matches.len(), 1);
  assert_eq!(matches.matches[0].bounds, Rect::new(0.0, 0.0, 20.0, 10.0));
}

#[test]
fn multi_word_continuous_span_unions_bounds_and_computes_exact_action_point() {
  let word = |text: &str, x: f64, width: f64| RecognizedWord {
    text: text.into(),
    bounds: Rect::new(x, 10.0, width, 20.0),
    confidence: Some(0.95),
  };
  let line = [
    word("Quick", 0.0, 40.0),
    word("Brown", 45.0, 45.0),
    word("Fox", 95.0, 30.0),
    word("Jumps", 130.0, 45.0),
  ];

  // 1. Two-word continuous span: "Brown Fox"
  let matches2 = match_word_spans([&line[..]], "brown fox");
  assert_eq!(matches2.matches.len(), 1);
  assert_eq!(matches2.matches[0].text, "Brown Fox");
  // Span union: [45.0..125.0] (width 80.0), height 20.0
  assert_eq!(matches2.matches[0].bounds, Rect::new(45.0, 10.0, 80.0, 20.0));
  assert_eq!(matches2.matches[0].action_point(), Point::new(85.0, 20.0));

  // 2. Three-word continuous span: "Quick Brown Fox"
  let matches3 = match_word_spans([&line[..]], "quick brown fox");
  assert_eq!(matches3.matches.len(), 1);
  assert_eq!(matches3.matches[0].text, "Quick Brown Fox");
  // Span union: [0.0..125.0] (width 125.0), height 20.0
  assert_eq!(matches3.matches[0].bounds, Rect::new(0.0, 10.0, 125.0, 20.0));
  assert_eq!(matches3.matches[0].action_point(), Point::new(62.5, 20.0));

  // The span action point is distinctly centered on the sub-phrase, NOT the entire line (which ends at 175.0, center 87.5)
  assert_ne!(matches3.matches[0].action_point().x, 87.5);
}
