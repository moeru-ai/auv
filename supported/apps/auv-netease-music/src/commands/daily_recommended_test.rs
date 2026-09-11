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
