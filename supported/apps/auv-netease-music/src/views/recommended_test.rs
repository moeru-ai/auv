use super::*;

// These are narrow OCR-region classification cases. They do not open an app,
// deliver input, or fake a driver workflow.
#[test]
fn classifies_feature_entries_without_treating_recommended_playlists_as_entries() {
  let view = RecommendedView::parse(
    &recognition(vec![
      ("每日推荐", 354.0, 100.0, 90.0, 22.0),
      ("私人雷达", 538.0, 100.0, 90.0, 22.0),
      ("心动模式", 720.0, 100.0, 90.0, 22.0),
      ("推荐歌单", 343.0, 340.0, 110.0, 25.0),
      ("某张真正的歌单", 343.0, 410.0, 150.0, 22.0),
    ]),
    auv_driver::Size::new(1645.0, 957.0),
  )
  .expect("recommended view should parse");

  let entries = view.featured_entries().visible_items();
  assert_eq!(entries.len(), 3);
  assert_eq!(entries[0].entry().kind(), FeaturedEntryKind::DailyRecommended);
  assert_eq!(entries[1].entry().kind(), FeaturedEntryKind::PrivateRadar);
  assert_eq!(entries[2].entry().kind(), FeaturedEntryKind::HeartbeatMode);
  assert_eq!(view.daily(), Some(DailyRecommendedRef));
}

#[test]
fn rejects_daily_detail_text_without_the_recommended_playlist_section() {
  let view = RecommendedView::parse(
    &recognition(vec![
      ("27 / 7 每日推荐", 343.0, 102.0, 220.0, 34.0),
      ("播放全部", 343.0, 160.0, 90.0, 24.0),
    ]),
    auv_driver::Size::new(1645.0, 957.0),
  );

  assert_eq!(view, None);
}

fn recognition(regions: Vec<(&str, f64, f64, f64, f64)>) -> TextRecognition {
  TextRecognition {
    origin: None,
    text: regions.iter().map(|(text, _, _, _, _)| *text).collect::<Vec<_>>().join("\n"),
    regions: regions
      .into_iter()
      .map(|(text, x, y, width, height)| auv_driver::vision::RecognizedText {
        text: text.to_string(),
        bounds: auv_driver::Rect::new(x, y, width, height),
        confidence: Some(0.9),
      })
      .collect(),
  }
}
