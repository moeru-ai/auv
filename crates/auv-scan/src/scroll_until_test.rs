use std::time::Duration;

use auv_driver::{
  Capture, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, RecognizedText, Rect, Scroll, TextRecognition,
};
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
  recognitions: u32,
  /// Backing pixels per point for native captures.
  scale: u32,
  resolutions: Vec<CaptureResolution>,
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
      recognitions: 0,
      scale: 1,
      resolutions: Vec::new(),
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

  fn capture(&mut self, resolution: CaptureResolution) -> DriverResult<Capture> {
    self.resolutions.push(resolution);
    let position = self.position;
    let scale = if resolution == CaptureResolution::Native {
      self.scale
    } else {
      1
    };
    // Each logical row spans `scale` backing rows.
    let image = RgbaImage::from_fn(40 * scale, self.viewport as u32 * scale, |_, y| {
      let value = (((position + i64::from(y / scale)) * 37).rem_euclid(251)) as u8;
      Rgba([value, value.wrapping_mul(3), value.wrapping_add(90), 255])
    });
    Ok(Capture {
      origin: None,
      image,
      // A window at (100, 200) on screen, so text matches must be offset.
      bounds: Rect::new(100.0, 200.0, 40.0, self.viewport as f64),
      scale_factor: f64::from(scale),
      backend: "fake".to_string(),
      fallback_reason: None,
    })
  }

  fn recognize_text(&mut self, _: &Capture) -> DriverResult<TextRecognition> {
    self.recognitions += 1;
    let visible = self.text_row.filter(|row| (self.position..self.position + self.viewport).contains(row));
    // Like the drivers, bounds are in the capture's screen space: the window
    // sits at (100, 200) on screen.
    let mut regions = vec![RecognizedText {
      text: format!("row at {}", self.position),
      bounds: Rect::new(100.0, 200.0, 40.0, 1.0),
      confidence: None,
    }];
    regions.extend(visible.map(|row| RecognizedText {
      text: "TARGET ROW".to_string(),
      bounds: Rect::new(100.0, 200.0 + (row - self.position) as f64, 40.0, 1.0),
      confidence: None,
    }));
    Ok(TextRecognition {
      origin: None,
      text: String::new(),
      regions,
    })
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
    output: ScrollUntilOutputOptions::default(),
  }
}

fn run(list: &mut FakeList, request: &ScrollUntilRequest) -> DriverResult<ScrollUntilResult> {
  scroll_until(list, request, &mut |_| Ok(ScrollUntilDecision::Continue))
}

#[test]
fn end_stops_after_consecutive_no_motion_at_the_bottom() {
  let mut list = FakeList::new(260);
  let mut streaks = Vec::new();
  let result = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |update| {
    streaks.push(update.no_motion_streak);
    Ok(ScrollUntilDecision::Continue)
  })
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::EndByNoVisualProgress);
  assert_eq!(list.position, 200);
  // Four steps reach 200; two more confirm no motion.
  assert_eq!(result.steps, 6);
  assert_eq!(streaks, [0, 0, 0, 0, 0, 1, 2]);
  assert_eq!(result.action.unwrap().selected_path, InputDeliveryPath::WindowTargetedWheel);
}

#[test]
fn end_keeps_scrolling_when_lazy_content_arrives_during_settle() {
  let mut list = FakeList::new(260);
  list.lazy_batch = 300;
  list.lazy_after_waits = 1;
  let result = run(&mut list, &request(ScrollUntilCondition::End)).unwrap();
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
    &mut |_| Ok(ScrollUntilDecision::Continue),
  )
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::TextVisible);
  assert!(list.position <= 420 && 420 < list.position + 60, "{}", list.position);
  let matched = result.text_match.unwrap();
  assert_eq!(matched.text, "TARGET ROW");
  // ROOT CAUSE:
  //
  // If the window was not at the screen origin, the match was offset by the
  // window origin twice: the drivers already return OCR bounds in screen
  // space, and `text_match` added `capture.bounds.origin` again. The fake
  // returned offsets instead of screen bounds, which hid it.
  //
  // The fix uses the driver's screen bounds as they are.
  assert_eq!(matched.bounds, Rect::new(100.0, 200.0 + (420 - list.position) as f64, 40.0, 1.0));
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
    &mut |_| Ok(ScrollUntilDecision::Continue),
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
    &mut |_| Ok(ScrollUntilDecision::Continue),
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
  let result = run(&mut list, &short).unwrap();
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
  region.motion_region = Some(auv_driver::RelativeRect::new(0.5, 0.0, 0.6, 1.0));
  cases.push(region);
  cases.push(request(ScrollUntilCondition::TextVisible {
    query: "  ".to_string(),
  }));
  for case in cases {
    let mut list = FakeList::new(260);
    assert!(matches!(run(&mut list, &case), Err(DriverError::InvalidInput { .. })), "{case:?}");
    assert_eq!(list.scrolls, 0);
  }
}

#[test]
fn observer_sees_every_update_with_text_by_default() {
  let mut list = FakeList::new(260);
  let mut seen = Vec::new();
  scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |update| {
    assert!(update.text.is_some(), "{update:?}");
    seen.push((update.steps, update.motion.is_some(), update.stop));
    Ok(ScrollUntilDecision::Continue)
  })
  .unwrap();
  assert_eq!(seen.first(), Some(&(0, false, None)), "initial update has no motion yet");
  assert_eq!(seen.len(), 7);
  assert_eq!(seen.last(), Some(&(6, true, Some(ScrollUntilStopReason::EndByNoVisualProgress))));
}

#[test]
fn observer_stop_ends_the_loop_as_predicate_satisfied() {
  let mut list = FakeList::new(2_000);
  let result = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |update| {
    let text = update.text.expect("text is observed by default");
    Ok(if text.regions[0].text == "row at 150" {
      ScrollUntilDecision::Stop
    } else {
      ScrollUntilDecision::Continue
    })
  })
  .unwrap();
  assert_eq!((result.reason, result.steps, list.position), (ScrollUntilStopReason::PredicateSatisfied, 3, 150));
}

#[test]
fn observer_can_stop_before_the_first_step() {
  let mut list = FakeList::new(2_000);
  let result = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |_| Ok(ScrollUntilDecision::Stop)).unwrap();
  assert_eq!((result.reason, result.steps, list.scrolls), (ScrollUntilStopReason::PredicateSatisfied, 0, 0));
  assert!(result.action.is_none());
}

#[test]
fn built_in_stop_wins_over_the_observer_decision() {
  let mut list = FakeList::new(2_000);
  list.text_row = Some(10);
  let result = scroll_until(
    &mut list,
    &request(ScrollUntilCondition::TextVisible {
      query: "target".to_string(),
    }),
    &mut |update| {
      assert_eq!(update.stop, Some(ScrollUntilStopReason::TextVisible));
      Ok(ScrollUntilDecision::Stop)
    },
  )
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::TextVisible);
}

#[test]
fn observer_errors_abort_the_loop() {
  let mut list = FakeList::new(2_000);
  let error = scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |update| {
    if update.steps == 2 {
      Err(DriverError::InvalidInput {
        message: "client went away".to_string(),
      })
    } else {
      Ok(ScrollUntilDecision::Continue)
    }
  })
  .unwrap_err();
  assert!(error.to_string().contains("client went away"), "{error}");
  assert_eq!(list.scrolls, 2);
}

#[test]
fn opted_out_updates_skip_recognition() {
  let mut list = FakeList::new(260);
  let mut end = request(ScrollUntilCondition::End);
  end.output = ScrollUntilOutputOptions { text: false };
  scroll_until(&mut list, &end, &mut |update| {
    assert!(update.text.is_none());
    Ok(ScrollUntilDecision::Continue)
  })
  .unwrap();
  assert_eq!(list.recognitions, 0);
}

#[test]
fn text_condition_still_recognizes_when_text_is_opted_out() {
  let mut list = FakeList::new(2_000);
  list.text_row = Some(420);
  let mut find = request(ScrollUntilCondition::TextVisible {
    query: "target".to_string(),
  });
  find.output.text = false;
  let result = scroll_until(&mut list, &find, &mut |update| {
    assert!(update.text.is_none());
    Ok(ScrollUntilDecision::Continue)
  })
  .unwrap();
  assert_eq!(result.reason, ScrollUntilStopReason::TextVisible);
  assert!(list.recognitions > 0);
}

#[test]
fn motion_only_loops_capture_at_logical_resolution() {
  let mut list = FakeList::new(260);
  let mut end = request(ScrollUntilCondition::End);
  end.output = ScrollUntilOutputOptions { text: false };
  scroll_until(&mut list, &end, &mut |_| Ok(ScrollUntilDecision::Continue)).unwrap();
  assert!(list.resolutions.iter().all(|resolution| *resolution == CaptureResolution::Logical), "{:?}", list.resolutions);

  let mut list = FakeList::new(260);
  scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |_| Ok(ScrollUntilDecision::Continue)).unwrap();
  assert!(list.resolutions.iter().all(|resolution| *resolution == CaptureResolution::Native), "text needs native pixels");
}

#[test]
fn retina_captures_detect_the_same_motion_as_one_x_captures() {
  // ROOT CAUSE:
  //
  // If a window capture came back at 2x, motion was compared in backing
  // pixels, so the ±24 px search policy validated on 1x captures covered only
  // 12 points and a step's shift read as twice as large.
  //
  // The fix compares motion per logical point whatever the capture resolution.
  let run = |scale: u32| {
    let mut list = FakeList::new(260);
    list.scale = scale;
    let mut seen = Vec::new();
    scroll_until(&mut list, &request(ScrollUntilCondition::End), &mut |observation| {
      seen.push((observation.steps, observation.motion.map(|motion| (motion.estimated_shift, motion.no_motion))));
      Ok(ScrollUntilDecision::Continue)
    })
    .unwrap();
    seen
  };
  assert_eq!(run(2), run(1));
}

#[test]
fn new_request_uses_the_shared_defaults_and_validates() {
  let request = ScrollUntilRequest::new(
    ScrollUntilStep::Instant {
      delta: Scroll::new(0.0, 120.0),
    },
    ScrollUntilCondition::End,
  );
  assert_eq!((request.max_steps, request.settle, request.no_motion_confirmations), (50, Duration::from_millis(400), 2));
  assert_eq!(request.motion_region, None);
  assert_eq!(request.output, ScrollUntilOutputOptions::default());
  assert!(request.validate().is_ok());
}
