use std::time::Duration;

use auv_driver_common::geometry::{CoordinateSpace, Point, Rect, ScreenPoint};
use auv_driver_common::mouse_input::MotionEvent;
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{Click, InputTarget, MouseButton, MouseMotionSample};
use auv_driver_overlay::{ActionEvent, ShowOptions, Travel};

use super::*;

fn window(title: Option<&str>, app: Option<&str>) -> Window {
  Window {
    reference: WindowRef {
      id: "4242".to_string(),
    },
    title: title.map(str::to_string),
    app_name: app.map(str::to_string),
    app_bundle_id: None,
    process_id: Some(7),
    frame: Rect::new(100.0, 120.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

fn started(x: f64, y: f64) -> MotionEvent {
  MotionEvent::Started {
    point: Point::new(x, y),
    samples: 10,
    duration: Duration::from_millis(100),
  }
}

fn progress(index: u64, x: f64, y: f64) -> MotionEvent {
  MotionEvent::Progress {
    index,
    sample: MouseMotionSample {
      point: Point::new(x, y),
      elapsed: Duration::from_millis(index * 10),
    },
  }
}

#[test]
fn a_click_request_reports_one_click_per_press_at_the_delivered_point() {
  let point = Point::new(310.0, 220.0);
  let double = Click::Double {
    interval: Duration::from_millis(75),
  };
  let triple = Click::Repeated {
    count: 3,
    interval: Duration::from_millis(75),
  };

  for (click, presses) in [(Click::Single, 1), (double, 2), (triple, 3)] {
    let events = clicked(point, MouseButton::Right, &click);
    assert_eq!(events.len(), presses);
    assert!(events.iter().all(|event| {
      *event
        == ActionEvent::Clicked {
          point: ScreenPoint(point),
          button: MouseButton::Right,
        }
    }));
  }
}

#[test]
fn a_targeted_window_is_reported_with_its_id_frame_and_title() {
  let event = window_targeted(&window(Some("notes.txt - Notepad"), Some("notepad.exe")));
  assert_eq!(
    event,
    ActionEvent::WindowTargeted {
      id: "4242".to_string(),
      frame: Rect::new(100.0, 120.0, 800.0, 600.0),
      label: Some("notes.txt - Notepad".to_string()),
    }
  );
}

#[test]
fn an_untitled_window_is_labelled_by_its_app() {
  for title in [None, Some("")] {
    let ActionEvent::WindowTargeted { label, .. } = window_targeted(&window(title, Some("notepad.exe"))) else {
      panic!("window event expected");
    };
    assert_eq!(label.as_deref(), Some("notepad.exe"));
  }
  let ActionEvent::WindowTargeted { label, .. } = window_targeted(&window(None, None)) else {
    panic!("window event expected");
  };
  assert_eq!(label, None, "no label is better than an invented one");
}

fn moved(x: f64, y: f64, travel: Travel) -> ActionEvent {
  ActionEvent::Moved {
    point: ScreenPoint::new(x, y),
    travel,
  }
}

#[test]
fn a_movement_reports_nothing_until_it_has_delivered_a_sample() {
  let mut reporter = MotionReporter::new(None);
  assert_eq!(reporter.event(&started(10.0, 10.0)), vec![], "starting is not delivering");
}

#[test]
fn the_first_delivered_sample_jumps_and_later_samples_are_followed_directly() {
  let mut reporter = MotionReporter::new(None);
  reporter.event(&started(10.0, 10.0));

  assert_eq!(reporter.event(&progress(0, 10.0, 10.0)), vec![moved(10.0, 10.0, Travel::Jump)]);
  assert_eq!(reporter.event(&progress(1, 24.0, 18.0)), vec![moved(24.0, 18.0, Travel::Sampled)]);
  assert_eq!(reporter.event(&progress(4, 90.0, 50.0)), vec![moved(90.0, 50.0, Travel::Sampled)], "skipped samples report the delivered one");
}

#[test]
fn every_new_movement_starts_with_a_jump_again() {
  let mut reporter = MotionReporter::new(None);
  reporter.event(&started(0.0, 0.0));
  reporter.event(&progress(0, 0.0, 0.0));
  reporter.event(&progress(1, 5.0, 5.0));

  reporter.event(&started(400.0, 300.0));
  assert_eq!(reporter.event(&progress(0, 400.0, 300.0)), vec![moved(400.0, 300.0, Travel::Jump)]);
}

#[test]
fn a_movement_aimed_at_a_window_marks_it_once_with_its_first_delivered_sample() {
  let target = window(Some("Editor"), Some("editor.exe"));
  let mut reporter = MotionReporter::new(Some(&InputTarget::Window(target.clone())));
  assert_eq!(reporter.event(&started(10.0, 10.0)), vec![], "nothing is marked before anything is delivered");

  assert_eq!(reporter.event(&progress(0, 10.0, 10.0)), vec![window_targeted(&target), moved(10.0, 10.0, Travel::Jump)]);
  assert_eq!(reporter.event(&progress(1, 20.0, 10.0)), vec![moved(20.0, 10.0, Travel::Sampled)], "the mark is not repeated per sample");
}

#[test]
fn a_foreground_movement_marks_no_window() {
  let mut reporter = MotionReporter::new(Some(&InputTarget::Foreground));
  assert_eq!(reporter.event(&progress(0, 1.0, 2.0)), vec![moved(1.0, 2.0, Travel::Jump)]);
}

#[test]
fn a_pointer_warp_is_a_jump_to_the_delivered_point() {
  assert_eq!(
    moved_to(Point::new(5.0, 6.0)),
    ActionEvent::Moved {
      point: ScreenPoint::new(5.0, 6.0),
      travel: Travel::Jump,
    }
  );
}

#[test]
fn reporting_without_a_follower_is_a_no_op_and_only_one_follower_runs_at_a_time() {
  report([moved_to(Point::new(1.0, 1.0))]);

  let first = follow(ShowOptions::new()).expect("first follower starts");
  let second = follow(ShowOptions::new());
  assert!(second.is_err(), "a second follower must be rejected, not replace the first");

  let stats = first.stop().expect("stop returns the run's stats");
  assert_eq!(stats.frames, 0, "nothing was reported, so nothing was drawn");

  let again = follow(ShowOptions::new()).expect("stopping frees the slot");
  drop(again);
}
