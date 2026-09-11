//! System OCR backed by Tesseract through `leptess`.
//!
//! This mirrors the Windows OCR module: callers provide a raw RGBA image and
//! receive shared [`TextRecognition`] records whose bounds are in image-pixel
//! coordinates. Capture-region cropping and coordinate projection belong to the
//! `vision` module.

use std::fmt;

use auv_driver_common::geometry::Rect;
use auv_driver_common::vision::{OcrMatch, OcrMatches, RecognizedText, TextRecognition, TextRecognitionOptions};
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

#[derive(Clone, Debug, PartialEq)]
struct Word {
  text: String,
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
  let tsv = tesseract_tsv_from_rgba(rgba, width, height, options)?;
  Ok(text_recognition_from_tsv(&tsv))
}

#[cfg(target_os = "linux")]
pub(crate) fn find_text_in_rgba(
  rgba: &[u8],
  width: u32,
  height: u32,
  query: &str,
  options: &TextRecognitionOptions,
) -> Result<OcrMatches, OcrError> {
  let tsv = tesseract_tsv_from_rgba(rgba, width, height, options)?;
  Ok(text_matches_from_tsv(&tsv, query))
}

#[cfg(target_os = "linux")]
fn tesseract_tsv_from_rgba(rgba: &[u8], width: u32, height: u32, options: &TextRecognitionOptions) -> Result<String, OcrError> {
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
  // TODO(linux-tesseract-session-reuse): cache language-keyed engines only
  // after the Driver owns their thread/concurrency lifecycle; current profiling
  // shows image scope and remote frame transport dominate engine initialization.
  let mut tess =
    leptess::LepTess::new(None, &language).map_err(|error| OcrError::Runtime(format!("failed to initialize Tesseract: {error}")))?;

  // NOTICE(linux-tesseract-custom-words): leptess exposes Tesseract variables
  // but not a stable cross-version user-word injection surface. Custom word
  // weighting is deferred until an owner-approved OCR-quality slice needs it.
  tess.set_image_from_mem(&png).map_err(|error| OcrError::Runtime(format!("failed to load image into Tesseract: {error}")))?;
  tess.set_source_resolution(144);
  tess.get_tsv_text(0).map_err(|error| OcrError::Runtime(format!("failed to read Tesseract TSV: {error}")))
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

#[cfg(not(target_os = "linux"))]
pub(crate) fn find_text_in_rgba(
  _rgba: &[u8],
  _width: u32,
  _height: u32,
  _query: &str,
  _options: &TextRecognitionOptions,
) -> Result<OcrMatches, OcrError> {
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

fn text_matches_from_tsv(tsv: &str, query: &str) -> OcrMatches {
  let normalized_query = normalize_match_text(query);
  if normalized_query.is_empty() {
    return OcrMatches::default();
  }

  let mut matches = Vec::new();
  for line in parse_tsv_lines(tsv) {
    for start in 0..line.words.len() {
      let mut candidate = String::new();
      for end in start..line.words.len() {
        if !candidate.is_empty() {
          candidate.push(' ');
        }
        candidate.push_str(&line.words[end].text);
        if !normalize_match_text(&candidate).contains(&normalized_query) {
          continue;
        }
        let words = &line.words[start..=end];
        let Some(bounds) = union_words(words) else {
          break;
        };
        matches.push(OcrMatch {
          text: candidate,
          confidence: mean_confidence(words).unwrap_or_default() as f64,
          bounds,
        });
        break;
      }
    }
  }
  let candidates = matches.clone();
  matches.retain(|candidate| !candidates.iter().any(|other| candidate != other && rect_strictly_contains(candidate.bounds, other.bounds)));
  OcrMatches { matches }
}

fn rect_strictly_contains(container: Rect, candidate: Rect) -> bool {
  let contains = candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height;
  contains && candidate.size.width * candidate.size.height < container.size.width * container.size.height
}

fn normalize_match_text(text: &str) -> String {
  text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
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
    let Some(word) = parse_word(&columns, text.clone()) else {
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

fn parse_word(columns: &[&str], text: String) -> Option<Word> {
  Some(Word {
    text,
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
#[path = "ocr_test.rs"]
mod tests;
