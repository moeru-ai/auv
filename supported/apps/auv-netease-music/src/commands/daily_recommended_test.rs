use super::*;

#[test]
fn card_click_point_targets_card_body_from_title_bounds() {
  let bounds = ViewBounds::new(430.0, 102.0, 72.0, 20.0);

  let point = daily_recommended_card_click_point(bounds);

  assert_eq!(point, auv_driver::Point::new(485.0, 182.0));
}

#[test]
fn card_click_point_handles_bottom_title_bounds() {
  let bounds = ViewBounds::new(430.0, 278.0, 145.0, 36.0);

  let point = daily_recommended_card_click_point(bounds);

  assert_eq!(point, auv_driver::Point::new(500.0, 183.0));
}

#[test]
fn sidebar_recommend_prefers_the_exact_label_over_the_daily_card_title() {
  // ROOT CAUSE:
  //
  // If the recommendation home was already open, the "每日推荐" card title
  // (just above the sidebar row, and inside the 28% guard) also contains
  // "推荐", so the topmost-containing match clicked the card instead of the
  // sidebar entry. Starting from another page, the sidebar entry was clicked
  // instead, so the flow behaved differently by starting page.
  //
  // The fix prefers an exact label over a containing one.
  let window = auv_driver::WindowRef {
    id: "window-1".into(),
  };
  let text = |text: &str, x, y, width, height| auv_driver::vision::RecognizedText {
    text: text.to_string(),
    bounds: auv_driver::Rect::new(x, y, width, height),
    confidence: None,
  };
  let recognition = TextRecognition {
    origin: Some(auv_driver::Position::in_window(&window, auv_driver::WindowPoint::new(0.0, 0.0))),
    text: String::new(),
    regions: vec![
      text("每日推荐", 363.0, 98.0, 158.0, 32.0),
      text("推荐", 31.0, 100.0, 58.0, 21.0),
    ],
  };

  let matched =
    best_text_match(&recognition, "推荐", Size::new(1400.0, 900.0), |bounds, size| bounds.x < size.width * 0.28).unwrap().expect("a match");

  assert_eq!(matched.value.text, "推荐");
}
