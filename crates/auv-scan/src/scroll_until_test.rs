use std::time::Duration;

use auv_driver::{Capture, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, Rect, Scroll};
use image::{Rgba, RgbaImage};

use super::*;

/// A vertical list of distinct rows. `content` grows by `lazy_batch` rows once
/// the viewport has waited at the bottom `lazy_after_waits` times.
struct FakeList {
  position: i64,
  content: i64,
  viewport: i64,
  lazy_batch: i64,
  lazy_after_waits: u32,
  bottom_waits: u32,
  text_row: Option<i64>,
  scrolls: u32,
}

impl FakeList {
  fn new(content: i64) -> Self {
    Self {
      position: 0,
      content,
      viewport: 60,
      lazy_batch: 0,
      lazy_after_waits: 0,
      bottom_waits: 0,
      text_row: None,
      scrolls: 0,
    }
  }

  fn max_position(&self) -> i64 {
    (self.content - self.viewport).max(0)
  }
}

fn action() -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedWheel,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::WindowTargetedWheel,
    )],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::None,
    clipboard_disturbance: DisturbanceLevel::None,
  }
}

impl ScrollUntilSurface for FakeList {
  fn scroll(&mut self, step: &ScrollUntilStep) -> DriverResult<(InputActionResult, Scroll)> {
    self.scrolls += 1;
    let delta = step.delta().delta_y as i64;
    self.position = (self.position + delta).clamp(0, self.max_position());
    Ok((action(), step.delta()))
  }

  fn capture(&mut self) -> DriverResult<Capture> {
    let position = self.position;
    let image = RgbaImage::from_fn(40, self.viewport as u32, |_, y| {
      let value = (((position + i64::from(y)) * 37).rem_euclid(251)) as u8;
      Rgba([value, value.wrapping_mul(3), value.wrapping_add(90), 255])
    });
    Ok(Capture {
      origin: None,
      image,
      bounds: Rect::new(0.0, 0.0, 40.0, self.viewport as f64),
      scale_factor: 1.0,
      backend: "fake".to_string(),
      fallback_reason: None,
    })
  }

  fn find_text(&mut self, _: &Capture, query: &str) -> DriverResult<Option<ScrollUntilTextMatch>> {
    let visible = self.text_row.filter(|row| (self.position..self.position + self.viewport).contains(row));
    Ok(visible.map(|row| ScrollUntilTextMatch {
      text: query.to_string(),
      bounds: Rect::new(0.0, (row - self.position) as f64, 40.0, 1.0),
    }))
  }

  fn wait(&mut self, _: Duration) -> DriverResult<()> {
    if self.position == self.max_position() && self.lazy_batch > 0 {
      self.bottom_waits += 1;
      if self.bottom_waits >= self.lazy_after_waits {
        self.content += self.lazy_batch;
        self.lazy_batch = 0;
      }
    }
    Ok(())
  }
}

fn request(condition: ScrollUntilCondition) -> ScrollUntilRequest {
  ScrollUntilRequest {
    step: ScrollUntilStep::Instant {
      delta: Scroll::new(0.0, 50.0),
    },
    condition,
    max_steps: 50,
    settle: Duration::ZERO,
    no_motion_confirmations: 2,
    motion_region: None,
  }
}

#[test]
fn end_stops_after_consecutive_no_motion_at_the_bottom() {
  let mut list = FakeList::new(260);
  let mut progress = Vec::new();
  let result = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |update| progress.push(update.clone())).unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::EndByNoVisualProgress);
  assert_eq!(list.position, 200);
  // Four steps reach 200; two more confirm no motion.
  assert_eq!(result.steps, 6);
  assert_eq!(progress.last().unwrap().no_motion_streak, 2);
  assert_eq!(result.action.unwrap().selected_path, InputDeliveryPath::WindowTargetedWheel);
}

#[test]
fn end_keeps_scrolling_when_lazy_content_arrives_during_settle() {
  let mut list = FakeList::new(260);
  list.lazy_batch = 300;
  list.lazy_after_waits = 1;
  let result = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |_| {}).unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::EndByNoVisualProgress);
  assert_eq!(list.content, 560);
  assert_eq!(list.position, 500);
}

#[test]
fn text_condition_stops_as_soon_as_the_query_is_visible() {
  let mut list = FakeList::new(2_000);
  list.text_row = Some(420);
  let result = scroll_until(
    &mut list,
    &request(ScrollUntilCondition::TextVisible {
      query: "Target".to_string(),
    }),
    &mut |_| {},
  )
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::TextVisible);
  assert!(list.position <= 420 && 420 < list.position + 60, "{}", list.position);
  assert_eq!(result.text_match.unwrap().text, "Target");
}

#[test]
fn text_already_visible_needs_no_step() {
  let mut list = FakeList::new(2_000);
  list.text_row = Some(10);
  let result = scroll_until(
    &mut list,
    &request(ScrollUntilCondition::TextVisible {
      query: "Target".to_string(),
    }),
    &mut |_| {},
  )
  .unwrap();
  assert_eq!((result.reason, result.steps, list.scrolls), (ScrollUntilStopReason::TextVisible, 0, 0));
  assert!(result.action.is_none());
}

#[test]
fn text_condition_stops_at_the_end_when_the_query_never_appears() {
  let mut list = FakeList::new(260);
  let result = scroll_until(
    &mut list,
    &request(ScrollUntilCondition::TextVisible {
      query: "Missing".to_string(),
    }),
    &mut |_| {},
  )
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::EndByNoVisualProgress);
  assert!(result.text_match.is_none());
}

#[test]
fn budget_exhaustion_is_reported_separately_from_the_end() {
  let mut list = FakeList::new(10_000);
  let mut short = request(ScrollUntilCondition::End);
  short.max_steps = 3;
  let result = scroll_until(&mut list, &short, &mut |_| {}).unwrap();
  assert_eq!((result.reason, result.steps), (ScrollUntilStopReason::BudgetExhausted, 3));
  assert_eq!(result.delivered, Scroll::new(0.0, 150.0));
}

#[test]
fn invalid_requests_are_rejected_before_any_input() {
  let base = request(ScrollUntilCondition::End);
  let mut cases = Vec::new();
  let mut diagonal = base.clone();
  diagonal.step = ScrollUntilStep::Instant {
    delta: Scroll::new(10.0, 10.0),
  };
  cases.push(diagonal);
  let mut zero = base.clone();
  zero.step = ScrollUntilStep::Instant {
    delta: Scroll::new(0.0, 0.0),
  };
  cases.push(zero);
  let mut budget = base.clone();
  budget.max_steps = 0;
  cases.push(budget);
  let mut confirmations = base.clone();
  confirmations.no_motion_confirmations = 0;
  cases.push(confirmations);
  let mut region = base.clone();
  region.motion_region = Some(auv_driver::RatioRect::new(0.5, 0.0, 0.6, 1.0));
  cases.push(region);
  cases.push(request(ScrollUntilCondition::TextVisible {
    query: "  ".to_string(),
  }));
  for case in cases {
    let mut list = FakeList::new(260);
    assert!(matches!(scroll_until(&mut list, &case, &mut |_| {}), Err(DriverError::InvalidInput { .. })), "{case:?}");
    assert_eq!(list.scrolls, 0);
  }
}
