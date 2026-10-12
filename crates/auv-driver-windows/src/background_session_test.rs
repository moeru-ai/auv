use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use auv_driver_common::geometry::{CoordinateSpace, Point, Rect, WindowPoint};
use auv_driver_common::input::{Click, ClickModifiers, InputDeliveryPath, MouseButton, Scroll};
use auv_driver_common::window::{Window, WindowRef};
use auv_driver_common::{DriverError, DriverResult};
use image::RgbaImage;

use super::*;

/// The operating system, as the session sees it: which windows exist and what was sent to them.
#[derive(Default)]
struct World {
  windows: HashMap<isize, WindowProbe>,
  probes: usize,
  clicks: Vec<(isize, Point, ClickRequest)>,
  scrolls: Vec<(isize, Point)>,
  /// Handles a stream was opened for, in order.
  opened: Vec<isize>,
  /// Makes every post fail with this message.
  refuse_posts: Option<String>,
  streams: HashMap<isize, Script>,
}

#[derive(Default)]
struct Script {
  /// Frames the compositor has delivered.
  delivered: u64,
  /// Frames a reader has taken.
  read: u64,
  /// Errors the next reads return, front first.
  faults: Vec<StreamError>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<World>>);

impl Fake {
  fn world(&self) -> std::sync::MutexGuard<'_, World> {
    self.0.lock().unwrap()
  }

  fn open_window(&self, hwnd: isize, process_id: u32, frame: Rect) {
    self.world().windows.insert(
      hwnd,
      WindowProbe {
        exists: true,
        process_id,
        thread_id: process_id + 1,
        class_name: "Notepad".into(),
        minimized: false,
        cloaked: false,
        frame: Some(frame),
      },
    );
  }

  fn close_window(&self, hwnd: isize) {
    self.world().windows.remove(&hwnd);
  }

  fn edit(&self, hwnd: isize, change: impl FnOnce(&mut WindowProbe)) {
    change(self.world().windows.get_mut(&hwnd).expect("window exists"));
  }

  /// The window repaints: the compositor delivers one more frame.
  fn repaint(&self, hwnd: isize) {
    self.world().streams.entry(hwnd).or_default().delivered += 1;
  }
}

struct FakeFrames(Fake, isize);

impl FrameSource for FakeFrames {
  fn read(&mut self) -> Result<FrameUpdate, StreamError> {
    let mut world = self.0.world();
    let script = world.streams.entry(self.1).or_default();
    if !script.faults.is_empty() {
      return Err(script.faults.remove(0));
    }
    if script.delivered == script.read {
      return Ok(FrameUpdate::Unchanged {
        frame_number: script.delivered,
      });
    }
    script.read = script.delivered;
    Ok(FrameUpdate::Fresh {
      image: RgbaImage::new(8, 6),
      frame_number: script.delivered,
    })
  }

  fn frames_delivered(&self) -> u64 {
    self.0.world().streams.get(&self.1).map(|script| script.delivered).unwrap_or(0)
  }
}

impl Backend for Fake {
  fn probe(&self, hwnd: isize) -> WindowProbe {
    let mut world = self.world();
    world.probes += 1;
    world.windows.get(&hwnd).cloned().unwrap_or_else(WindowProbe::gone)
  }

  fn post_click(&self, window: &Window, screen_point: Point, request: &ClickRequest) -> DriverResult<()> {
    let hwnd: isize = window.reference.id.parse().unwrap();
    let mut world = self.world();
    world.clicks.push((hwnd, screen_point, request.clone()));
    match &world.refuse_posts {
      Some(message) => Err(DriverError::Backend {
        message: message.clone(),
      }),
      None => Ok(()),
    }
  }

  fn post_scroll(&self, window: &Window, screen_point: Point, _scroll: Scroll) -> DriverResult<()> {
    let hwnd: isize = window.reference.id.parse().unwrap();
    let mut world = self.world();
    world.scrolls.push((hwnd, screen_point));
    match &world.refuse_posts {
      Some(message) => Err(DriverError::Backend {
        message: message.clone(),
      }),
      None => Ok(()),
    }
  }

  fn open_frames(&self, window: &Window) -> DriverResult<Box<dyn FrameSource>> {
    let hwnd: isize = window.reference.id.parse().unwrap();
    self.world().opened.push(hwnd);
    Ok(Box::new(FakeFrames(self.clone(), hwnd)))
  }
}

const FRAME: Rect = Rect::new(100.0, 200.0, 800.0, 600.0);

fn window(hwnd: isize, process_id: u32) -> Window {
  Window {
    reference: WindowRef {
      id: hwnd.to_string(),
    },
    title: Some(format!("window {hwnd}")),
    app_name: None,
    application_id: None,
    process_id: Some(process_id),
    // Stale on purpose: the session must trust the live frame, not the one it was handed.
    frame: Rect::new(0.0, 0.0, 1.0, 1.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
  }
}

/// A session over `count` windows (handles 101, 102, ...), and the world they live in.
fn session_with(count: usize) -> (BackgroundSession, Fake, Vec<TargetId>) {
  let fake = Fake::default();
  let mut session = BackgroundSession::with_backend(Box::new(fake.clone()));
  let mut ids = Vec::new();
  for index in 0..count {
    let hwnd = 101 + index as isize;
    fake.open_window(hwnd, 1000 + index as u32 * 10, FRAME);
    ids.push(session.attach(&window(hwnd, 1000 + index as u32 * 10)).unwrap());
  }
  (session, fake, ids)
}

fn left_click() -> ClickRequest {
  ClickRequest::default()
}

#[test]
fn a_session_targets_at_most_five_windows() {
  let (mut session, fake, _) = session_with(MAX_TARGETS);
  fake.open_window(900, 2000, FRAME);

  assert_eq!(session.attach(&window(900, 2000)), Err(AttachError::Full));
  assert_eq!(session.targets().len(), 5);
}

#[test]
fn a_window_is_a_target_once_and_a_dead_or_foreign_handle_is_refused() {
  let (mut session, fake, ids) = session_with(1);
  fake.open_window(900, 2000, FRAME);

  assert_eq!(session.attach(&window(101, 1000)), Err(AttachError::AlreadyAttached(ids[0])));
  assert_eq!(session.attach(&window(404, 1)), Err(AttachError::Gone));
  // The window says process 7 owns the handle; the OS says 2000 does.
  assert_eq!(session.attach(&window(900, 7)), Err(AttachError::Gone));
  let mut not_a_handle = window(900, 2000);
  not_a_handle.reference.id = "not-a-number".into();
  assert_eq!(session.attach(&not_a_handle), Err(AttachError::NotAWindowHandle("not-a-number".into())));
}

#[test]
fn ids_are_never_reused_so_a_stale_id_names_nothing() {
  let (mut session, fake, ids) = session_with(1);
  assert!(session.detach(ids[0]));
  fake.open_window(900, 2000, FRAME);
  let replacement = session.attach(&window(900, 2000)).unwrap();

  assert_ne!(replacement, ids[0]);
  assert!(matches!(
    session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()),
    Outcome::Failed {
      operation: "click",
      ..
    }
  ));
  assert!(fake.world().clicks.is_empty());
}

#[test]
fn a_click_is_resolved_against_the_live_frame_and_follows_a_window_that_moved() {
  let (mut session, fake, ids) = session_with(1);

  let first = session.click(ids[0], WindowPoint::new(10.0, 20.0), left_click()).done().unwrap();
  assert_eq!(first.frame, FRAME);
  assert_eq!(first.followed, None);
  assert_eq!(fake.world().clicks[0].1, Point::new(110.0, 220.0));

  // The window is dragged and resized between two operations.
  let moved = Rect::new(500.0, 50.0, 640.0, 480.0);
  fake.edit(101, |probe| probe.frame = Some(moved));
  let second = session.click(ids[0], WindowPoint::new(10.0, 20.0), left_click()).done().unwrap();

  assert_eq!(fake.world().clicks[1].1, Point::new(510.0, 70.0));
  assert_eq!(second.frame, moved);
  assert_eq!(
    second.followed,
    Some(FrameChange {
      from: FRAME,
      to: moved
    })
  );
  // Following is reported once: the next operation finds the window where it already is.
  assert_eq!(session.click(ids[0], WindowPoint::new(10.0, 20.0), left_click()).done().unwrap().followed, None);
}

#[test]
fn a_click_reports_a_window_targeted_delivery_that_is_not_verified() {
  let (mut session, _fake, ids) = session_with(1);

  let delivery = session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()).done().unwrap();

  assert_eq!(delivery.result.selected_path, InputDeliveryPath::WindowTargetedMouse);
  assert!(!delivery.result.verified, "a posted message is not proof the application acted on it");
  assert_eq!(delivery.result.attempts.len(), 1);
}

#[test]
fn a_scroll_posts_a_wheel_to_the_followed_point() {
  let (mut session, fake, ids) = session_with(1);
  fake.edit(101, |probe| probe.frame = Some(Rect::new(0.0, 0.0, 300.0, 200.0)));

  let delivery = session.scroll(ids[0], WindowPoint::new(50.0, 60.0), Scroll::new(0.0, 300.0)).done().unwrap();

  assert_eq!(delivery.result.selected_path, InputDeliveryPath::WindowTargetedWheel);
  assert_eq!(fake.world().scrolls, vec![(101, Point::new(50.0, 60.0))]);
}

#[test]
fn a_minimized_target_is_skipped_never_restored_and_operable_again_when_the_user_restores_it() {
  let (mut session, fake, ids) = session_with(1);
  fake.edit(101, |probe| {
    probe.minimized = true;
    // Windows reports a minimized window parked far off screen.
    probe.frame = Some(Rect::new(-32000.0, -32000.0, 160.0, 28.0));
  });

  assert_eq!(session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()), Outcome::SkippedMinimized);
  assert_eq!(session.scroll(ids[0], WindowPoint::new(1.0, 1.0), Scroll::new(0.0, 100.0)), Outcome::SkippedMinimized);
  assert!(matches!(session.capture(ids[0]), Outcome::SkippedMinimized));
  assert!(fake.world().clicks.is_empty() && fake.world().scrolls.is_empty() && fake.world().opened.is_empty());
  assert_eq!(session.targets()[0].state, TargetState::Minimized);
  assert_eq!(session.targets()[0].frame, FRAME, "the parked off-screen frame must not become the target frame");
  assert_eq!(session.stats().skipped_minimized, 3);

  fake.edit(101, |probe| {
    probe.minimized = false;
    probe.frame = Some(FRAME);
  });
  assert!(session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()).is_done());
  assert_eq!(session.targets()[0].state, TargetState::Live);
}

#[test]
fn a_closed_target_is_lost_and_the_other_targets_carry_on() {
  let (mut session, fake, ids) = session_with(3);
  fake.close_window(102);

  let lost = Outcome::TargetLost {
    reason: LostReason::WindowDestroyed,
  };
  assert_eq!(session.click(ids[1], WindowPoint::new(1.0, 1.0), left_click()), lost);
  assert!(session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()).is_done());
  assert!(session.click(ids[2], WindowPoint::new(1.0, 1.0), left_click()).is_done());
  // Lost is final, and it is counted once however often it is asked about.
  assert_eq!(session.scroll(ids[1], WindowPoint::new(1.0, 1.0), Scroll::new(0.0, 100.0)), lost);
  assert!(matches!(session.capture(ids[1]), Outcome::TargetLost { .. }));
  assert_eq!(session.stats().lost, 1);
  assert_eq!(
    session.targets()[1].state,
    TargetState::Lost {
      reason: LostReason::WindowDestroyed
    }
  );
  assert_eq!(fake.world().clicks.iter().map(|click| click.0).collect::<Vec<_>>(), vec![101, 103]);

  // A new window that happens to get the dead window's handle does not revive the target.
  fake.open_window(102, 1010, FRAME);
  assert_eq!(session.click(ids[1], WindowPoint::new(1.0, 1.0), left_click()), lost);
}

#[test]
fn a_handle_reused_by_another_window_is_a_different_window_not_the_target() {
  let (mut session, fake, ids) = session_with(2);
  // Handle 101 now belongs to another process; 102 to another window of the same process.
  fake.edit(101, |probe| probe.process_id = 5555);
  fake.edit(102, |probe| probe.class_name = "SomethingElse".into());

  assert_eq!(
    session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click()),
    Outcome::TargetLost {
      reason: LostReason::ProcessChanged
    }
  );
  assert_eq!(
    session.click(ids[1], WindowPoint::new(1.0, 1.0), left_click()),
    Outcome::TargetLost {
      reason: LostReason::HandleReused
    }
  );
  assert!(fake.world().clicks.is_empty(), "nothing may be posted to a window that is not the target");
}

#[test]
fn alt_and_meta_clicks_are_unsupported_in_the_background_and_post_nothing() {
  let (mut session, fake, ids) = session_with(1);
  for modifiers in [
    ClickModifiers {
      alt: true,
      ..Default::default()
    },
    ClickModifiers {
      meta: true,
      ..Default::default()
    },
  ] {
    let outcome = session.click(
      ids[0],
      WindowPoint::new(1.0, 1.0),
      ClickRequest {
        modifiers,
        ..left_click()
      },
    );
    assert!(
      matches!(
        outcome,
        Outcome::Unsupported {
          operation: "click",
          ..
        }
      ),
      "{outcome:?}"
    );
  }
  assert!(fake.world().clicks.is_empty());
  // Shift and Control are what a posted mouse message can carry.
  let shifted = ClickModifiers {
    shift: true,
    control: true,
    ..Default::default()
  };
  let request = ClickRequest {
    modifiers: shifted,
    button: MouseButton::Right,
    click: Click::Single,
  };
  assert!(session.click(ids[0], WindowPoint::new(1.0, 1.0), request).is_done());
}

#[test]
fn a_refused_post_is_reported_after_one_attempt_and_never_retried_through_another_route() {
  let (mut session, fake, ids) = session_with(1);
  fake.world().refuse_posts = Some("PostMessageW failed: access denied".into());

  let click = session.click(ids[0], WindowPoint::new(1.0, 1.0), left_click());
  let scroll = session.scroll(ids[0], WindowPoint::new(1.0, 1.0), Scroll::new(0.0, 100.0));

  assert!(matches!(&click, Outcome::Failed { operation: "click", message } if message.contains("access denied")), "{click:?}");
  assert!(
    matches!(
      &scroll,
      Outcome::Failed {
        operation: "scroll",
        ..
      }
    ),
    "{scroll:?}"
  );
  // One attempt each, through the one background route. The backend has no other route to try.
  assert_eq!(fake.world().clicks.len(), 1);
  assert_eq!(fake.world().scrolls.len(), 1);
  assert_eq!(session.stats().clicks + session.stats().scrolls, 0, "a refused post is not a delivery");
}

#[test]
fn keyboard_requests_are_unsupported_and_touch_nothing() {
  let (mut session, fake, ids) = session_with(1);
  let probes_before = fake.world().probes;

  let typed = session.type_text(ids[0], "hello");
  let pressed = session.press_keys(ids[0], &["ctrl", "a"]);

  assert!(
    matches!(
      typed,
      Outcome::Unsupported {
        operation: "type_text",
        ..
      }
    ),
    "{typed:?}"
  );
  assert!(
    matches!(
      pressed,
      Outcome::Unsupported {
        operation: "press_keys",
        ..
      }
    ),
    "{pressed:?}"
  );
  let world = fake.world();
  assert!(world.clicks.is_empty() && world.scrolls.is_empty() && world.opened.is_empty());
  assert_eq!(world.probes, probes_before, "an unsupported operation does not even look at the window");
  assert_eq!(session.stats().unsupported, 2);
}

#[test]
fn an_unknown_target_is_a_typed_failure_for_every_operation() {
  let (mut session, fake, ids) = session_with(1);
  session.detach(ids[0]);

  assert!(matches!(
    session.type_text(ids[0], "x"),
    Outcome::Failed {
      operation: "type_text",
      ..
    }
  ));
  assert!(matches!(
    session.scroll(ids[0], WindowPoint::new(1.0, 1.0), Scroll::new(0.0, 1.0)),
    Outcome::Failed {
      operation: "scroll",
      ..
    }
  ));
  assert!(matches!(
    session.capture(ids[0]),
    Outcome::Failed {
      operation: "capture",
      ..
    }
  ));
  assert!(fake.world().scrolls.is_empty() && fake.world().opened.is_empty());
}

#[test]
fn every_operation_costs_one_handle_probe_and_never_a_window_enumeration() {
  let (mut session, fake, ids) = session_with(5);
  let after_attach = fake.world().probes;
  assert_eq!(after_attach, 5, "attach probes each window once");

  for round in 0..20 {
    for id in &ids {
      assert!(session.click(*id, WindowPoint::new(1.0, round as f64), left_click()).is_done());
    }
  }

  assert_eq!(fake.world().probes - after_attach, 100, "one probe per operation");
  assert_eq!(session.stats().probes, 105);
}

#[test]
fn each_target_gets_one_stream_that_is_reused_however_often_the_session_switches_windows() {
  let (mut session, fake, ids) = session_with(5);
  for hwnd in 101..=105 {
    fake.repaint(hwnd);
  }

  for _ in 0..30 {
    for id in &ids {
      assert!(session.capture(*id).is_done());
    }
  }

  let mut opened = fake.world().opened.clone();
  opened.sort_unstable();
  assert_eq!(opened, vec![101, 102, 103, 104, 105], "one stream per window, opened once");
  assert_eq!(session.stats().streams_opened, 5);
  assert_eq!(session.stats().streams_reopened, 0);
}

#[test]
fn a_window_that_has_not_changed_is_not_copied_again() {
  let (mut session, fake, ids) = session_with(1);
  fake.repaint(101);

  assert!(matches!(
    session.capture(ids[0]).done().unwrap(),
    FrameRead::Fresh {
      frame_number: 1,
      ..
    }
  ));
  for _ in 0..50 {
    assert!(matches!(session.capture(ids[0]).done().unwrap(), FrameRead::Unchanged { frame_number: 1 }));
  }
  let stats = session.stats();
  assert_eq!((stats.frames_fresh, stats.frames_unchanged), (1, 50));
  assert_eq!(stats.frames_delivered, 1, "the compositor delivered no frame while the window stood still");

  fake.repaint(101);
  assert!(matches!(
    session.capture(ids[0]).done().unwrap(),
    FrameRead::Fresh {
      frame_number: 2,
      ..
    }
  ));
}

#[test]
fn a_fresh_capture_describes_the_live_frame_and_names_the_window() {
  let (mut session, fake, ids) = session_with(1);
  fake.repaint(101);
  fake.edit(101, |probe| probe.frame = Some(Rect::new(10.0, 20.0, 4.0, 3.0)));

  let FrameRead::Fresh {
    capture, followed, ..
  } = session.capture(ids[0]).done().unwrap()
  else {
    panic!("expected a fresh frame");
  };

  assert_eq!(capture.bounds, Rect::new(10.0, 20.0, 4.0, 3.0));
  assert_eq!(capture.scale_factor, 2.0, "an 8 pixel wide image of a 4 point wide window");
  assert_eq!(capture.backend, crate::wgc::WGC_BACKEND);
  assert!(followed.is_some());
  assert_eq!(capture.origin.as_ref().map(|origin| origin.point), Some(Point::new(0.0, 0.0)));
}

#[test]
fn a_failed_stream_is_reopened_once_and_a_second_failure_is_reported() {
  let (mut session, fake, ids) = session_with(1);
  fake.repaint(101);
  assert!(session.capture(ids[0]).is_done());

  fake.world().streams.get_mut(&101).unwrap().faults = vec![StreamError::Failed("device removed".into())];
  assert!(session.capture(ids[0]).is_done(), "one fault is absorbed by reopening the stream");
  assert_eq!(session.stats().streams_reopened, 1);

  fake.world().streams.get_mut(&101).unwrap().faults = vec![
    StreamError::Failed("device removed".into()),
    StreamError::Failed("still removed".into()),
  ];
  let outcome = session.capture(ids[0]);
  assert!(matches!(&outcome, Outcome::Failed { operation: "capture", message } if message == "still removed"), "{outcome:?}");
}

// ROOT CAUSE:
//
// If a window's capture region changed size, the stream recreated its frame pool in place and kept
// delivering frames from the old origin. After a move, a window whose frame bounds shrank from its
// whole rectangle to its visible frame came back shifted by its 7 pixel invisible border, so a
// point read off a frame landed beside its target (`background-session-eval geometry`).
//
// Before the fix, the session served those frames. The fix ends the stream with `Resized`, and the
// session replaces it with one that lines up, without counting that as a fault.
#[test]
fn a_stream_whose_window_changed_size_is_replaced_and_the_read_comes_from_the_new_one() {
  let (mut session, fake, ids) = session_with(1);
  fake.repaint(101);
  assert!(session.capture(ids[0]).is_done());

  fake.world().streams.get_mut(&101).unwrap().faults = vec![StreamError::Resized];
  fake.repaint(101);
  assert!(matches!(session.capture(ids[0]).done().unwrap(), FrameRead::Fresh { .. }));

  let stats = session.stats();
  assert_eq!((stats.streams_opened, stats.streams_resized, stats.streams_reopened), (1, 1, 0));
  assert_eq!(fake.world().opened, vec![101, 101]);
}

#[test]
fn a_window_that_keeps_changing_size_is_a_typed_failure_not_an_endless_reopen() {
  let (mut session, fake, ids) = session_with(1);
  fake.world().streams.entry(101).or_default().faults = vec![StreamError::Resized; 4];

  let outcome = session.capture(ids[0]);

  assert!(
    matches!(
      &outcome,
      Outcome::Failed {
        operation: "capture",
        ..
      }
    ),
    "{outcome:?}"
  );
  assert_eq!(session.stats().streams_resized, 3);
}

#[test]
fn a_stream_that_reports_the_window_closed_marks_the_target_lost() {
  let (mut session, fake, ids) = session_with(2);
  fake.repaint(101);
  assert!(session.capture(ids[0]).is_done());

  fake.world().streams.get_mut(&101).unwrap().faults = vec![StreamError::Closed];

  assert_eq!(
    session.capture(ids[0]).map(|_| ()),
    Outcome::TargetLost {
      reason: LostReason::WindowDestroyed
    }
  );
  assert_eq!(
    session.targets()[0].state,
    TargetState::Lost {
      reason: LostReason::WindowDestroyed
    }
  );
  assert!(session.click(ids[1], WindowPoint::new(1.0, 1.0), left_click()).is_done());
}

// ROOT CAUSE:
//
// If a target was lost or detached, `SessionStats::frames_delivered` fell by the frames its stream
// had delivered, because the total was summed over the live streams only and the stream of a lost
// window is closed. A run that lost two windows reported fewer frames delivered than it had read.
//
// Before the fix, the total went from 4 to 2 when the window that had delivered two frames was
// lost. The fix adds the frames of streams closed since, so the total only ever grows.
#[test]
fn frames_delivered_never_shrinks_when_a_target_is_lost_or_detached() {
  let (mut session, fake, ids) = session_with(3);
  fake.repaint(101);
  fake.repaint(101);
  fake.repaint(102);
  fake.repaint(103);
  for id in &ids {
    assert!(session.capture(*id).is_done());
  }
  assert_eq!(session.stats().frames_delivered, 4);

  fake.close_window(101);
  assert!(matches!(session.capture(ids[0]), Outcome::TargetLost { .. }));
  assert_eq!(session.stats().frames_delivered, 4, "the lost window's two frames still count");

  assert!(session.detach(ids[1]));
  assert_eq!(session.stats().frames_delivered, 4, "so does a detached window's frame");

  fake.repaint(103);
  assert_eq!(session.stats().frames_delivered, 5);
}

#[test]
fn refresh_reports_every_target_including_a_move_without_operating_on_any() {
  let (mut session, fake, _ids) = session_with(3);
  let moved = Rect::new(1.0, 2.0, 300.0, 200.0);
  fake.edit(102, |probe| probe.frame = Some(moved));
  fake.close_window(103);

  let report = session.refresh();

  assert_eq!(report.len(), 3);
  assert_eq!(report[0].0.state, TargetState::Live);
  assert_eq!(
    report[1].1,
    Some(FrameChange {
      from: FRAME,
      to: moved
    })
  );
  assert_eq!(
    report[2].0.state,
    TargetState::Lost {
      reason: LostReason::WindowDestroyed
    }
  );
  assert!(fake.world().clicks.is_empty() && fake.world().scrolls.is_empty());
}

#[test]
fn outcomes_serialize_with_the_names_the_evidence_uses() {
  let skipped: Outcome<()> = Outcome::SkippedMinimized;
  let lost: Outcome<()> = Outcome::TargetLost {
    reason: LostReason::WindowDestroyed,
  };
  let unsupported: Outcome<()> = Outcome::Unsupported {
    operation: "type_text",
    reason: "no background keyboard".into(),
  };

  assert_eq!(serde_json::to_value(&skipped).unwrap(), serde_json::json!({"result": "skipped_minimized"}));
  assert_eq!(serde_json::to_value(&lost).unwrap(), serde_json::json!({"result": "target_lost", "reason": "window_destroyed"}));
  assert_eq!(
    serde_json::to_value(&unsupported).unwrap(),
    serde_json::json!({"result": "unsupported", "operation": "type_text", "reason": "no background keyboard"})
  );
  assert_eq!(
    serde_json::to_value(TargetState::Lost {
      reason: LostReason::HandleReused
    })
    .unwrap(),
    serde_json::json!({"state": "lost", "reason": "handle_reused"})
  );
}
