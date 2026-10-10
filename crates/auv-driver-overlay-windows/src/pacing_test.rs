use std::time::{Duration, Instant};

use auv_driver_overlay_common::Wake;

use super::{FRAME_INTERVAL, Pacer, Step};

fn ms(value: u64) -> Duration {
  Duration::from_millis(value)
}

fn pacer(idle_removal: Option<Duration>) -> Pacer {
  Pacer::new(FRAME_INTERVAL, idle_removal)
}

#[test]
fn a_fresh_pacer_waits_for_the_first_event() {
  assert_eq!(pacer(None).step(Instant::now()), Step::WaitForEvent);
}

#[test]
fn the_first_event_draws_immediately() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.event();
  assert_eq!(pacer.step(start), Step::Render);
}

#[test]
fn events_inside_a_frame_wait_for_the_frame_boundary_instead_of_exceeding_sixty_fps() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::Idle);
  pacer.event();

  assert_eq!(pacer.step(start + ms(3)), Step::WaitUntil(start + FRAME_INTERVAL));
  assert_eq!(pacer.step(start + FRAME_INTERVAL), Step::Render);
}

#[test]
fn an_event_after_a_quiet_period_draws_without_waiting_for_a_frame_boundary() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::Idle);
  pacer.event();
  assert_eq!(pacer.step(start + ms(500)), Step::Render, "added latency must be zero once the frame budget has passed");
}

#[test]
fn an_animating_scene_is_drawn_once_per_frame_interval() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::NextFrame);

  assert_eq!(pacer.step(start + ms(1)), Step::WaitUntil(start + FRAME_INTERVAL));
  assert_eq!(pacer.step(start + FRAME_INTERVAL), Step::Render);
}

#[test]
fn a_slow_frame_is_followed_by_an_immediate_one_without_a_burst_of_catch_up_frames() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::NextFrame);

  // The frame took 40 ms, so the next one is overdue and runs at once.
  let late = start + ms(40);
  assert_eq!(pacer.step(late), Step::Render);
  pacer.drew(late, Wake::NextFrame);
  assert_eq!(pacer.step(late + ms(1)), Step::WaitUntil(late + FRAME_INTERVAL), "only one frame for the missed time");
}

#[test]
fn a_still_scene_with_a_pending_mark_sleeps_until_it_is_due() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::After(ms(1400)));

  assert_eq!(pacer.step(start + ms(100)), Step::WaitUntil(start + ms(1400)));
  assert_eq!(pacer.step(start + ms(1400)), Step::Render);
}

#[test]
fn a_still_scene_is_removed_after_the_idle_period_and_not_before() {
  let start = Instant::now();
  let mut pacer = pacer(Some(ms(2000)));
  pacer.event();
  pacer.drew(start, Wake::Idle);

  assert_eq!(pacer.step(start + ms(10)), Step::WaitUntil(start + ms(2000)));
  assert_eq!(pacer.step(start + ms(2000)), Step::Remove);

  pacer.removed();
  assert_eq!(pacer.step(start + ms(2001)), Step::WaitForEvent);
}

#[test]
fn an_event_cancels_the_pending_removal() {
  let start = Instant::now();
  let mut pacer = pacer(Some(ms(2000)));
  pacer.drew(start, Wake::Idle);
  pacer.event();
  assert_eq!(pacer.step(start + ms(1000)), Step::Render);
  pacer.drew(start + ms(1000), Wake::Idle);

  assert_eq!(pacer.step(start + ms(2500)), Step::WaitUntil(start + ms(3000)), "the idle period restarts from the later frame");
}

#[test]
fn manual_removal_never_removes() {
  let start = Instant::now();
  let mut pacer = pacer(None);
  pacer.drew(start, Wake::Idle);
  assert_eq!(pacer.step(start + Duration::from_secs(3600)), Step::WaitForEvent);
}

#[test]
fn repeated_idle_frames_do_not_push_the_removal_deadline_back() {
  let start = Instant::now();
  let mut pacer = pacer(Some(ms(2000)));
  pacer.drew(start, Wake::Idle);
  pacer.drew(start + ms(900), Wake::Idle);
  assert_eq!(pacer.step(start + ms(2000)), Step::Remove);
}
