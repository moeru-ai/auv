//! System OCR backed by `Windows.Media.Ocr`.
//!
//! This mirrors the macOS Vision OCR capability: callers provide a raw RGBA
//! image and receive a shared [`TextRecognition`] whose region bounds are in
//! image pixel coordinates. Mapping pixel coordinates back into screen or
//! window space stays with the caller, exactly like the macOS driver.

#[cfg(target_os = "windows")]
use auv_driver::geometry::Rect;
#[cfg(target_os = "windows")]
use auv_driver::vision::RecognizedText;
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};

/// Errors raised while running Windows system OCR.
#[derive(Debug, thiserror::Error)]
pub enum OcrError {
  /// The current build target does not provide `Windows.Media.Ocr`.
  #[error("windows OCR is unsupported on this target")]
  Unsupported,
  /// The supplied buffer length did not match `width * height * 4`.
  #[error("image buffer length {actual} did not match expected {expected} (width*height*4)")]
  InvalidImage { expected: usize, actual: usize },
  /// The image dimensions could not be represented for the OCR engine.
  #[error("image dimensions exceed the supported range")]
  ImageTooLarge,
  /// A `Windows.Media.Ocr` runtime call failed.
  #[error("windows OCR runtime error: {0}")]
  Runtime(String),
}

/// Axis-aligned rectangle in image pixel space.
///
/// Kept host-independent so the line-bounds union logic can be unit tested
/// without a live WinRT OCR engine.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RectF {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
}

/// Computes the bounding rectangle that encloses every input rectangle.
///
/// `Windows.Media.Ocr` exposes a bounding rect per word but not per line, so a
/// line's bounds are derived from the union of its word rects.
fn union_rect(rects: &[RectF]) -> Option<RectF> {
  let mut iter = rects.iter().copied();
  let first = iter.next()?;
  let mut min_x = first.x;
  let mut min_y = first.y;
  let mut max_x = first.x + first.width;
  let mut max_y = first.y + first.height;
  for rect in iter {
    min_x = min_x.min(rect.x);
    min_y = min_y.min(rect.y);
    max_x = max_x.max(rect.x + rect.width);
    max_y = max_y.max(rect.y + rect.height);
  }
  Some(RectF {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  })
}

/// Recognizes text in a raw RGBA image using the Windows system OCR engine.
///
/// The returned [`TextRecognition`] has one region per recognized line, with
/// bounds in image pixel coordinates. `options.recognition_languages` selects
/// the OCR language when the first tag is installed; otherwise the user
/// profile languages are used.
#[cfg(target_os = "windows")]
pub fn recognize_text_in_rgba(rgba: &[u8], width: u32, height: u32, options: &TextRecognitionOptions) -> Result<TextRecognition, OcrError> {
  use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
  use windows::Security::Cryptography::CryptographicBuffer;

  let expected = (width as usize) * (height as usize) * 4;
  if rgba.len() != expected {
    return Err(OcrError::InvalidImage {
      expected,
      actual: rgba.len(),
    });
  }
  let width_i32 = i32::try_from(width).map_err(|_| OcrError::ImageTooLarge)?;
  let height_i32 = i32::try_from(height).map_err(|_| OcrError::ImageTooLarge)?;

  let engine = create_engine(options)?;

  // Windows OCR consumes BGRA8; the public API takes RGBA to match the macOS
  // driver, so swap the red/blue channels before handing pixels to WinRT.
  let bgra = rgba_to_bgra(rgba);
  let buffer = CryptographicBuffer::CreateFromByteArray(&bgra).map_err(OcrError::from_winrt)?;
  let bitmap =
    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width_i32, height_i32).map_err(OcrError::from_winrt)?;

  let result = engine.RecognizeAsync(&bitmap).map_err(OcrError::from_winrt)?.get().map_err(OcrError::from_winrt)?;

  text_recognition_from_result(&result)
}

/// Stub used on non-Windows targets so the crate stays cross-compilable, in the
/// same spirit as the macOS driver's non-macOS OCR stubs.
#[cfg(not(target_os = "windows"))]
pub fn recognize_text_in_rgba(
  _rgba: &[u8],
  _width: u32,
  _height: u32,
  _options: &TextRecognitionOptions,
) -> Result<TextRecognition, OcrError> {
  Err(OcrError::Unsupported)
}

#[cfg(target_os = "windows")]
impl OcrError {
  fn from_winrt(error: windows::core::Error) -> Self {
    OcrError::Runtime(error.to_string())
  }
}

#[cfg(target_os = "windows")]
fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
  let mut bgra = rgba.to_vec();
  for pixel in bgra.chunks_exact_mut(4) {
    pixel.swap(0, 2);
  }
  bgra
}

#[cfg(target_os = "windows")]
fn create_engine(options: &TextRecognitionOptions) -> Result<windows::Media::Ocr::OcrEngine, OcrError> {
  use windows::Globalization::Language;
  use windows::Media::Ocr::OcrEngine;
  use windows::core::HSTRING;

  // NOTICE: Windows.Media.Ocr cannot be biased toward custom vocabulary, so
  // `TextRecognitionOptions::custom_words` is intentionally ignored here.
  // TODO(windows-ocr): expose custom-word weighting if a future engine adds it.
  if let Some(tag) = options.recognition_languages.as_ref().and_then(|languages| languages.first()) {
    let language = Language::CreateLanguage(&HSTRING::from(tag.as_str())).map_err(OcrError::from_winrt)?;
    if OcrEngine::IsLanguageSupported(&language).map_err(OcrError::from_winrt)? {
      return OcrEngine::TryCreateFromLanguage(&language).map_err(OcrError::from_winrt);
    }
  }
  // NOTICE: a null engine here means no OCR language pack is installed; the
  // failure then surfaces from RecognizeAsync as a runtime error.
  OcrEngine::TryCreateFromUserProfileLanguages().map_err(OcrError::from_winrt)
}

#[cfg(target_os = "windows")]
fn text_recognition_from_result(result: &windows::Media::Ocr::OcrResult) -> Result<TextRecognition, OcrError> {
  let mut regions = Vec::new();
  let mut line_texts = Vec::new();
  for line in result.Lines().map_err(OcrError::from_winrt)? {
    let text = line.Text().map_err(OcrError::from_winrt)?.to_string();
    let mut word_rects = Vec::new();
    for word in line.Words().map_err(OcrError::from_winrt)? {
      let rect = word.BoundingRect().map_err(OcrError::from_winrt)?;
      word_rects.push(RectF {
        x: f64::from(rect.X),
        y: f64::from(rect.Y),
        width: f64::from(rect.Width),
        height: f64::from(rect.Height),
      });
    }
    let bounds = union_rect(&word_rects).unwrap_or(RectF {
      x: 0.0,
      y: 0.0,
      width: 0.0,
      height: 0.0,
    });
    line_texts.push(text.clone());
    regions.push(RecognizedText {
      text,
      bounds: Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
      // NOTICE: Windows.Media.Ocr does not expose recognition confidence.
      confidence: None,
    });
  }
  Ok(TextRecognition {
    text: line_texts.join("\n"),
    regions,
  })
}

#[cfg(test)]
mod tests {
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
}
