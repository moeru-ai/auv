use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect};

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
  pub fn find_contains(&self, query: &str) -> Vec<&RecognizedText> {
    let normalized_query = normalize_text(query);
    self.regions.iter().filter(|region| normalize_text(&region.text).contains(&normalized_query)).collect()
  }

  pub fn best_contains(&self, query: &str) -> Option<&RecognizedText> {
    self.find_contains(query).into_iter().next()
  }
}

fn normalize_text(text: &str) -> String {
  text.to_lowercase()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_recognition_finds_case_insensitive_contains_match() {
    let recognition = TextRecognition {
      text: "Cure For Me\nAURORA".to_string(),
      regions: vec![
        RecognizedText {
          text: "Cure For Me".to_string(),
          bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
          confidence: Some(0.9),
        },
        RecognizedText {
          text: "AURORA".to_string(),
          bounds: Rect::new(50.0, 60.0, 70.0, 80.0),
          confidence: Some(0.8),
        },
      ],
    };

    let matched = recognition.best_contains("cure for").expect("text should match");

    assert_eq!(matched.text, "Cure For Me");
    assert_eq!(matched.action_point(), Point::new(25.0, 40.0));
  }

  #[test]
  fn text_recognition_options_preserve_provider_hints() {
    let options = TextRecognitionOptions::default().with_custom_words(["绚香", "AURORA"]).with_recognition_languages(["zh-Hans", "en-US"]);

    assert_eq!(options.custom_words, vec!["绚香", "AURORA"]);
    assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
  }

  #[test]
  fn ocr_matches_share_action_point_and_best_match_contract() {
    let matches = OcrMatches {
      matches: vec![OcrMatch {
        text: "Play".to_string(),
        confidence: 0.92,
        bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
      }],
    };

    let matched = matches.best_match().expect("one match");

    assert_eq!(matched.action_point(), Point::new(25.0, 40.0));
  }
}
