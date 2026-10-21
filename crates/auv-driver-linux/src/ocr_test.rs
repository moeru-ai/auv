use auv_driver_common::geometry::Rect;
use std::ffi::OsString;

use super::*;

// ROOT CAUSE:
//
// If Tesseract ran on a 1x capture, small UI text was often not recognized at
// all (OSWorld X11 captures; GNOME Text Editor body text on Wayland).
// Recognition now runs on a 2x image, and TSV coordinates are scaled back so
// bounds stay in input-image pixels.
#[test]
fn upscaled_tsv_coordinates_map_back_to_input_pixels() {
  let tsv = "\
level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
5\t1\t1\t1\t1\t1\t200\t100\t60\t30\t96.0\tCart
";

  let matches = text_matches_from_tsv(tsv, "cart", 0.5);

  assert_eq!(matches.matches[0].bounds, Rect::new(100.0, 50.0, 30.0, 15.0));
  assert_eq!(text_recognition_from_tsv(tsv, 0.5).regions[0].bounds, Rect::new(100.0, 50.0, 30.0, 15.0));
}

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

  let recognition = text_recognition_from_tsv(tsv, 1.0);

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

  let matches = text_matches_from_tsv(tsv, "AGENTS.md", 1.0);

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

#[test]
fn explicit_tessdata_prefix_takes_priority_over_bundled_models() {
  let executable = std::path::Path::new("/opt/auv/auv");

  let resolved = resolve_tessdata_path(executable, Some(OsString::from("/srv/tessdata")), "eng", |_| false);

  assert_eq!(resolved, Some(std::path::PathBuf::from("/srv/tessdata")));
}

#[test]
fn discovers_requested_models_beside_the_release_executable() {
  let executable = std::path::Path::new("/opt/auv/auv");

  let resolved =
    resolve_tessdata_path(executable, None, "eng", |path| path == std::path::Path::new("/opt/auv/share/auv/tessdata/eng.traineddata"));

  assert_eq!(resolved, Some(std::path::PathBuf::from("/opt/auv/share/auv/tessdata")));
}

#[test]
fn falls_back_to_tesseract_defaults_when_bundle_lacks_a_requested_model() {
  let executable = std::path::Path::new("/opt/auv/auv");

  let resolved = resolve_tessdata_path(executable, None, "eng+chi_sim", |path| path.ends_with("eng.traineddata"));

  assert_eq!(resolved, None);
}
