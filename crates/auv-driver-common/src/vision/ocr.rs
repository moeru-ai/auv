use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Position, Positioned, Rect, WindowPoint};
use crate::window::WindowRef;

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

  /// Binds window-local recognition to its source window. Convert recognition
  /// into window space before calling this; no coordinate conversion is implicit.
  pub fn in_window(self, window: &WindowRef) -> Positioned<Self> {
    let position = Position::in_window(window, WindowPoint::from(self.action_point()));
    Positioned {
      value: self,
      position,
    }
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
  /// Changes the origin of already-scaled logical bounds. For full-window
  /// capture OCR, pass the capture bounds origin to obtain window-local bounds.
  /// Pixel scaling is owned by the recognition producer and must not be repeated.
  pub fn relative_to(mut self, origin: Point) -> Self {
    for region in &mut self.regions {
      region.bounds.origin.x -= origin.x;
      region.bounds.origin.y -= origin.y;
    }
    self
  }

  pub fn find_contains(&self, query: &str) -> Vec<&RecognizedText> {
    let normalized_query = query.to_lowercase();
    self.regions.iter().filter(|region| region.text.to_lowercase().contains(&normalized_query)).collect()
  }

  pub fn best_contains(&self, query: &str) -> Option<&RecognizedText> {
    self.find_contains(query).into_iter().next()
  }
}

#[cfg(test)]
#[path = "ocr_test.rs"]
mod tests;
