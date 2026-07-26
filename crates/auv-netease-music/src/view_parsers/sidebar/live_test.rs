use super::*;

#[test]
fn sidebar_ls_scan_ocr_options_merges_query_into_custom_words() {
  let base = TextRecognitionOptions::default().with_custom_words(["绚香"]);
  let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

  assert_eq!(options.custom_words, vec!["绚香".to_string(), "3".to_string()]);
}

#[test]
fn sidebar_ls_scan_ocr_options_leaves_base_untouched_without_query() {
  let base = TextRecognitionOptions::default().with_custom_words(["绚香"]);
  let options = sidebar_ls_scan_ocr_options(&base, None);

  assert_eq!(options, base);
}

#[test]
fn sidebar_ls_scan_ocr_options_sets_default_languages_for_single_digit_query() {
  let base = TextRecognitionOptions::default();
  let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

  assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
}

#[test]
fn sidebar_ls_scan_ocr_options_leaves_languages_for_non_numeric_query() {
  let base = TextRecognitionOptions::default();
  let options = sidebar_ls_scan_ocr_options(&base, Some("My Playlist"));

  assert_eq!(options.recognition_languages, None);
}

#[test]
fn sidebar_ls_scan_ocr_options_preserves_caller_recognition_languages() {
  let base = TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
  let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

  assert_eq!(options.recognition_languages, Some(vec!["ja-JP".to_string()]));
}
