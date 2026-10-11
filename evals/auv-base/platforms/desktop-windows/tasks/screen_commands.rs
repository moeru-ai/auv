//! `screen`: `screen.findText` and `screen.waitForText` on Windows, run as the real `auv` binary
//! against a window this harness owns, while an independent watch checks the user's desktop.
//!
//! These commands read the whole primary display, so what they keep is a screenshot of the
//! user's desktop. The harness never opens it: every command records into a store that is removed
//! however the scenario ends, and the only things written down are the receiver's own text boxes,
//! a count of the other matches, and the desktop watch.
//!
//! What each command shows is judged against the receiver's own facts, never against what the
//! driver reports about itself: the screen rectangle of the control that draws a text, read from
//! the receiver, and that rectangle again after the receiver has moved.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use auv_driver_common::Driver;
use auv_driver_common::window::Window;
use auv_driver_windows::{ClickRequest, WindowsDriver};
use serde_json::{Value, json};

use super::background::{Desk, Tally, discover_by_hwnd, invariants, occlusion, point_in_window, run_id, spawn_receiver_with, system_facts};
use super::records::Records;
use super::watch::DesktopWatch;
use super::window_commands::{Invocation, Placement, TEXT_SLACK, inside, invoke, near_corner, rect_of};

pub struct ScreenCommandsOptions {
  pub output: PathBuf,
  /// The `auv` executable under test.
  pub auv: PathBuf,
}

/// Removes the store the commands record into, however the scenario ends: each command keeps the
/// screenshot it read as its OCR source, and none of it is evidence.
struct StoreGuard(PathBuf);

impl Drop for StoreGuard {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

/// `[x, y, width, height]` of a window rectangle given as `[left, top, right, bottom]`.
fn sized(rect: [i32; 4]) -> [f64; 4] {
  [
    f64::from(rect[0]),
    f64::from(rect[1]),
    f64::from(rect[2] - rect[0]),
    f64::from(rect[3] - rect[1]),
  ]
}

/// The program of each window above `window` in the Z-order whose frame overlaps it: what has to be
/// closed for the receiver's text to be readable. Programs only, never titles.
fn covering_programs(driver: &auv_driver_windows::WindowsDriverSession, window: &Window) -> Vec<String> {
  let Ok(windows) = driver.window().list() else {
    return Vec::new();
  };
  windows
    .iter()
    .take_while(|other| other.reference.id != window.reference.id)
    .filter(|other| other.frame.intersect(window.frame).is_some())
    .map(|other| other.app_name.clone().unwrap_or_else(|| "unknown".to_string()))
    .collect()
}

/// The bounds of every match a text command returned.
fn match_bounds(run: &Invocation) -> Vec<[f64; 4]> {
  run
    .json
    .as_ref()
    .and_then(|json| json["result"]["matches"].as_array())
    .map(|matches| matches.iter().filter_map(|matched| rect_of(&matched["bounds"])).collect())
    .unwrap_or_default()
}

/// Whether `found` is where `rect`'s control draws its text. The screen is not the receiver's
/// alone, so a match elsewhere is somebody else's text and does not count.
fn belongs(placement: Placement, found: [f64; 4], rect: [i32; 4]) -> bool {
  match placement {
    Placement::Corner => near_corner([found[0] - f64::from(rect[0]), found[1] - f64::from(rect[1])]),
    Placement::Inside => inside(found, rect, TEXT_SLACK),
    Placement::Recorded => true,
  }
}

struct Scenario<'a> {
  auv: &'a Path,
  store: &'a Path,
  hwnd: isize,
  desk: Desk<'a>,
  tally: Tally,
  records: Records,
}

impl Scenario<'_> {
  /// Runs one command between two looks at the desktop. Returns the run and what the watch saw.
  fn run(&mut self, command: &str, args: &[&str]) -> Result<(Invocation, Value)> {
    let (measured, _) = self.desk.measure(|| invoke(self.auv, self.store, command, args), || ());
    self.tally.record(&measured);
    let facts = json!({"invariants": invariants(&measured, self.hwnd), "verdict": measured.verdict});
    Ok((measured.value?, facts))
  }

  /// Runs `screen.findText` and returns the match that sits at the control `rect`.
  fn find(&mut self, query: &str, control: &str, rect: [i32; 4], placement: Placement) -> Result<Option<[f64; 4]>> {
    let (run, facts) = self.run("screen.findText", &[query])?;
    let all = match_bounds(&run);
    let belonging = all.iter().copied().find(|found| belongs(placement, *found, rect));
    self.tally.expect(run.completed(), format!("screen.findText {query:?} failed: {}", run.failure()));
    self.tally.expect(belonging.is_some(), format!("screen.findText {query:?} put none of its {} match(es) at the {control}", all.len()));
    // Only the match at the receiver is written down; the rest is the user's screen and is counted.
    self.records.write(
      "screen.findText",
      json!({"query": query, "control": control, "control_rect": rect, "exit": run.exit, "ms": run.ms, "matches": all.len(),
        "match": belonging, "offset_from_control_corner": belonging.map(|found| [found[0] - f64::from(rect[0]), found[1] - f64::from(rect[1])]),
        "watch": facts}),
    )?;
    Ok(belonging)
  }
}

/// Runs the screen text commands and writes `screen.jsonl`. Passes when every command showed what
/// the receiver knows to be true and the desktop was never disturbed.
pub fn screen_commands(options: &ScreenCommandsOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let auv = options.auv.canonicalize().with_context(|| format!("find the auv executable {}", options.auv.display()))?;
  let run = run_id();
  let records = Records::create(&options.output.join("screen.jsonl"), &run, "screen")?;
  let store = options.output.join("store");
  let _store = StoreGuard(store.clone());
  std::fs::create_dir_all(&store)?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;

  // Text can only be read off the screen where nothing covers it, so the receiver is not sunk.
  let mut target = spawn_receiver_with(&options.output, 0, false)?;
  let hwnd: isize = target.layout.hwnd.parse().context("receiver window handle")?;
  let (window, _) = discover_by_hwnd(&driver, hwnd)?;
  let covered = occlusion(&driver, &window);
  let layout = target.refresh_layout()?;
  if covered > 0.01 {
    // Text under another window cannot be read, and the commands would screenshot whatever is on
    // top of it. Stop before any of them runs. The programs are named on the console only.
    let programs = covering_programs(&driver, &window);
    let mut records = records;
    records.write(
      "summary",
      json!({"passed": false, "skipped": "the receiver is covered", "covered_fraction": covered, "windows_above": programs.len()}),
    )?;
    let _ = target.close();
    println!("screen: SKIPPED, the receiver is {covered:.2} covered by {}", programs.join(", "));
    println!("  Close or minimize them, or run it on a free desktop. No screen command ran.");
    return Ok(false);
  }
  let start = watch.snapshot();
  let mut scenario = Scenario {
    auv: &auv,
    store: &store,
    hwnd,
    desk: Desk::new(&watch, hwnd),
    tally: Tally::default(),
    records,
  };

  // The receiver must be on the display the commands read.
  let (displays, facts) = scenario.run("display.list", &[])?;
  let listed = displays.json.as_ref().and_then(|json| json["result"]["displays"].as_array()).cloned().unwrap_or_default();
  let primary = listed.iter().find(|display| display["is_primary"] == true);
  let primary_frame = primary.and_then(|display| rect_of(&display["frame"]));
  scenario.tally.expect(displays.completed() && primary_frame.is_some(), "display.list gave no primary display");
  let primary_rect = primary_frame.map(|frame| {
    [
      frame[0] as i32,
      frame[1] as i32,
      (frame[0] + frame[2]) as i32,
      (frame[1] + frame[3]) as i32,
    ]
  });
  scenario.tally.expect(
    primary_rect.is_some_and(|primary| inside(sized(layout.window_rect), primary, 0.0)),
    "the receiver is not on the primary display, which is the one the screen commands read",
  );
  scenario.records.write(
    "setup",
    json!({"system": system_facts(), "auv": auv.file_name().map(|name| name.to_string_lossy().into_owned()),
      "displays": listed.len(), "primary_frame": primary_frame, "primary_scale_factor": primary.map(|display| display["scale_factor"].clone()),
      "receiver": {"pid": layout.pid, "window_rect": layout.window_rect, "label_rect": layout.label_rect, "edit_rect": layout.edit_rect},
      "covered_fraction": covered, "foreground": start.foreground.to_string(), "display_list": facts}),
  )?;

  // screen.findText: text is placed against the control that draws it, in screen coordinates.
  // - "clicks 0" starts at the left edge of the label, near its top.
  // - "line 000" is the first line of the edit control, which only has to contain it.
  let label = scenario.find("clicks 0", "label", layout.label_rect, Placement::Corner)?;
  scenario.find("line 000", "edit", layout.edit_rect, Placement::Inside)?;

  // The same text follows the receiver when it moves, so the bounds are screen positions and not
  // something the capture happens to repeat.
  let (left, top) = (layout.window_rect[0] - 500, layout.window_rect[1] - 300);
  target.request(json!({"cmd": "move", "left": left, "top": top}))?;
  std::thread::sleep(Duration::from_millis(400));
  let moved = target.refresh_layout()?;
  let shifted = scenario.find("clicks 0", "label (moved)", moved.label_rect, Placement::Corner)?;
  let travelled = label.zip(shifted).map(|(before, after)| [after[0] - before[0], after[1] - before[1]]);
  let wanted = [
    f64::from(moved.label_rect[0] - layout.label_rect[0]),
    f64::from(moved.label_rect[1] - layout.label_rect[1]),
  ];
  scenario.tally.expect(
    travelled.is_some_and(|travelled| (travelled[0] - wanted[0]).abs() <= 2.0 && (travelled[1] - wanted[1]).abs() <= 2.0),
    format!("the label's text moved {travelled:?} when the receiver moved {wanted:?}"),
  );
  scenario.records.write("screen.findText.moved", json!({"receiver_moved": wanted, "text_moved": travelled}))?;

  // Give the receiver text worth waiting for: three clicks, posted to the window.
  let (window, _) = discover_by_hwnd(&driver, hwnd)?;
  let button = point_in_window(&window, moved.button_center);
  let mut session = driver.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
  let id = session.attach(&window).map_err(|error| anyhow::anyhow!("attach the receiver: {error}"))?;
  let delivered = (0..3).filter(|_| session.click(id, button, ClickRequest::default()).is_done()).count();
  std::thread::sleep(Duration::from_millis(200));
  let state = target.state()?;
  scenario.tally.expect(delivered == 3 && state.clicks == 3, format!("the receiver counted {} of 3 posted clicks", state.clicks));

  // screen.waitForText: present now, absent for good, and appearing while it waits.
  let (present, facts) = scenario.run("screen.waitForText", &["clicks 3"])?;
  scenario.tally.expect(present.completed(), format!("screen.waitForText failed on text that is there: {}", present.failure()));
  scenario.tally.expect(
    match_bounds(&present).iter().any(|found| belongs(Placement::Corner, *found, moved.label_rect)),
    "screen.waitForText did not place the text at the label",
  );
  scenario.records.write("screen.waitForText.present", json!({"exit": present.exit, "ms": present.ms, "watch": facts}))?;

  let (absent, facts) = scenario.run("screen.waitForText", &["clicks 99"])?;
  scenario.tally.expect(
    absent.exit == Some(1) && absent.failure().contains("did not find text"),
    format!("screen.waitForText on absent text ended {:?}: {}", absent.exit, absent.failure()),
  );
  scenario.tally.expect(absent.ms >= 4_500.0, format!("screen.waitForText gave up after {:.0} ms, before its timeout", absent.ms));
  scenario
    .records
    .write("screen.waitForText.absent", json!({"exit": absent.exit, "ms": absent.ms, "failure": absent.failure(), "watch": facts}))?;

  // The label reads "clicks 3" until the clicks below arrive, 1.5 s after the command starts.
  let (appearing, _) = scenario.desk.measure(
    || -> Result<(Invocation, usize)> {
      std::thread::scope(|scope| {
        let waiter = scope.spawn(|| invoke(&auv, &store, "screen.waitForText", &["clicks 6"]));
        std::thread::sleep(Duration::from_millis(1_500));
        let sent = (0..3).filter(|_| session.click(id, button, ClickRequest::default()).is_done()).count();
        let run = waiter.join().map_err(|_| anyhow::anyhow!("the waiting command panicked"))??;
        Ok((run, sent))
      })
    },
    || (),
  );
  scenario.tally.record(&appearing);
  let appearing_facts = json!({"invariants": invariants(&appearing, hwnd), "verdict": appearing.verdict});
  let (appearing_run, sent) = appearing.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  scenario.tally.expect(*sent == 3, format!("{sent} of 3 clicks reached the receiver while the command waited"));
  scenario
    .tally
    .expect(appearing_run.completed(), format!("screen.waitForText missed text that appeared while it waited: {}", appearing_run.failure()));
  scenario.records.write(
    "screen.waitForText.appearing",
    json!({"exit": appearing_run.exit, "ms": appearing_run.ms, "clicks_sent": sent, "watch": appearing_facts}),
  )?;

  // What the commands did to the desktop.
  let finish = watch.snapshot();
  let Scenario {
    mut tally,
    mut records,
    ..
  } = scenario;
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the scenario");
  tally.expect(tally.violated == 0, format!("{} command(s) disturbed the desktop", tally.violated));
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({"passed": passed, "ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "failures": tally.failures, "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys}),
  )?;
  drop(session);
  let _ = target.close();
  println!(
    "screen: {} commands, {} held, {} inconclusive, {} violated; {}",
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("screen.jsonl").display());
  Ok(passed)
}
