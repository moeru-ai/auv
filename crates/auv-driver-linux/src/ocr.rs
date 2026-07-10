//! System OCR backed by Tesseract through `leptess`.
//!
//! This mirrors the Windows OCR module: callers provide a raw RGBA image and
//! receive shared [`TextRecognition`] records whose bounds are in image-pixel
//! coordinates. Capture-region cropping and coordinate projection belong to the
//! `vision` module.

use std::fmt;

use auv_driver_common::geometry::Rect;
use auv_driver_common::vision::{RecognizedText, TextRecognition, TextRecognitionOptions};
#[cfg(target_os = "linux")]
use image::ImageEncoder;

#[derive(Debug)]
pub enum OcrError {
  Unsupported,
  InvalidImage { expected: usize, actual: usize },
  ImageTooLarge,
  Runtime(String),
}

impl fmt::Display for OcrError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Unsupported => write!(f, "linux OCR is unsupported on this target"),
      Self::InvalidImage { expected, actual } => {
        write!(f, "image buffer length {actual} did not match expected {expected} (width*height*4)")
      }
      Self::ImageTooLarge => write!(f, "image dimensions exceed the supported range"),
      Self::Runtime(message) => write!(f, "linux OCR runtime error: {message}"),
    }
  }
}

impl std::error::Error for OcrError {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Word {
  left: f64,
  top: f64,
  width: f64,
  height: f64,
  confidence: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Line {
  text: String,
  words: Vec<Word>,
}

/// Recognizes text in a raw RGBA image using Tesseract.
///
/// The returned [`TextRecognition`] has one region per recognized text line,
/// with bounds in image-pixel coordinates. `recognition_languages` are mapped
/// to Tesseract language ids and joined with `+`; the default is `eng`.
#[cfg(target_os = "linux")]
pub fn recognize_text_in_rgba(rgba: &[u8], width: u32, height: u32, options: &TextRecognitionOptions) -> Result<TextRecognition, OcrError> {
  let expected = (width as usize) * (height as usize) * 4;
  if rgba.len() != expected {
    return Err(OcrError::InvalidImage {
      expected,
      actual: rgba.len(),
    });
  }
  if i32::try_from(width).is_err() || i32::try_from(height).is_err() {
    return Err(OcrError::ImageTooLarge);
  }

  let mut png = Vec::new();
  image::codecs::png::PngEncoder::new(&mut png)
    .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
    .map_err(|error| OcrError::Runtime(format!("failed to encode RGBA image as PNG: {error}")))?;

  let language = tesseract_language(options);
  let mut tess =
    leptess::LepTess::new(None, &language).map_err(|error| OcrError::Runtime(format!("failed to initialize Tesseract: {error}")))?;

  // NOTICE(linux-tesseract-custom-words): leptess exposes Tesseract variables
  // but not a stable cross-version user-word injection surface. Custom word
  // weighting is deferred until an owner-approved OCR-quality slice needs it.
  tess.set_image_from_mem(&png).map_err(|error| OcrError::Runtime(format!("failed to load image into Tesseract: {error}")))?;
  tess.set_source_resolution(144);
  let tsv = tess.get_tsv_text(0).map_err(|error| OcrError::Runtime(format!("failed to read Tesseract TSV: {error}")))?;

  Ok(text_recognition_from_tsv(&tsv))
}

#[cfg(not(target_os = "linux"))]
pub fn recognize_text_in_rgba(
  _rgba: &[u8],
  _width: u32,
  _height: u32,
  _options: &TextRecognitionOptions,
) -> Result<TextRecognition, OcrError> {
  Err(OcrError::Unsupported)
}

#[cfg(target_os = "linux")]
fn tesseract_language(options: &TextRecognitionOptions) -> String {
  options
    .recognition_languages
    .as_ref()
    .map(|languages| languages.iter().map(|language| tesseract_language_tag(language)).collect::<Vec<_>>().join("+"))
    .filter(|language| !language.is_empty())
    .unwrap_or_else(|| "eng".to_string())
}

#[cfg(target_os = "linux")]
fn tesseract_language_tag(language: &str) -> String {
  match language {
    "en" | "en-US" | "en_US" | "eng" => "eng".to_string(),
    "zh" | "zh-CN" | "zh-Hans" | "zh_CN" | "chi_sim" => "chi_sim".to_string(),
    "zh-TW" | "zh-Hant" | "zh_TW" | "chi_tra" => "chi_tra".to_string(),
    other => other.replace('-', "_"),
  }
}

fn text_recognition_from_tsv(tsv: &str) -> TextRecognition {
  let lines = parse_tsv_lines(tsv);
  let regions = lines
    .into_iter()
    .filter_map(|line| {
      let bounds = union_words(&line.words)?;
      Some(RecognizedText {
        text: line.text,
        bounds,
        confidence: mean_confidence(&line.words),
      })
    })
    .collect::<Vec<_>>();
  TextRecognition {
    text: regions.iter().map(|region| region.text.as_str()).collect::<Vec<_>>().join("\n"),
    regions,
  }
}

fn parse_tsv_lines(tsv: &str) -> Vec<Line> {
  let mut lines = Vec::<((String, String, String), Line)>::new();
  for row in tsv.lines().skip(1) {
    let columns = row.split('\t').collect::<Vec<_>>();
    if columns.len() < 12 || columns[0] != "5" {
      continue;
    }
    let text = columns[11..].join("\t").trim().to_string();
    if text.is_empty() {
      continue;
    }
    let Some(word) = parse_word(&columns) else {
      continue;
    };
    let key = (columns[2].to_string(), columns[3].to_string(), columns[4].to_string());
    if let Some((_, line)) = lines.iter_mut().find(|(existing, _)| *existing == key) {
      if !line.text.is_empty() {
        line.text.push(' ');
      }
      line.text.push_str(&text);
      line.words.push(word);
    } else {
      lines.push((
        key,
        Line {
          text,
          words: vec![word],
        },
      ));
    }
  }
  lines.into_iter().map(|(_, line)| line).collect()
}

fn parse_word(columns: &[&str]) -> Option<Word> {
  Some(Word {
    left: columns.get(6)?.parse().ok()?,
    top: columns.get(7)?.parse().ok()?,
    width: columns.get(8)?.parse().ok()?,
    height: columns.get(9)?.parse().ok()?,
    confidence: parse_confidence(columns.get(10)?),
  })
}

fn parse_confidence(raw: &str) -> Option<f32> {
  let confidence = raw.parse::<f32>().ok()?;
  if confidence < 0.0 {
    None
  } else {
    Some((confidence / 100.0).clamp(0.0, 1.0))
  }
}

fn union_words(words: &[Word]) -> Option<Rect> {
  let mut iter = words.iter();
  let first = iter.next()?;
  let mut min_x = first.left;
  let mut min_y = first.top;
  let mut max_x = first.left + first.width;
  let mut max_y = first.top + first.height;
  for word in iter {
    min_x = min_x.min(word.left);
    min_y = min_y.min(word.top);
    max_x = max_x.max(word.left + word.width);
    max_y = max_y.max(word.top + word.height);
  }
  Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

fn mean_confidence(words: &[Word]) -> Option<f32> {
  let mut total = 0.0f32;
  let mut count = 0usize;
  for confidence in words.iter().filter_map(|word| word.confidence) {
    total += confidence;
    count += 1;
  }
  (count > 0).then_some(total / count as f32)
}

#[cfg(test)]
mod tests {
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
}
