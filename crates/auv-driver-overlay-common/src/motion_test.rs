use std::time::Duration;

use auv_driver_common::{MouseButton, Rect, ScreenPoint};

use super::{ActionEvent, CURSOR_COLORS, MotionScene, Travel, Wake};
use crate::Layer;
use crate::layers::{BuiltInCursor, Cursor, CursorImage, CursorPose};
use crate::style::Color;

fn ms(value: u64) -> Duration {
  Duration::from_millis(value)
}

fn at(x: f64, y: f64) -> ScreenPoint {
  ScreenPoint::new(x, y)
}

fn scene() -> MotionScene {
  MotionScene::new()
}

fn jump(x: f64, y: f64) -> ActionEvent {
  ActionEvent::Moved {
    point: at(x, y),
    travel: Travel::Jump,
    window: None,
  }
}

fn jump_in(window: &str, x: f64, y: f64) -> ActionEvent {
  ActionEvent::Moved {
    point: at(x, y),
    travel: Travel::Jump,
    window: Some(window.into()),
  }
}

fn sample(x: f64, y: f64) -> ActionEvent {
  ActionEvent::Moved {
    point: at(x, y),
    travel: Travel::Sampled,
    window: None,
  }
}

fn click(x: f64, y: f64, button: MouseButton) -> ActionEvent {
  ActionEvent::Clicked {
    point: at(x, y),
    button,
    window: None,
  }
}

fn click_in(window: &str, x: f64, y: f64) -> ActionEvent {
  ActionEvent::Clicked {
    point: at(x, y),
    button: MouseButton::Left,
    window: Some(window.into()),
  }
}

fn targeted(id: &str, frame: Rect) -> ActionEvent {
  ActionEvent::WindowTargeted {
    id: id.into(),
    frame,
    label: None,
  }
}

fn cursor_x(scene: &MotionScene, now: Duration) -> f64 {
  scene.cursor_point(None, now).expect("cursor placed").point().x
}

/// Every cursor drawn at `now`, bottom first.
fn cursor_layers(scene: &mut MotionScene, now: u64) -> Vec<Cursor> {
  let frame = scene.frame(ms(now));
  frame
    .overlay
    .layers()
    .iter()
    .filter_map(|layer| match layer {
      Layer::Cursor(cursor) => Some(cursor.clone()),
      _ => None,
    })
    .collect()
}

/// The cursor layer of the frame drawn at `now` (always the top layer).
fn cursor_layer(scene: &mut MotionScene, now: u64) -> Cursor {
  let frame = scene.frame(ms(now));
  let Some(Layer::Cursor(cursor)) = frame.overlay.layers().last() else {
    panic!("the cursor is drawn last at {now} ms");
  };
  cursor.clone()
}

/// Tilt of every frame drawn at 60 fps over `from..to` milliseconds.
fn tilts(scene: &mut MotionScene, from: u64, to: u64) -> Vec<f64> {
  (from..to).step_by(16).map(|now| cursor_layer(scene, now).pose().tilt_degrees).collect()
}

fn layer_kinds(layers: &[Layer]) -> Vec<&'static str> {
  layers
    .iter()
    .map(|layer| match layer {
      Layer::Cursor(_) => "cursor",
      Layer::Outline(_) => "outline",
      Layer::Status(_) => "status",
    })
    .collect()
}

fn accent(cursor: &Cursor) -> Color {
  cursor.style().accent.expect("live cursors carry their window's color")
}

#[test]
fn an_empty_scene_draws_nothing_and_stays_idle() {
  let mut scene = scene();
  let frame = scene.frame(ms(500));
  assert!(frame.overlay.layers().is_empty());
  assert_eq!(frame.wake, Wake::Idle);
  assert_eq!(scene.cursor_point(None, ms(500)), None);
}

#[test]
fn the_first_report_places_the_cursor_on_its_point_and_grows_it_in() {
  let mut scene = scene();
  scene.apply(jump(120.0, 80.0), ms(10));
  assert_eq!(scene.cursor_point(None, ms(10)), Some(at(120.0, 80.0)), "nothing to travel from");

  let first = scene.frame(ms(10));
  assert!(first.overlay.layers().is_empty(), "the cursor starts too small to draw");
  assert_eq!(first.wake, Wake::NextFrame);

  let sizes: Vec<f64> = (26..=270).step_by(16).map(|now| cursor_layer(&mut scene, now).pose().scale).collect();
  assert!(sizes.windows(2).take(8).all(|pair| pair[1] > pair[0]), "grows: {sizes:?}");
  let peak = sizes.iter().copied().fold(0.0, f64::max);
  assert!(peak > 1.05 && peak < 1.15, "overshoots a little: {peak}");

  let grown = cursor_layer(&mut scene, 270);
  assert_eq!(grown.pose(), CursorPose::REST, "full size once grown");
  assert_eq!(grown.point(), at(120.0, 80.0));
  // Still from here until the cursor starts fading, 3.4 s after the report.
  assert_eq!(scene.frame(ms(400)).wake, Wake::After(ms(3010)));
}

#[test]
fn a_jump_springs_along_the_straight_line_and_never_teleports() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(300.0, 0.0), ms(1000));

  assert_eq!(cursor_x(&scene, ms(1000)), 0.0, "the jump starts where the cursor is drawn");
  // After one smooth time (110 ms) a critically damped spring from rest has covered
  // 1 - 3/e^2 of the way, about 59%.
  let one_smooth_time = cursor_x(&scene, ms(1110));
  assert!((one_smooth_time - 300.0 * (1.0 - 3.0 * (-2.0f64).exp())).abs() < 1e-6, "{one_smooth_time}");

  // Peak speed is 300 px * omega / e, about 2 px per millisecond.
  let mut previous = 0.0;
  for now in 1000..=2000 {
    let point = scene.cursor_point(None, ms(now)).unwrap().point();
    assert_eq!(point.y, 0.0, "left the straight line at {now} ms");
    assert!(point.x >= previous, "moved backwards at {now} ms");
    assert!(point.x - previous < 2.1, "jumped {} px in one millisecond at {now} ms", point.x - previous);
    previous = point.x;
  }
  assert_eq!(cursor_x(&scene, ms(2000)), 300.0, "settles exactly on the reported point");
}

#[test]
fn a_jump_comes_to_rest_upright_and_the_scene_stops_drawing() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(600.0, 0.0), ms(1000));

  let mut now = 1000;
  while scene.frame(ms(now)).wake == Wake::NextFrame {
    now += 16;
    assert!(now < 2000, "still drawing a second after a 600 px jump");
  }
  assert_eq!(scene.cursor_point(None, ms(now)), Some(at(600.0, 0.0)));
  assert_eq!(cursor_layer(&mut scene, now).pose(), CursorPose::REST);
}

// ROOT CAUSE:
//
// If a new point was reported while the cursor was still travelling, the cursor stopped
// dead and started over, because every jump restarted `EaseInOutExpo` from zero speed.
//
// Before the fix, a retarget dropped the cursor's speed to nothing for about 100 ms.
// The fix carries the drawn velocity into the next spring.
#[test]
fn a_new_jump_mid_flight_keeps_the_cursor_moving_instead_of_stopping() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(300.0, 0.0), ms(1000));
  let drawn = cursor_x(&scene, ms(1060));
  let before = drawn - cursor_x(&scene, ms(1052));

  scene.apply(jump(900.0, 0.0), ms(1060));
  assert_eq!(cursor_x(&scene, ms(1060)), drawn, "retargeting must not move the cursor at the retarget instant");
  let after = cursor_x(&scene, ms(1068)) - drawn;
  assert!(after > before * 0.9, "the cursor kept its speed: {before:.1} px in the 8 ms before, {after:.1} px after");
}

#[test]
fn a_retarget_never_carries_the_cursor_past_its_new_target() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(600.0, 0.0), ms(1000));
  // Flying right at about 4 px/ms, the next reported point is only 5 px ahead.
  let target = cursor_x(&scene, ms(1055)) + 5.0;
  scene.apply(jump(target, 0.0), ms(1055));

  for now in 1055..2500 {
    let x = cursor_x(&scene, ms(now));
    assert!(x <= target, "passed the reported point at {now} ms: {x} > {target}");
  }
  assert_eq!(cursor_x(&scene, ms(2500)), target);
}

#[test]
fn a_turn_mid_flight_curves_but_stays_near_the_direct_line() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(600.0, 0.0), ms(1000));
  let corner = scene.cursor_point(None, ms(1055)).unwrap().point();
  // Flying right at about 4 px/ms, the next reported point is straight below.
  let target = at(corner.x, corner.y + 200.0);
  scene.apply(
    ActionEvent::Moved {
      point: target,
      travel: Travel::Jump,
      window: None,
    },
    ms(1055),
  );

  let swing = (1055..2500).map(|now| cursor_x(&scene, ms(now)) - corner.x).fold(0.0, f64::max);
  assert!(swing > 1.0, "the cursor carries some of its speed into the turn");
  assert!(swing <= 0.25 * 200.0 + 1e-6, "swung {swing:.1} px off the direct line");
  assert_eq!(scene.cursor_point(None, ms(2500)), Some(target));
}

#[test]
fn the_cursor_never_leaves_the_span_of_reported_points() {
  let mut scene = scene();
  let reports = [
    (0.0, 0.0),
    (300.0, 40.0),
    (120.0, 260.0),
    (500.0, 500.0),
    (10.0, 480.0),
  ];
  scene.apply(jump(reports[0].0, reports[0].1), ms(0));
  for (index, (x, y)) in reports.iter().copied().enumerate().skip(1) {
    scene.apply(jump(x, y), ms(index as u64 * 150));
  }
  for now in 0..2000 {
    let point = scene.cursor_point(None, ms(now)).unwrap().point();
    assert!((0.0..=500.0).contains(&point.x) && (0.0..=500.0).contains(&point.y), "drew at ({}, {}) at {now} ms", point.x, point.y);
  }
}

#[test]
fn a_sampled_stream_is_followed_closely_without_restarting_per_sample() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  // The driver plays a 1 px/ms drag at 125 Hz.
  for tick in 0..=50u64 {
    scene.apply(sample(tick as f64 * 8.0, 0.0), ms(1000 + tick * 8));
  }
  // A critically damped spring trails a steady stream by its smooth time, 25 ms here. A
  // jump-length spring per sample would trail by more than 100 px.
  let lag = 400.0 - cursor_x(&scene, ms(1400));
  assert!((15.0..=35.0).contains(&lag), "trails the stream by {lag:.1} px at 1 px/ms");
  assert_eq!(cursor_x(&scene, ms(2000)), 400.0, "settles on the last sample");
}

#[test]
fn a_sample_during_a_jump_keeps_the_cursor_continuous() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(100.0, 0.0), ms(1000));
  let drawn = cursor_x(&scene, ms(1050));

  scene.apply(sample(104.0, 0.0), ms(1050));
  assert_eq!(cursor_x(&scene, ms(1050)), drawn, "a sample must not move the cursor at the instant it arrives");
  assert_eq!(cursor_x(&scene, ms(1500)), 104.0);
}

#[test]
fn the_cursor_tilts_with_its_horizontal_speed_and_stands_upright_at_rest() {
  let mut scene = scene();
  scene.apply(jump(500.0, 300.0), ms(0));
  assert_eq!(cursor_layer(&mut scene, 300).pose().tilt_degrees, 0.0, "a resting cursor stands upright");

  scene.apply(jump(1100.0, 300.0), ms(1000));
  let rightward = tilts(&mut scene, 1000, 2500);
  let clockwise = rightward.iter().copied().fold(0.0, f64::max);
  assert!(clockwise > 5.0 && clockwise <= 14.0, "moving right tilts clockwise by at most 14 degrees, peaked at {clockwise}");
  assert_eq!(rightward.last().copied(), Some(0.0), "upright again once the cursor stopped");

  scene.apply(jump(500.0, 300.0), ms(3000));
  let counter_clockwise = tilts(&mut scene, 3000, 3300).into_iter().fold(0.0, f64::min);
  assert!((-14.0..-5.0).contains(&counter_clockwise), "moving left tilts the other way, peaked at {counter_clockwise}");
}

#[test]
fn a_vertical_move_does_not_tilt_the_cursor() {
  let mut scene = scene();
  scene.apply(jump(100.0, 0.0), ms(0));
  scene.apply(jump(100.0, 600.0), ms(1000));
  assert!(tilts(&mut scene, 1000, 2000).iter().all(|tilt| *tilt == 0.0));
}

#[test]
fn a_click_dips_the_cursor_about_its_tip_with_the_pressed_art_then_restores_it() {
  let mut scene = scene();
  scene.apply(jump(10.0, 10.0), ms(0));
  scene.apply(click(10.0, 10.0, MouseButton::Left), ms(1000));

  let pressed = cursor_layer(&mut scene, 1000);
  assert_eq!(pressed.image(), &CursorImage::built_in(BuiltInCursor::AuvClick));
  assert_eq!(pressed.pose().scale, 1.0, "the press starts at full size");
  let deepest = cursor_layer(&mut scene, 1090);
  assert!((deepest.pose().scale - 0.84).abs() < 1e-9, "{}", deepest.pose().scale);
  assert_eq!(deepest.point(), at(10.0, 10.0), "the press pivots on the click point");

  let released = cursor_layer(&mut scene, 1200);
  assert_eq!(released.image(), &CursorImage::built_in(BuiltInCursor::Auv));
  assert_eq!(released.pose(), CursorPose::REST);
}

#[test]
fn a_click_ripples_at_the_reported_point_at_the_reported_time() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(click(200.0, 100.0, MouseButton::Left), ms(1000));

  let frame = scene.frame(ms(1000));
  assert_eq!(layer_kinds(frame.overlay.layers()), ["outline", "cursor"]);
  let Layer::Outline(ripple) = &frame.overlay.layers()[0] else {
    panic!("expected the ripple outline first");
  };
  let bounds = ripple.rect();
  assert_eq!(bounds.origin.x + bounds.size.width / 2.0, 200.0);
  assert_eq!(bounds.origin.y + bounds.size.height / 2.0, 100.0);
  assert_eq!(frame.wake, Wake::NextFrame);
}

// The macOS renderer's click ripple (`drawFlashRippleIfActive` in `Overlay.swift`) is a
// 2 px lime ring growing from 3 to 28 px with an ease-out cubic and fading from 0.7.
#[test]
fn a_left_click_ripple_matches_the_macos_ripple() {
  let mut scene = scene();
  scene.apply(click(50.0, 50.0, MouseButton::Left), ms(0));

  let ripple_at = |scene: &mut MotionScene, now| {
    let frame = scene.frame(ms(now));
    let Layer::Outline(outline) = frame.overlay.layers()[0].clone() else {
      panic!("ripple expected at {now} ms");
    };
    outline
  };
  let start = ripple_at(&mut scene, 0);
  assert_eq!(start.rect().size.width, 6.0);
  assert_eq!(start.style().stroke.width, 2.0);
  assert_eq!(start.style().stroke.color, Color::AUV_LIME.with_alpha(0.7));

  let half = ripple_at(&mut scene, 225);
  assert!((half.rect().size.width / 2.0 - (3.0 + 25.0 * (1.0 - 0.5f64.powi(3)))).abs() < 1e-9);
  assert!((half.style().stroke.color.alpha - 0.35).abs() < 1e-9);
}

#[test]
fn a_ripple_grows_and_fades_then_leaves_the_scene() {
  let mut scene = scene();
  scene.apply(click(50.0, 50.0, MouseButton::Left), ms(0));

  let ripple_at = |scene: &mut MotionScene, now| {
    let frame = scene.frame(ms(now));
    let Layer::Outline(outline) = frame.overlay.layers()[0].clone() else {
      panic!("ripple expected at {now} ms");
    };
    (outline.rect().size.width, outline.style().stroke.color.alpha)
  };
  let (early_size, early_alpha) = ripple_at(&mut scene, 100);
  let (late_size, late_alpha) = ripple_at(&mut scene, 400);
  assert!(late_size > early_size, "ripple must expand");
  assert!(late_alpha < early_alpha, "ripple must fade");

  let done = scene.frame(ms(450));
  assert_eq!(layer_kinds(done.overlay.layers()), ["cursor"]);
  assert_eq!(done.wake, Wake::After(ms(2950)), "only the cursor's own fade is left");
}

#[test]
fn every_click_gets_a_ripple_even_when_reported_faster_than_the_cursor_travels() {
  let mut scene = scene();
  for (index, x) in [100.0, 400.0, 700.0].into_iter().enumerate() {
    scene.apply(click(x, 20.0, MouseButton::Left), ms(index as u64 * 40));
  }
  let frame = scene.frame(ms(100));
  let ripples = frame.overlay.layers().iter().filter(|layer| matches!(layer, Layer::Outline(_))).count();
  assert_eq!(ripples, 3);
}

#[test]
fn live_ripples_are_bounded() {
  let mut scene = scene();
  for index in 0..40 {
    scene.apply(click(index as f64, 0.0, MouseButton::Left), ms(index));
  }
  let ripples = scene.frame(ms(40)).overlay.layers().iter().filter(|layer| matches!(layer, Layer::Outline(_))).count();
  assert_eq!(ripples, 16);
}

#[test]
fn right_and_middle_clicks_do_not_reuse_the_left_click_color() {
  let color = |button| {
    let mut scene = scene();
    scene.apply(click(0.0, 0.0, button), ms(0));
    let Layer::Outline(outline) = scene.frame(ms(10)).overlay.layers()[0].clone() else {
      panic!("ripple");
    };
    (outline.style().stroke.color.red, outline.style().stroke.color.green, outline.style().stroke.color.blue)
  };
  assert_ne!(color(MouseButton::Left), color(MouseButton::Right));
  assert_ne!(color(MouseButton::Left), color(MouseButton::Middle));
}

#[test]
fn two_targeted_windows_are_marked_at_once_and_stay_in_report_order() {
  let mut scene = scene();
  let first = Rect::new(100.0, 100.0, 800.0, 600.0);
  let second = Rect::new(1000.0, 200.0, 500.0, 400.0);
  scene.apply(
    ActionEvent::WindowTargeted {
      id: "11".into(),
      frame: first,
      label: Some("Notepad".into()),
    },
    ms(0),
  );
  scene.apply(targeted("22", second), ms(10));
  scene.apply(jump(150.0, 150.0), ms(20));

  let frame = scene.frame(ms(300));
  assert_eq!(layer_kinds(frame.overlay.layers()), ["outline", "status", "outline", "cursor"]);
  let Layer::Outline(outline) = &frame.overlay.layers()[0] else {
    panic!()
  };
  assert_eq!(outline.rect(), first);
  let Layer::Status(status) = &frame.overlay.layers()[1] else {
    panic!()
  };
  assert_eq!(status.text(), "Notepad");
  let Layer::Outline(other) = &frame.overlay.layers()[2] else {
    panic!()
  };
  assert_eq!(other.rect(), second);
}

#[test]
fn reporting_a_window_again_refreshes_its_mark_and_follows_it() {
  let mut scene = scene();
  scene.apply(targeted("11", Rect::new(0.0, 0.0, 100.0, 100.0)), ms(0));
  scene.apply(targeted("11", Rect::new(40.0, 40.0, 100.0, 100.0)), ms(2500));

  // 3.2 s after the first report, but only 0.7 s after the refresh.
  let frame = scene.frame(ms(3200));
  let [Layer::Outline(outline)] = frame.overlay.layers() else {
    panic!("one refreshed mark expected");
  };
  assert_eq!(outline.rect(), Rect::new(40.0, 40.0, 100.0, 100.0));
  assert_eq!(outline.style().stroke.color.alpha, 1.0);
}

#[test]
fn a_mark_waits_then_fades_then_expires_and_the_scene_sleeps_in_between() {
  let mut scene = scene();
  scene.apply(targeted("11", Rect::new(0.0, 0.0, 100.0, 100.0)), ms(0));

  let steady = scene.frame(ms(1000));
  assert_eq!(steady.wake, Wake::After(ms(1400)), "fade starts at 2.4 s");

  let fading = scene.frame(ms(2700));
  let [Layer::Outline(outline)] = fading.overlay.layers() else {
    panic!("mark still drawn while fading");
  };
  assert!(outline.style().stroke.color.alpha < 1.0 && outline.style().stroke.color.alpha > 0.0);
  assert_eq!(fading.wake, Wake::NextFrame);

  let gone = scene.frame(ms(3000));
  assert!(gone.overlay.layers().is_empty());
  assert_eq!(gone.wake, Wake::Idle);
}

#[test]
fn clear_forgets_the_cursor_ripples_and_marks() {
  let mut scene = scene();
  scene.apply(click(5.0, 5.0, MouseButton::Left), ms(0));
  scene.apply(targeted("1", Rect::new(0.0, 0.0, 10.0, 10.0)), ms(0));
  scene.set_status(None, Some("Working"), ms(0));
  scene.clear();
  assert!(scene.frame(ms(1)).overlay.layers().is_empty());
  assert_eq!(scene.cursor_point(None, ms(1)), None);
}

#[test]
fn each_window_gets_its_own_cursor_in_its_own_color() {
  let mut scene = scene();
  scene.apply(click_in("notes", 100.0, 100.0), ms(0));
  scene.apply(click_in("browser", 700.0, 100.0), ms(1000));

  let cursors = cursor_layers(&mut scene, 2000);
  assert_eq!(cursors.len(), 2, "the first window's cursor stays where it acted");
  assert_eq!(cursors[0].point(), at(100.0, 100.0));
  assert_eq!(cursors[1].point(), at(700.0, 100.0));
  assert_eq!(accent(&cursors[0]), CURSOR_COLORS[0].fill, "the first window takes AUV cyan");
  assert_eq!(accent(&cursors[1]), CURSOR_COLORS[1].fill, "the next window takes the next color");
  assert_ne!(accent(&cursors[0]), accent(&cursors[1]));
}

#[test]
fn screen_actions_and_window_actions_move_different_cursors() {
  let mut scene = scene();
  scene.apply(jump(50.0, 50.0), ms(0));
  scene.apply(jump_in("notes", 400.0, 300.0), ms(500));
  scene.apply(jump(60.0, 60.0), ms(1000));

  assert_eq!(scene.cursor_point(None, ms(2000)), Some(at(60.0, 60.0)));
  assert_eq!(scene.cursor_point(Some("notes"), ms(2000)), Some(at(400.0, 300.0)));
  assert_eq!(cursor_layers(&mut scene, 2000).len(), 2);
}

#[test]
fn a_new_window_cursor_sets_off_from_the_cursor_that_acted_last() {
  let mut scene = scene();
  scene.apply(jump_in("notes", 100.0, 100.0), ms(0));
  scene.apply(jump_in("browser", 700.0, 100.0), ms(1000));

  assert_eq!(scene.cursor_point(Some("browser"), ms(1000)), Some(at(100.0, 100.0)), "starts on the notes cursor");
  let mut previous = 100.0;
  for now in 1001..=2000 {
    let point = scene.cursor_point(Some("browser"), ms(now)).unwrap().point();
    assert!(point.x >= previous && point.x <= 700.0 && point.y == 100.0, "travels straight over at {now} ms");
    previous = point.x;
  }
  assert_eq!(previous, 700.0);
  assert_eq!(scene.cursor_point(Some("notes"), ms(2000)), Some(at(100.0, 100.0)), "the notes cursor stays put");

  let sizes: Vec<f64> = (1016..1300).step_by(16).map(|now| cursor_layer(&mut scene, now).pose().scale).collect();
  assert!(sizes[0] < 0.5 && sizes.iter().any(|scale| *scale > 1.0), "grows in as it leaves: {sizes:?}");
}

#[test]
fn the_cursor_that_acted_last_is_drawn_on_top() {
  let mut scene = scene();
  scene.apply(jump_in("notes", 100.0, 100.0), ms(0));
  scene.apply(jump_in("browser", 700.0, 100.0), ms(500));
  assert_eq!(cursor_layer(&mut scene, 1500).point(), at(700.0, 100.0));

  scene.apply(click_in("notes", 120.0, 100.0), ms(2000));
  assert_eq!(cursor_layer(&mut scene, 3000).point(), at(120.0, 100.0));
}

#[test]
fn a_quiet_window_cursor_shrinks_into_its_tip_and_leaves() {
  let mut scene = scene();
  scene.apply(jump_in("notes", 300.0, 200.0), ms(0));

  let steady = scene.frame(ms(3000));
  assert_eq!(steady.wake, Wake::After(ms(400)), "fades from 3.4 s");

  let leaving = cursor_layer(&mut scene, 3700);
  assert!((leaving.pose().scale - 0.75).abs() < 1e-9, "half way out: {}", leaving.pose().scale);
  assert_eq!(leaving.point(), at(300.0, 200.0), "shrinks about its tip");
  assert_eq!(scene.frame(ms(3800)).wake, Wake::NextFrame);

  let gone = scene.frame(ms(4000));
  assert!(gone.overlay.layers().is_empty());
  assert_eq!(gone.wake, Wake::Idle);
  assert_eq!(scene.cursor_point(Some("notes"), ms(4000)), None);
}

#[test]
fn a_window_keeps_its_color_when_its_cursor_comes_back() {
  let mut scene = scene();
  scene.apply(jump_in("notes", 100.0, 100.0), ms(0));
  scene.apply(jump_in("browser", 700.0, 100.0), ms(100));
  assert!(cursor_layers(&mut scene, 5000).is_empty(), "both cursors faded out");

  scene.apply(jump_in("notes", 100.0, 100.0), ms(6000));
  scene.apply(jump_in("repl", 400.0, 400.0), ms(6100));
  let cursors = cursor_layers(&mut scene, 7000);
  assert_eq!(accent(&cursors[0]), CURSOR_COLORS[0].fill, "notes is cyan again");
  assert_eq!(accent(&cursors[1]), CURSOR_COLORS[2].fill, "a third window takes the third color");
}

#[test]
fn a_window_mark_takes_its_cursor_color() {
  let mut scene = scene();
  scene.apply(targeted("notes", Rect::new(0.0, 0.0, 400.0, 300.0)), ms(0));
  scene.apply(click_in("notes", 100.0, 100.0), ms(0));
  scene.apply(targeted("browser", Rect::new(500.0, 0.0, 400.0, 300.0)), ms(1000));
  scene.apply(click_in("browser", 700.0, 100.0), ms(1000));

  let frame = scene.frame(ms(2000));
  let outlines: Vec<Color> = frame
    .overlay
    .layers()
    .iter()
    .filter_map(|layer| match layer {
      Layer::Outline(outline) if outline.rect().size.width > 100.0 => Some(outline.style().stroke.color),
      _ => None,
    })
    .collect();
  assert_eq!(outlines, [CURSOR_COLORS[0].fill, CURSOR_COLORS[1].fill]);
}

#[test]
fn cursors_are_bounded() {
  let mut scene = scene();
  for index in 0..40 {
    scene.apply(jump_in(&format!("window-{index}"), index as f64 * 10.0, 0.0), ms(index));
  }
  assert_eq!(cursor_layers(&mut scene, 1000).len(), 16);
}

#[test]
fn a_status_types_out_beside_its_cursor_in_its_color() {
  let mut scene = scene();
  scene.apply(jump_in("repl", 300.0, 200.0), ms(0));
  scene.set_status(Some("repl"), Some("Recording the run"), ms(1000));

  let first = cursor_layer(&mut scene, 1000);
  assert!(first.label_visible());
  assert_eq!(first.label(), Some("R"), "the first character shows at once");
  assert_eq!(first.style().label_background.alpha, 0.0, "and the pill fades in");

  // One character per 28 ms.
  assert_eq!(cursor_layer(&mut scene, 1140).label(), Some("Record"));
  assert_eq!(scene.frame(ms(1200)).wake, Wake::NextFrame, "still typing");

  let typed = cursor_layer(&mut scene, 1500);
  assert_eq!(typed.label(), Some("Recording the run"));
  assert_eq!(typed.style().label_background, CURSOR_COLORS[0].fill);
  assert_eq!(typed.style().label_foreground, CURSOR_COLORS[0].ink);
  assert_eq!(scene.frame(ms(1600)).wake, Wake::After(ms(2800)), "still until the cursor fades");
}

#[test]
fn a_new_status_retypes_in_a_pill_that_stays_up() {
  let mut scene = scene();
  scene.apply(jump_in("repl", 300.0, 200.0), ms(0));
  scene.set_status(Some("repl"), Some("Opening the REPL"), ms(500));
  scene.set_status(Some("repl"), Some("Recording the run"), ms(1500));

  let replaced = cursor_layer(&mut scene, 1500);
  assert_eq!(replaced.label(), Some("R"));
  assert_eq!(replaced.style().label_background.alpha, 1.0);
}

#[test]
fn a_status_keeps_its_cursor_from_fading_out() {
  let mut scene = scene();
  scene.apply(jump_in("notes", 300.0, 200.0), ms(0));
  scene.set_status(Some("notes"), Some("Reading the page"), ms(3000));

  assert_eq!(cursor_layer(&mut scene, 5000).pose(), CursorPose::REST, "4 s after the action, 2 s after the status");
  assert!(cursor_layers(&mut scene, 7000).is_empty(), "gone 4 s after the status");
}

#[test]
fn a_status_set_before_the_first_action_appears_with_its_cursor() {
  let mut scene = scene();
  scene.set_status(Some("repl"), Some("Opening the REPL"), ms(0));
  let waiting = scene.frame(ms(100));
  assert!(waiting.overlay.layers().is_empty(), "no cursor to show it beside yet");
  assert_eq!(waiting.wake, Wake::Idle);

  scene.apply(click_in("repl", 300.0, 200.0), ms(500));
  let appeared = cursor_layer(&mut scene, 520);
  assert_eq!(appeared.label(), Some("O"), "starts typing when the cursor appears");
}

#[test]
fn clearing_a_status_removes_its_pill() {
  let mut scene = scene();
  scene.apply(jump(300.0, 200.0), ms(0));
  scene.set_status(None, Some("Working"), ms(500));
  scene.set_status(None, None, ms(1000));
  assert!(!cursor_layer(&mut scene, 1100).label_visible());

  scene.set_status(None, Some("Working"), ms(1200));
  scene.set_status(None, Some("  "), ms(1300));
  assert!(!cursor_layer(&mut scene, 1400).label_visible(), "a blank status clears too");
}

#[test]
fn a_status_is_shown_on_one_line_and_cut_to_a_bounded_length() {
  let mut scene = scene();
  scene.apply(jump(300.0, 200.0), ms(0));

  scene.set_status(None, Some(" Opening\nthe REPL "), ms(500));
  assert_eq!(cursor_layer(&mut scene, 2000).label(), Some("Opening the REPL"));

  let long = "x".repeat(100);
  scene.set_status(None, Some(&long), ms(2000));
  let label = cursor_layer(&mut scene, 4000).label().unwrap().to_string();
  assert_eq!(label.chars().count(), 48);
  assert!(label.ends_with('…'), "{label}");
}
