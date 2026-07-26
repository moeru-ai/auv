use auv_driver_common::geometry::Rect;

use super::*;

#[test]
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

#[cfg(target_os = "linux")]
#[test]
fn maps_common_language_tags_to_tesseract_ids() {
  let options = TextRecognitionOptions::default().with_recognition_languages(["en-US", "zh-Hans"]);

  assert_eq!(tesseract_language(&options), "eng+chi_sim");
}
