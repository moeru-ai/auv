use super::*;

#[test]
fn classifies_detail_page_from_title_and_play_label() {
  let recognition = recognition(&["27 / 7 每日推荐", "播放全部", "Magnolia"]);

  let view = DailyRecommendedView::parse(&recognition).expect("daily recommended detail should parse");

  assert_eq!(view.title(), "27 / 7 每日推荐");
}

#[test]
fn rejects_home_page_card_without_detail_play_label() {
  let recognition = recognition(&["每日推荐", "推荐歌单"]);

  assert_eq!(DailyRecommendedView::parse(&recognition), None);
}

fn recognition(labels: &[&str]) -> TextRecognition {
  TextRecognition {
    origin: None,
    text: labels.join("\n"),
    regions: labels
      .iter()
      .enumerate()
      .map(|(index, text)| auv_driver::vision::RecognizedText {
        text: (*text).to_string(),
        bounds: auv_driver::Rect::new(343.0, 100.0 + index as f64 * 48.0, 180.0, 24.0),
        confidence: Some(0.9),
      })
      .collect(),
  }
}
