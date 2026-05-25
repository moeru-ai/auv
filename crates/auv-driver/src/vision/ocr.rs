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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRecognition {
  pub text: String,
  pub regions: Vec<RecognizedText>,
}

impl TextRecognition {
  pub fn find_contains(&self, query: &str) -> Vec<&RecognizedText> {
    let normalized_query = normalize_text(query);
    self
      .regions
      .iter()
      .filter(|region| normalize_text(&region.text).contains(&normalized_query))
      .collect()
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

    let matched = recognition
      .best_contains("cure for")
      .expect("text should match");

    assert_eq!(matched.text, "Cure For Me");
    assert_eq!(matched.action_point(), Point::new(25.0, 40.0));
  }
}
