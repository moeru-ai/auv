use super::*;

#[test]
fn union_rect_encloses_all_word_rects() {
  let rects = [
    RectF {
      x: 10.0,
      y: 20.0,
      width: 30.0,
      height: 10.0,
    },
    RectF {
      x: 50.0,
      y: 18.0,
      width: 20.0,
      height: 14.0,
    },
  ];

  let bounds = union_rect(&rects).expect("non-empty input yields bounds");

  assert_eq!(
    bounds,
    RectF {
      x: 10.0,
      y: 18.0,
      width: 60.0,
      height: 14.0,
    }
  );
}

#[test]
fn union_rect_is_none_for_empty_input() {
  assert_eq!(union_rect(&[]), None);
}

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

// ROOT CAUSE:
//
// The Windows OCR pipeline (engine creation, BGRA bitmap construction, and
// RecognizeAsync) only runs on Windows. This smoke test exercises the full
// WinRT path on a solid-color buffer and asserts it returns Ok, which would
// catch regressions in buffer conversion or engine wiring.
#[cfg(target_os = "windows")]
#[test]
fn recognizes_solid_color_buffer_without_error() {
  let width = 64u32;
  let height = 16u32;
  let rgba = vec![255u8; (width * height * 4) as usize];

  let recognition = recognize_text_in_rgba(&rgba, width, height, &TextRecognitionOptions::default())
    .expect("windows OCR engine should process a solid-color buffer");

  // A blank image yields no readable lines; the pipeline must still succeed.
  assert_eq!(recognition.regions.len(), recognition.text.lines().count());
}
