use std::time::Duration;

use auv_driver_common::{MouseButton, Rect, ScreenPoint};

use super::{ActionEvent, MotionScene, Travel, Wake};
use crate::layers::{BuiltInCursor, CursorImage};
use crate::{Easing, Layer, MotionOptions};

fn ms(value: u64) -> Duration {
  Duration::from_millis(value)
}

fn at(x: f64, y: f64) -> ScreenPoint {
  ScreenPoint::new(x, y)
}

fn scene() -> MotionScene {
  MotionScene::new(MotionOptions::new())
}

fn jump(x: f64, y: f64) -> ActionEvent {
  ActionEvent::Moved {
    point: at(x, y),
    travel: Travel::Jump,
  }
}

fn sample(x: f64, y: f64) -> ActionEvent {
  ActionEvent::Moved {
    point: at(x, y),
    travel: Travel::Sampled,
  }
}

fn cursor_x(scene: &MotionScene, now: Duration) -> f64 {
  scene.cursor_point(now).expect("cursor placed").point().x
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

// The shared easing contract is the macOS renderer's `easeInOutExpo` in
// `Overlay.swift`. These values pin the Rust evaluation to the same function.
#[test]
fn ease_in_out_expo_matches_the_macos_contract() {
  let easing = Easing::EaseInOutExpo;
  assert_eq!(easing.apply(0.0), 0.0);
  assert_eq!(easing.apply(1.0), 1.0);
  assert_eq!(easing.apply(0.25), 2f64.powi(-5) / 2.0);
  assert_eq!(easing.apply(0.75), (2.0 - 2f64.powi(-5)) / 2.0);
  assert!((easing.apply(0.5) - 0.5).abs() < 1e-12);
}

#[test]
fn ease_in_out_expo_clamps_out_of_range_progress_and_rejects_nan() {
  let easing = Easing::EaseInOutExpo;
  assert_eq!(easing.apply(-3.0), 0.0);
  assert_eq!(easing.apply(7.0), 1.0);
  assert_eq!(easing.apply(f64::NAN), 0.0);
}

#[test]
fn ease_in_out_expo_never_moves_backwards() {
  let easing = Easing::EaseInOutExpo;
  let mut previous = 0.0;
  for step in 0..=2000 {
    let value = easing.apply(step as f64 / 2000.0);
    assert!(value >= previous, "eased progress dropped at step {step}: {value} < {previous}");
    previous = value;
  }
}

#[test]
fn motion_progress_follows_the_configured_duration_and_zero_has_arrived() {
  let motion = MotionOptions::new().with_duration(ms(200));
  assert_eq!(motion.progress(ms(0)), 0.0);
  assert!((motion.progress(ms(100)) - 0.5).abs() < 1e-12);
  assert_eq!(motion.progress(ms(200)), 1.0);
  assert_eq!(motion.progress(ms(900)), 1.0);
  assert_eq!(MotionOptions::new().with_duration(Duration::ZERO).progress(ms(0)), 1.0);
}

#[test]
fn an_empty_scene_draws_nothing_and_stays_idle() {
  let mut scene = scene();
  let frame = scene.frame(ms(500));
  assert!(frame.overlay.layers().is_empty());
  assert_eq!(frame.wake, Wake::Idle);
  assert_eq!(scene.cursor_point(ms(500)), None);
}

#[test]
fn the_first_report_places_the_cursor_without_gliding_from_anywhere() {
  let mut scene = scene();
  scene.apply(jump(120.0, 80.0), ms(10));
  assert_eq!(scene.cursor_point(ms(10)), Some(at(120.0, 80.0)));
  assert_eq!(scene.frame(ms(10)).wake, Wake::Idle);
}

#[test]
fn a_jump_eases_to_the_reported_point_and_never_teleports() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(100.0, 0.0), ms(1000));

  assert_eq!(cursor_x(&scene, ms(1000)), 0.0);
  assert!((cursor_x(&scene, ms(1160)) - 50.0).abs() < 1e-9, "midpoint of the default 320 ms ease");
  assert_eq!(cursor_x(&scene, ms(1320)), 100.0);
  assert_eq!(cursor_x(&scene, ms(5000)), 100.0);

  // The steepest part of the curve moves about 2.2 px per millisecond over 100 px.
  let mut previous = cursor_x(&scene, ms(1000));
  for now in 1001..=1320 {
    let x = cursor_x(&scene, ms(now));
    assert!(x >= previous, "moved backwards at {now} ms");
    assert!(x - previous < 2.5, "jumped {} px in one millisecond at {now} ms", x - previous);
    previous = x;
  }
}

#[test]
fn a_new_jump_mid_flight_departs_from_where_the_cursor_is_drawn() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(200.0, 0.0), ms(1000));
  let drawn = cursor_x(&scene, ms(1200));
  assert!(drawn > 0.0 && drawn < 200.0);

  scene.apply(jump(0.0, 100.0), ms(1200));
  assert_eq!(cursor_x(&scene, ms(1200)), drawn, "retargeting must not move the cursor at the retarget instant");
  let arrived = scene.cursor_point(ms(1200 + 320)).unwrap();
  assert_eq!(arrived, at(0.0, 100.0));
}

#[test]
fn sampled_positions_are_drawn_without_added_delay() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  for (index, x) in [10.0, 25.0, 45.0, 70.0].into_iter().enumerate() {
    let now = ms(1000 + 16 * index as u64);
    scene.apply(sample(x, 0.0), now);
    assert_eq!(cursor_x(&scene, now), x, "a sample must be on screen the instant it is reported");
  }
}

#[test]
fn a_sample_during_a_glide_keeps_the_cursor_continuous_and_converges_on_it() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(jump(100.0, 0.0), ms(1000));
  let before = cursor_x(&scene, ms(1200));

  scene.apply(sample(104.0, 0.0), ms(1200));
  let after = cursor_x(&scene, ms(1200));
  assert!((after - before).abs() <= 4.0, "a sample moved the cursor by more than the target moved");
  assert_eq!(cursor_x(&scene, ms(1320)), 104.0);
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
    let point = scene.cursor_point(ms(now)).unwrap().point();
    assert!((0.0..=500.0).contains(&point.x) && (0.0..=500.0).contains(&point.y), "drew at ({}, {}) at {now} ms", point.x, point.y);
  }
}

#[test]
fn a_click_ripples_at_the_reported_point_at_the_reported_time() {
  let mut scene = scene();
  scene.apply(jump(0.0, 0.0), ms(0));
  scene.apply(
    ActionEvent::Clicked {
      point: at(200.0, 100.0),
      button: MouseButton::Left,
    },
    ms(1000),
  );

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

#[test]
fn a_click_shows_the_pressed_cursor_only_briefly() {
  let mut scene = scene();
  scene.apply(
    ActionEvent::Clicked {
      point: at(10.0, 10.0),
      button: MouseButton::Left,
    },
    ms(0),
  );
  let variant = |scene: &mut MotionScene, now| {
    let frame = scene.frame(ms(now));
    let Some(Layer::Cursor(cursor)) = frame.overlay.layers().last() else {
      panic!("cursor is drawn last");
    };
    match cursor.image() {
      CursorImage::BuiltIn { variant } => *variant,
      CursorImage::Svg { .. } => panic!("scene uses built-in art"),
    }
  };
  assert_eq!(variant(&mut scene, 50), BuiltInCursor::AuvClick);
  assert_eq!(variant(&mut scene, 300), BuiltInCursor::Auv);
}

#[test]
fn a_ripple_grows_and_fades_then_leaves_the_scene() {
  let mut scene = scene();
  scene.apply(
    ActionEvent::Clicked {
      point: at(50.0, 50.0),
      button: MouseButton::Left,
    },
    ms(0),
  );

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
  assert_eq!(done.wake, Wake::Idle);
}

#[test]
fn every_click_gets_a_ripple_even_when_reported_faster_than_the_glide() {
  let mut scene = scene();
  for (index, x) in [100.0, 400.0, 700.0].into_iter().enumerate() {
    scene.apply(
      ActionEvent::Clicked {
        point: at(x, 20.0),
        button: MouseButton::Left,
      },
      ms(index as u64 * 40),
    );
  }
  let frame = scene.frame(ms(100));
  let ripples = frame.overlay.layers().iter().filter(|layer| matches!(layer, Layer::Outline(_))).count();
  assert_eq!(ripples, 3);
}

#[test]
fn live_ripples_are_bounded() {
  let mut scene = scene();
  for index in 0..40 {
    scene.apply(
      ActionEvent::Clicked {
        point: at(index as f64, 0.0),
        button: MouseButton::Left,
      },
      ms(index),
    );
  }
  let ripples = scene.frame(ms(40)).overlay.layers().iter().filter(|layer| matches!(layer, Layer::Outline(_))).count();
  assert_eq!(ripples, 16);
}

#[test]
fn right_and_middle_clicks_do_not_reuse_the_left_click_color() {
  let color = |button| {
    let mut scene = scene();
    scene.apply(
      ActionEvent::Clicked {
        point: at(0.0, 0.0),
        button,
      },
      ms(0),
    );
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
  scene.apply(
    ActionEvent::WindowTargeted {
      id: "22".into(),
      frame: second,
      label: None,
    },
    ms(10),
  );
  scene.apply(jump(150.0, 150.0), ms(20));

  let frame = scene.frame(ms(30));
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
  let id = "11".to_string();
  scene.apply(
    ActionEvent::WindowTargeted {
      id: id.clone(),
      frame: Rect::new(0.0, 0.0, 100.0, 100.0),
      label: None,
    },
    ms(0),
  );
  scene.apply(
    ActionEvent::WindowTargeted {
      id,
      frame: Rect::new(40.0, 40.0, 100.0, 100.0),
      label: None,
    },
    ms(2500),
  );

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
  scene.apply(
    ActionEvent::WindowTargeted {
      id: "11".into(),
      frame: Rect::new(0.0, 0.0, 100.0, 100.0),
      label: None,
    },
    ms(0),
  );

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
  scene.apply(
    ActionEvent::Clicked {
      point: at(5.0, 5.0),
      button: MouseButton::Left,
    },
    ms(0),
  );
  scene.apply(
    ActionEvent::WindowTargeted {
      id: "1".into(),
      frame: Rect::new(0.0, 0.0, 10.0, 10.0),
      label: None,
    },
    ms(0),
  );
  scene.clear();
  assert!(scene.frame(ms(1)).overlay.layers().is_empty());
  assert_eq!(scene.cursor_point(ms(1)), None);
}
