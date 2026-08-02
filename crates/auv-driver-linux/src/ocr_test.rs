use auv_driver_common::geometry::Rect;

use super::*;

#[test]
#[cfg(target_os = "linux")]
fn rejects_buffer_with_mismatched_length() {
  let result = recognize_text_in_rgba(&[0u8; 7], 2, 2, &TextRecognitionOptions::default());

  match result {
    Err(OcrError::InvalidImage { expected, actual }) => {
      assert_eq!(expected, 16);
      assert_eq!(actual, 7);
    }
    other => panic!("expected InvalidImage error, got {other:?}"),
  }
}

#[test]
fn parses_tesseract_tsv_words_into_line_regions() {
  let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
5\t1\t1\t1\t1\t1\t10\t20\t30\t10\t91.5\tHello
5\t1\t1\t1\t1\t2\t50\t18\t20\t14\t84.5\tWorld
5\t1\t1\t1\t2\t1\t8\t42\t15\t9\t-1\tIgnoredConfidence
";

  let recognition = text_recognition_from_tsv(tsv);

  assert_eq!(recognition.text, "Hello World\nIgnoredConfidence");
  assert_eq!(recognition.regions.len(), 2);
  assert_eq!(recognition.regions[0].text, "Hello World");
  assert_eq!(recognition.regions[0].bounds, Rect::new(10.0, 18.0, 60.0, 14.0));
  assert_eq!(recognition.regions[0].confidence, Some(0.88));
  assert_eq!(recognition.regions[1].confidence, None);
}

#[test]
fn query_match_uses_word_span_instead_of_entire_tesseract_line() {
  // ROOT CAUSE:
  //
  // If Tesseract placed sidebar text and editor text on one TSV line, AUV
  // merged every word into one region and clicked the center of that entire
  // line instead of the queried word.
  //
  // Before the fix, `AGENTS.md` inherited a 2486-pixel-wide line bound. The fix
  // retains the minimal contiguous word span that contains the query.
  let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
5\t1\t1\t1\t1\t1\t74\t705\t20\t33\t20.0\t(@
5\t1\t1\t1\t1\t2\t101\t705\t70\t33\t94.0\tAGENTS.md
5\t1\t1\t1\t1\t3\t400\t705\t2160\t33\t80.0\tREADME-content
";

  let matches = text_matches_from_tsv(tsv, "AGENTS.md");

  assert_eq!(matches.matches.len(), 1);
  assert_eq!(matches.matches[0].text, "AGENTS.md");
  assert_eq!(matches.matches[0].bounds, Rect::new(101.0, 705.0, 70.0, 33.0));
  assert!((matches.matches[0].confidence - 0.94).abs() < 0.000_001);
}

#[cfg(target_os = "linux")]
#[test]
fn maps_common_language_tags_to_tesseract_ids() {
  let options = TextRecognitionOptions::default().with_recognition_languages(["en-US", "zh-Hans"]);

  assert_eq!(tesseract_language(&options), "eng+chi_sim");
}
