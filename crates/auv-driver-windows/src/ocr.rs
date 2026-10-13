//! System OCR backed by `Windows.Media.Ocr`.
//!
//! This mirrors the macOS Vision OCR capability: callers provide a raw RGBA
//! image and receive a shared [`TextRecognition`] whose region bounds are in
//! image pixel coordinates. Mapping pixel coordinates back into screen or
//! window space stays with the caller, exactly like the macOS driver.

use auv_driver_common::geometry::Rect;
use auv_driver_common::vision::{RecognizedText, RecognizedWord, TextRecognition, TextRecognitionOptions};

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

/// One `Windows.Media.Ocr` line with its word boxes, in image pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrLine {
  pub text: String,
  pub words: Vec<RecognizedWord>,
}

/// One region per line; a line's bounds are the union of its word boxes.
pub fn text_recognition_from_lines(lines: &[OcrLine]) -> TextRecognition {
  let regions = lines
    .iter()
    .map(|line| {
      let rects = line
        .words
        .iter()
        .map(|word| RectF {
          x: word.bounds.origin.x,
          y: word.bounds.origin.y,
          width: word.bounds.size.width,
          height: word.bounds.size.height,
        })
        .collect::<Vec<_>>();
      let bounds = union_rect(&rects).unwrap_or(RectF {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
      });
      RecognizedText {
        text: line.text.clone(),
        bounds: Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
        // NOTICE: Windows.Media.Ocr does not expose recognition confidence.
        confidence: None,
      }
    })
    .collect::<Vec<_>>();
  TextRecognition {
    origin: None,
    text: lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n"),
    regions,
  }
}

/// Recognizes text in a raw RGBA image using the Windows system OCR engine.
///
/// The returned [`TextRecognition`] has one region per recognized line, with
/// bounds in image pixel coordinates. `options.recognition_languages` selects
/// the OCR language when the first tag is installed; otherwise the user
/// profile languages are used.
pub fn recognize_text_in_rgba(rgba: &[u8], width: u32, height: u32, options: &TextRecognitionOptions) -> Result<TextRecognition, OcrError> {
  Ok(text_recognition_from_lines(&recognize_lines_in_rgba(rgba, width, height, options)?))
}

/// Recognizes lines with their word boxes, in image pixel coordinates.
#[cfg(target_os = "windows")]
pub fn recognize_lines_in_rgba(rgba: &[u8], width: u32, height: u32, options: &TextRecognitionOptions) -> Result<Vec<OcrLine>, OcrError> {
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

  lines_from_result(&result)
}

/// Stub used on non-Windows targets so the crate stays cross-compilable, in the
/// same spirit as the macOS driver's non-macOS OCR stubs.
#[cfg(not(target_os = "windows"))]
pub fn recognize_lines_in_rgba(
  _rgba: &[u8],
  _width: u32,
  _height: u32,
  _options: &TextRecognitionOptions,
) -> Result<Vec<OcrLine>, OcrError> {
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
fn lines_from_result(result: &windows::Media::Ocr::OcrResult) -> Result<Vec<OcrLine>, OcrError> {
  let mut lines = Vec::new();
  for line in result.Lines().map_err(OcrError::from_winrt)? {
    let text = line.Text().map_err(OcrError::from_winrt)?.to_string();
    let mut words = Vec::new();
    for word in line.Words().map_err(OcrError::from_winrt)? {
      let rect = word.BoundingRect().map_err(OcrError::from_winrt)?;
      words.push(RecognizedWord {
        text: word.Text().map_err(OcrError::from_winrt)?.to_string(),
        bounds: Rect::new(f64::from(rect.X), f64::from(rect.Y), f64::from(rect.Width), f64::from(rect.Height)),
        // NOTICE: Windows.Media.Ocr does not expose recognition confidence.
        confidence: None,
      });
    }
    lines.push(OcrLine { text, words });
  }
  Ok(lines)
}

#[cfg(test)]
#[path = "ocr_test.rs"]
mod tests;
