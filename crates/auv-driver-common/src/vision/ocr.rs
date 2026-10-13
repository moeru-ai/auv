use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Position, Positional, Positioned, Rect};
use crate::{DriverError, DriverResult};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecognizedText {
  pub text: String,
  pub bounds: Rect,
  pub confidence: Option<f32>,
}

impl RecognizedText {
  pub fn action_point(&self) -> Point {
    self.bounds.center()
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OcrMatch {
  pub text: String,
  pub confidence: f64,
  pub bounds: Rect,
}

impl OcrMatch {
  pub fn action_point(&self) -> Point {
    self.bounds.center()
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OcrMatches {
  pub matches: Vec<OcrMatch>,
}

impl OcrMatches {
  pub fn best_match(&self) -> Option<&OcrMatch> {
    self.matches.first()
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRecognition {
  /// Bounds are logical offsets from this origin. Raw image OCR and older
  /// serialized observations may be unbound; rebasing/action targets reject it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub origin: Option<Position>,
  pub text: String,
  pub regions: Vec<RecognizedText>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRecognitionOptions {
  pub custom_words: Vec<String>,
  pub recognition_languages: Option<Vec<String>>,
}

impl TextRecognitionOptions {
  pub fn with_custom_words(mut self, words: impl IntoIterator<Item = impl Into<String>>) -> Self {
    self.custom_words = words.into_iter().map(Into::into).collect();
    self
  }

  pub fn with_recognition_languages(mut self, languages: impl IntoIterator<Item = impl Into<String>>) -> Self {
    self.recognition_languages = Some(languages.into_iter().map(Into::into).collect());
    self
  }
}

impl TextRecognition {
  /// Expresses logical bounds relative to a position in the same owning space.
  /// The updated origin travels with the result, so rebasing again is safe and
  /// selected targets keep their identity. Cross-space projection is explicit;
  /// a same-shaped point in another window cannot rebind this observation.
  pub fn relative_to(mut self, target: &(impl Positional + ?Sized)) -> DriverResult<Self> {
    let origin = self.origin.as_ref().ok_or_else(|| DriverError::InvalidInput {
      message: "recognition has no bound coordinate origin".into(),
    })?;
    let target = target.position()?;
    if origin.coordinate_space != target.coordinate_space {
      return Err(DriverError::InvalidInput {
        message: "recognition and target belong to different coordinate spaces".into(),
      });
    }
    let dx = origin.point.x - target.point.x;
    let dy = origin.point.y - target.point.y;
    if !dx.is_finite() || !dy.is_finite() {
      return Err(DriverError::InvalidInput {
        message: "coordinate origins must be finite".into(),
      });
    }
    for region in &mut self.regions {
      region.bounds.origin.x += dx;
      region.bounds.origin.y += dy;
    }
    self.origin = Some(target);
    Ok(self)
  }

  /// Carries the observation origin into each selected text's action point.
  /// Bounds remain relative to the recognition origin for layout consumers.
  pub fn positioned_regions(&self) -> DriverResult<impl Iterator<Item = Positioned<RecognizedText>> + '_> {
    let origin = self.origin.clone().ok_or_else(|| DriverError::InvalidInput {
      message: "recognition has no bound coordinate origin".into(),
    })?;
    Ok(self.regions.iter().map(move |region| {
      let center = region.action_point();
      Positioned {
        value: region.clone(),
        position: Position {
          point: Point::new(origin.point.x + center.x, origin.point.y + center.y),
          coordinate_space: origin.coordinate_space.clone(),
        },
      }
    }))
  }

  /// Returns regions whose text contains `query` after both sides pass
  /// through [`normalize_match_text`].
  pub fn find_contains(&self, query: &str) -> Vec<&RecognizedText> {
    let normalized_query = normalize_match_text(query);
    if normalized_query.is_empty() {
      return Vec::new();
    }
    self.regions.iter().filter(|region| normalize_match_text(&region.text).contains(&normalized_query)).collect()
  }

  pub fn best_contains(&self, query: &str) -> Option<&RecognizedText> {
    self.find_contains(query).into_iter().next()
  }
}

/// Normalizes a text query or recognized text the same way before matching.
///
/// Besides whitespace and case, this folds typographic quotes and dashes,
/// drops spaces before `.` and `,`, and joins `. ` before a lowercase letter.
/// OCR engines disagree on punctuation in both directions: Vision reports
/// `“Report — Final”` as `"Report - Final"`, and Tesseract reads `Huashan.jpg`
/// as `Huashan. jpg`. A caller typing the visible text must still match.
///
/// A space after `.` stays before anything but a lowercase letter, so a
/// sentence end (`For example. Commands`) does not turn into a word that
/// matches `example.com`.
pub fn normalize_match_text(text: &str) -> String {
  let folded = text
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .chars()
    .map(|character| match character {
      '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => '\'',
      '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => '"',
      '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
      other => other,
    })
    .collect::<Vec<_>>();
  let mut normalized = String::with_capacity(text.len());
  for (index, &character) in folded.iter().enumerate() {
    if character == ' ' {
      let previous = index.checked_sub(1).map(|previous| folded[previous]);
      let next = folded.get(index + 1).copied();
      if matches!(next, Some('.' | ',')) || (previous == Some('.') && next.is_some_and(char::is_lowercase)) {
        continue;
      }
    }
    normalized.extend(character.to_lowercase());
  }
  normalized
}

#[cfg(test)]
#[path = "ocr_test.rs"]
mod tests;
