//! `foreground`: the commands that act on the desktop rather than on one window, run as the real
//! `auv` binary against a window this harness owns: `display.list`, `display.capture`,
//! `screen.captureRegion`, `input.moveMouse`, `input.clickPoint` and `input.drag` (screen, display
//! and window bases), and the foreground keyboard commands `input.typeText`, `input.key`,
//! `input.keys`, `input.holdKeys` and `input.keyboard`.
//!
//! Unlike the other scenarios this one moves the real pointer and sends real keys, because that is
//! what these commands do. It keeps them on its own window:
//!
//! - The receiver is created above ordinary windows, and every point a command aims the real
//!   pointer at is checked to belong to the receiver before the command runs.
//! - Before every command that moves the real pointer or sends keys, the harness waits until the
//!   user has not touched the mouse or keyboard for [`IDLE_MS`], and gives up if that never happens.
//! - A real click on the receiver makes it the foreground window, as a user's click would. Each
//!   keyboard command runs only while the receiver is the foreground window and its text box has
//!   the focus; otherwise the remaining keyboard commands are skipped and the run fails.
//! - At the end the pointer goes back where it was and the foreground goes back to the window that
//!   had it, however the run ends.
//!
//! What each command did is judged against what the receiver saw, never against what the driver
//! reports about itself: the pointer position Windows reports, the button's `BN_CLICKED` count, the
//! text box's text and the key messages it received, and the pad's drag record. Captures are judged
//! by decoding the receiver's state strip from their pixels, which only lines up when the bounds
//! place the image on the screen correctly. The display capture is a screenshot of the user's
//! screen: it is kept in a store that is removed however the run ends, and only the decoded strip
//! is written down.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use auv_driver_common::Driver;
use auv_driver_common::capture::Capture;
use auv_driver_common::geometry::Rect;
use auv_driver_windows::{ClickRequest, WindowsDriver};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, PostMessageW, SetCursorPos, WM_APP, WindowFromPoint};

use super::background::{
  Desk, Measured, Tally, discover_by_hwnd, invariants, occlusion, placement, point_in_window, run_id, spawn_topmost_receiver, system_facts,
};
use super::receiver_process::{Layout, Receiver, ReceiverSpec, Strip};
use super::records::Records;
use super::sustained::decode_strip;
use super::watch::DesktopWatch;
use super::window_commands::{Invocation, invoke, rect_of};

/// How long the user must have left the mouse and keyboard alone before a command sends real
/// input, so the input cannot interleave with theirs.
const IDLE_MS: u64 = 2_000;
/// How long the harness waits for that before it gives up on the rest of the run.
const IDLE_WAIT: Duration = Duration::from_secs(30);
/// `VK_SHIFT`, as the text box logs it.
const VK_SHIFT: u64 = 0x10;
/// The receiver's own message that hands the foreground to the window in `wparam`.
const WM_YIELD_FOREGROUND: u32 = WM_APP + 2;

pub struct ForegroundCommandsOptions {
  pub output: PathBuf,
  /// The `auv` executable under test.
  pub auv: PathBuf,
  /// Leave the text box with the desktop's input method. Key presses then go to the IME when one is
  /// active, so the text after a key command is recorded, not judged.
  pub keep_ime: bool,
}

/// Puts the pointer and the foreground back however the run ends. Declared after the receiver, so
/// it runs while the receiver still holds the foreground it can hand back.
struct Restore {
  cursor: (i32, i32),
  foreground: isize,
  receiver: isize,
  store: PathBuf,
}

impl Drop for Restore {
  fn drop(&mut self) {
    // SAFETY: a message to the harness's own receiver, which hands the foreground on only while it
    // holds it, and a pointer move back to where the user left it.
    unsafe {
      let _ = PostMessageW(HWND(self.receiver as *mut c_void), WM_YIELD_FOREGROUND, WPARAM(self.foreground as usize), LPARAM(0));
      let _ = SetCursorPos(self.cursor.0, self.cursor.1);
    }
    std::thread::sleep(Duration::from_millis(200));
    // The store holds a screenshot of the user's desktop; none of it is evidence.
    let _ = std::fs::remove_dir_all(&self.store);
  }
}

/// Whether the top-level window under the screen point `[x, y]` is `hwnd`.
fn owns_point(hwnd: isize, [x, y]: [i32; 2]) -> bool {
  // SAFETY: read-only window queries.
  unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) }.0 as isize == hwnd
}

fn center_of([left, top, right, bottom]: [i32; 4]) -> [i32; 2] {
  [(left + right) / 2, (top + bottom) / 2]
}

/// The pad's two drag ends, a third of the way in from each side, on its middle line.
fn pad_ends([left, top, right, bottom]: [i32; 4]) -> ([i32; 2], [i32; 2]) {
  let (width, middle) = (right - left, (top + bottom) / 2);
  ([left + width / 3, middle], [right - width / 3, middle])
}

fn near([x, y]: [i32; 2], point: &Value, slack: f64) -> bool {
  point.as_array().is_some_and(|point| {
    point.len() == 2
      && point[0].as_f64().is_some_and(|value| (value - f64::from(x)).abs() <= slack)
      && point[1].as_f64().is_some_and(|value| (value - f64::from(y)).abs() <= slack)
  })
}

fn arg(value: impl ToString) -> String {
  value.to_string()
}

struct Scenario<'a> {
  auv: &'a Path,
  store: &'a Path,
  hwnd: isize,
  watch: &'a DesktopWatch,
  desk: Desk<'a>,
  tally: Tally,
  records: Records,
}

impl Scenario<'_> {
  fn invoke(&mut self, command: &str, args: &[String]) -> Measured<Result<Invocation>> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (auv, store) = (self.auv, self.store);
    self.desk.measure(|| invoke(auv, store, command, &args), || ()).0
  }

  /// Runs a command that must leave the desktop alone: no injected input, no foreground or cursor
  /// change. It counts toward the held, inconclusive and violated totals.
  fn quiet(&mut self, command: &str, args: &[String]) -> Result<(Invocation, Value)> {
    let measured = self.invoke(command, args);
    self.tally.record(&measured);
    let facts = json!({"invariants": invariants(&measured, self.hwnd), "verdict": measured.verdict});
    Ok((measured.value?, facts))
  }

  /// Runs a command that acts through the real pointer or keyboard. The desktop is meant to change,
  /// so the watch's facts are recorded, and only a foreground change to a window of another program
  /// fails the run: that would be input that left the receiver.
  fn acting(&mut self, command: &str, args: &[String]) -> Result<(Invocation, Value)> {
    let measured = self.invoke(command, args);
    let facts = invariants(&measured, self.hwnd);
    let strayed = facts["foreground_event_log"]
      .as_array()
      .is_some_and(|events| events.iter().any(|event| event["became_foreground"] == true && event["owner"] == "other process"));
    self.tally.expect(!strayed, format!("{command}: another program's window became the foreground window"));
    Ok((measured.value?, facts))
  }

  /// Waits until the user has left the mouse and keyboard alone for [`IDLE_MS`].
  fn wait_until_idle(&self) -> Result<u64> {
    let started = Instant::now();
    loop {
      let now = self.watch.snapshot();
      let idle = if now.last_physical_ms == 0 {
        now.taken_ms
      } else {
        now.taken_ms.saturating_sub(now.last_physical_ms)
      };
      if idle >= IDLE_MS {
        return Ok(idle);
      }
      if started.elapsed() > IDLE_WAIT {
        bail!("the mouse or keyboard was in use for {} s; no real input was sent from then on", IDLE_WAIT.as_secs());
      }
      std::thread::sleep(Duration::from_millis(100));
    }
  }

  /// Before real pointer input: the user is idle and every point lies on the receiver.
  fn pointer_may_go(&self, points: &[[i32; 2]]) -> Result<()> {
    self.wait_until_idle()?;
    if let Some(point) = points.iter().find(|point| !owns_point(self.hwnd, **point)) {
      bail!("the point {point:?} is not on the receiver; no real pointer input was sent to it");
    }
    Ok(())
  }

  /// Before real keys: the user is idle, the receiver is the foreground window and its text box
  /// has the focus, so the keys can only reach the text box.
  fn keys_may_go(&self, target: &mut Receiver) -> Result<()> {
    self.wait_until_idle()?;
    if self.watch.snapshot().foreground != self.hwnd {
      bail!("the receiver is not the foreground window; no key was sent");
    }
    if target.request(json!({"cmd": "state"}))?["typing"]["focused"] != true {
      bail!("the receiver's text box does not have the focus; no key was sent");
    }
    Ok(())
  }
}

/// A capture command's saved image, decoded at the receiver's strip.
fn decoded_capture(run: &Invocation, strip: &Strip) -> Result<Option<(u8, u8)>> {
  let Some(json) = run.json.as_ref() else {
    return Ok(None);
  };
  let capture = &json["result"]["capture"];
  let (Some(bounds), Some(scale)) = (rect_of(&capture["bounds"]), capture["scale_factor"].as_f64()) else {
    return Ok(None);
  };
  let Some(path) = json["artifacts"].as_array().and_then(|artifacts| artifacts.iter().find_map(|artifact| artifact["file_path"].as_str()))
  else {
    return Ok(None);
  };
  let image = image::open(path).with_context(|| format!("read {path}"))?.to_rgba8();
  let capture = Capture {
    origin: None,
    image,
    bounds: Rect::new(bounds[0], bounds[1], bounds[2], bounds[3]),
    scale_factor: scale,
    backend: String::new(),
    fallback_reason: None,
  };
  Ok(Some(decode_strip(&capture, strip)))
}

/// The pixel size a capture command reported, against its bounds at its scale.
fn pixels_match_bounds(run: &Invocation) -> bool {
  let Some(capture) = run.json.as_ref().map(|json| &json["result"]["capture"]) else {
    return false;
  };
  let (Some(bounds), Some(scale)) = (rect_of(&capture["bounds"]), capture["scale_factor"].as_f64()) else {
    return false;
  };
  let pixels = (capture["pixel_dimensions"]["width"].as_f64(), capture["pixel_dimensions"]["height"].as_f64());
  matches!(pixels, (Some(width), Some(height)) if (width - bounds[2] * scale).abs() <= 1.0 && (height - bounds[3] * scale).abs() <= 1.0)
}

fn selected_path(run: &Invocation) -> Value {
  run.json.as_ref().map(|json| json["result"]["action"]["selected_path"].clone()).unwrap_or(Value::Null)
}

/// Runs the foreground commands and writes `foreground.jsonl`. Passes when every command showed
/// what the receiver knows to be true, the commands that should not touch the desktop left it
/// alone, and no input left the receiver.
pub fn foreground_commands(options: &ForegroundCommandsOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let auv = options.auv.canonicalize().with_context(|| format!("find the auv executable {}", options.auv.display()))?;
  let run = run_id();
  let mut records = Records::create(&options.output.join("foreground.jsonl"), &run, "foreground")?;
  let store = options.output.join("store");
  std::fs::create_dir_all(&store)?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let start = watch.snapshot();

  let (width, height) = (420, 440);
  let (left, top) = placement(0, width, height);
  let spec = ReceiverSpec {
    index: 0,
    title: "AUV eval foreground".into(),
    left,
    top,
    width,
    height,
    color: "2E6F9E".into(),
    sink: false,
  };
  let extra: &[&str] = if options.keep_ime {
    &["--foreground-input", "--keep-ime"]
  } else {
    &["--foreground-input"]
  };
  let mut target = spawn_topmost_receiver(&spec, extra)?;
  let hwnd = target.hwnd();
  let _restore = Restore {
    cursor: start.cursor,
    foreground: start.foreground,
    receiver: hwnd,
    store: store.clone(),
  };
  let (window, _) = discover_by_hwnd(&driver, hwnd)?;
  let covered = occlusion(&driver, &window);
  if covered > 0.01 {
    records.write("summary", json!({"passed": false, "skipped": "the receiver is covered", "covered_fraction": covered}))?;
    println!("foreground: SKIPPED, the receiver is {covered:.2} covered. No command ran.");
    return Ok(false);
  }

  // A fresh receiver may take the foreground on its first activation and hands it straight back
  // (it is not armed to keep it yet). Click it once through the background session first, as every
  // scenario does, and give it a state worth reading back: five clicks.
  let mut session = driver.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
  let id = session.attach(&window).map_err(|error| anyhow::anyhow!("attach the receiver: {error}"))?;
  let layout: Layout = target.refresh_layout()?;
  let button = point_in_window(&window, layout.button_center);
  let posted = (0..5).filter(|_| session.click(id, button, ClickRequest::default()).is_done()).count();
  std::thread::sleep(Duration::from_millis(300));
  let warm = target.state()?;
  let raw_layout = target.request(json!({"cmd": "layout"}))?;
  let typing_center: [i32; 2] = serde_json::from_value(raw_layout["typing_center"].clone()).context("text box position")?;
  let pad: [i32; 4] = serde_json::from_value(raw_layout["pad_rect"].clone()).context("pad position")?;
  let mut tally = Tally::default();
  tally.expect(posted == 5 && warm.clicks == 5, format!("the receiver counted {} of 5 posted clicks", warm.clicks));
  records.write(
    "setup",
    json!({"system": system_facts(), "auv": auv.file_name().map(|name| name.to_string_lossy().into_owned()),
      "receiver": {"pid": layout.pid, "window_rect": layout.window_rect, "button_rect": layout.button_rect, "typing_center": typing_center,
        "pad_rect": pad, "strip_origin": layout.strip.origin},
      "covered_fraction": covered, "warm_up": {"posted_clicks": posted, "clicks": warm.clicks,
        "foreground_taken": warm.foreground_taken, "foreground_yielded": warm.foreground_yielded},
      "foreground_is_receiver": watch.snapshot().foreground == hwnd}),
  )?;

  let mut scenario = Scenario {
    auv: &auv,
    store: &store,
    hwnd,
    watch: &watch,
    desk: Desk::new(&watch, hwnd),
    tally,
    records,
  };

  // display.list: the receiver must be on the primary display, which the captures read.
  let (displays, facts) = scenario.quiet("display.list", &[])?;
  let listed = displays.json.as_ref().and_then(|json| json["result"]["displays"].as_array()).cloned().unwrap_or_default();
  let primary = listed.iter().find(|display| display["is_primary"] == true).cloned();
  let primary_frame = primary.as_ref().and_then(|display| rect_of(&display["frame"]));
  let display_id = primary.as_ref().and_then(|display| display["id"].as_str()).unwrap_or_default().to_string();
  let on_primary = primary_frame.is_some_and(|frame| {
    let [x0, y0, x1, y1] = layout.window_rect;
    f64::from(x0) >= frame[0] && f64::from(y0) >= frame[1] && f64::from(x1) <= frame[0] + frame[2] && f64::from(y1) <= frame[1] + frame[3]
  });
  scenario.tally.expect(displays.completed() && primary_frame.is_some(), "display.list gave no primary display");
  scenario.tally.expect(on_primary, "the receiver is not on the primary display");
  scenario.records.write(
    "display.list",
    json!({"exit": displays.exit, "ms": displays.ms, "displays": listed.len(), "primary_frame": primary_frame,
      "primary_scale_factor": primary.as_ref().map(|display| display["scale_factor"].clone()), "watch": facts}),
  )?;

  // Window-relative input posts window messages: the pointer, the keyboard and the foreground stay
  // where they are.
  let before = target.state()?;
  let local = point_in_window(&window, layout.button_center);
  let (clicked, facts) = scenario.quiet(
    "input.clickPoint",
    &[
      arg(local.point().x),
      arg(local.point().y),
      arg("--target"),
      format!("window:{hwnd}"),
      arg("--relative-to"),
      arg("window"),
    ],
  )?;
  std::thread::sleep(Duration::from_millis(200));
  let after = target.state()?;
  scenario.tally.expect(clicked.completed(), format!("input.clickPoint relative to the window failed: {}", clicked.failure()));
  scenario
    .tally
    .expect(after.clicks == before.clicks + 1, format!("the button counted {} window click(s), not 1", after.clicks - before.clicks));
  scenario.tally.expect(after.foreground_taken == before.foreground_taken, "the window click gave the receiver the foreground");
  scenario.records.write(
    "input.clickPoint.window",
    json!({"exit": clicked.exit, "ms": clicked.ms, "selected_path": selected_path(&clicked), "clicks": [before.clicks, after.clicks],
      "watch": facts}),
  )?;

  let (from, to) = pad_ends(pad);
  let (local_from, local_to) = (point_in_window(&window, from), point_in_window(&window, to));
  target.request(json!({"cmd": "reset_log"}))?;
  let (dragged, facts) = scenario.quiet(
    "input.drag",
    &[
      arg(local_from.point().x),
      arg(local_from.point().y),
      arg(local_to.point().x),
      arg(local_to.point().y),
      arg("--target"),
      format!("window:{hwnd}"),
      arg("--relative-to"),
      arg("window"),
    ],
  )?;
  std::thread::sleep(Duration::from_millis(200));
  let drag = target.request(json!({"cmd": "state"}))?["pad"].clone();
  scenario.tally.expect(dragged.completed(), format!("input.drag relative to the window failed: {}", dragged.failure()));
  scenario.tally.expect(
    near(from, &drag["down"], 2.0) && near(to, &drag["up"], 2.0) && drag["moves"].as_u64().unwrap_or(0) > 0,
    format!("the pad recorded the window drag as {drag}, not from {from:?} to {to:?} with moves"),
  );
  scenario.records.write(
    "input.drag.window",
    json!({"exit": dragged.exit, "ms": dragged.ms, "selected_path": selected_path(&dragged), "from": from, "to": to, "pad": drag, "watch": facts}),
  )?;

  // Captures: the saved pixels decode to the receiver's own state only when the bounds and the
  // scale place the image on the screen correctly.
  let state = target.state()?;
  let wanted = ((state.clicks & 0xFF) as u8, (state.first_line & 0xFF) as u8);
  let (display_capture, facts) = scenario.quiet("display.capture", &[])?;
  let decoded = decoded_capture(&display_capture, &layout.strip)?;
  scenario.tally.expect(display_capture.completed(), format!("display.capture failed: {}", display_capture.failure()));
  scenario.tally.expect(pixels_match_bounds(&display_capture), "display.capture pixels are not the size of its bounds at its scale");
  scenario.tally.expect(decoded == Some(wanted), format!("the display capture decodes to {decoded:?}, not the receiver's {wanted:?}"));
  scenario.records.write(
    "display.capture",
    json!({"exit": display_capture.exit, "ms": display_capture.ms,
      "bounds": display_capture.json.as_ref().and_then(|json| rect_of(&json["result"]["capture"]["bounds"])),
      "pixel_dimensions": display_capture.json.as_ref().map(|json| json["result"]["capture"]["pixel_dimensions"].clone()),
      "backend": display_capture.json.as_ref().map(|json| json["result"]["capture"]["backend"].clone()),
      "decoded_strip": decoded, "wanted_strip": wanted, "watch": facts}),
  )?;

  let [x0, y0, x1, y1] = layout.window_rect;
  let region = [
    arg("--x"),
    arg(x0),
    arg("--y"),
    arg(y0),
    arg("--width"),
    arg(x1 - x0),
    arg("--height"),
    arg(y1 - y0),
  ];
  let (region_capture, facts) = scenario.quiet("screen.captureRegion", &region)?;
  let decoded = decoded_capture(&region_capture, &layout.strip)?;
  let bounds = region_capture.json.as_ref().and_then(|json| rect_of(&json["result"]["capture"]["bounds"]));
  scenario.tally.expect(region_capture.completed(), format!("screen.captureRegion failed: {}", region_capture.failure()));
  scenario.tally.expect(
    bounds
      == Some([
        f64::from(x0),
        f64::from(y0),
        f64::from(x1 - x0),
        f64::from(y1 - y0),
      ]),
    format!("screen.captureRegion bounds are {bounds:?}, not the region asked for"),
  );
  scenario.tally.expect(pixels_match_bounds(&region_capture), "screen.captureRegion pixels are not the size of its bounds at its scale");
  scenario.tally.expect(decoded == Some(wanted), format!("the region capture decodes to {decoded:?}, not the receiver's {wanted:?}"));
  scenario.records.write(
    "screen.captureRegion",
    json!({"exit": region_capture.exit, "ms": region_capture.ms, "bounds": bounds, "decoded_strip": decoded, "wanted_strip": wanted,
      "watch": facts}),
  )?;

  // From here on the commands move the real pointer and send real keys.
  let pad_center = center_of(pad);
  scenario.pointer_may_go(&[pad_center])?;
  let (moved, facts) = scenario.acting("input.moveMouse", &[arg(pad_center[0]), arg(pad_center[1])])?;
  let cursor = watch.snapshot().cursor;
  scenario.tally.expect(moved.completed(), format!("input.moveMouse failed: {}", moved.failure()));
  scenario.tally.expect(
    (cursor.0 - pad_center[0]).abs() <= 1 && (cursor.1 - pad_center[1]).abs() <= 1,
    format!("the pointer is at {cursor:?} after input.moveMouse to {pad_center:?}"),
  );
  scenario.tally.expect(facts["foreground_after"] == facts["foreground_before"], "input.moveMouse changed the foreground window");
  scenario.records.write(
    "input.moveMouse",
    json!({"exit": moved.exit, "ms": moved.ms, "to": pad_center, "pointer_after": [cursor.0, cursor.1], "watch": facts}),
  )?;

  // A real click on the receiver makes it the foreground window, as a user's click would. From now
  // on it keeps the foreground until the harness hands it back.
  target.request(json!({"cmd": "keep_foreground"}))?;
  let display_origin = primary_frame.map(|frame| (frame[0], frame[1])).unwrap_or_default();
  let button_center = layout.button_center;
  scenario.pointer_may_go(&[button_center])?;
  let before = target.state()?;
  let (clicked, facts) = scenario.acting(
    "input.clickPoint",
    &[
      arg(f64::from(button_center[0]) - display_origin.0),
      arg(f64::from(button_center[1]) - display_origin.1),
      arg("--target"),
      format!("display:{display_id}"),
    ],
  )?;
  std::thread::sleep(Duration::from_millis(200));
  let after = target.state()?;
  scenario.tally.expect(clicked.completed(), format!("input.clickPoint relative to the display failed: {}", clicked.failure()));
  scenario
    .tally
    .expect(after.clicks == before.clicks + 1, format!("the button counted {} display click(s), not 1", after.clicks - before.clicks));
  scenario.tally.expect(watch.snapshot().foreground == hwnd, "a real click on the receiver did not make it the foreground window");
  scenario.records.write(
    "input.clickPoint.display",
    json!({"exit": clicked.exit, "ms": clicked.ms, "selected_path": selected_path(&clicked), "display": display_id,
      "clicks": [before.clicks, after.clicks], "receiver_is_foreground": watch.snapshot().foreground == hwnd, "watch": facts}),
  )?;

  scenario.pointer_may_go(&[from, to])?;
  target.request(json!({"cmd": "reset_log"}))?;
  let (dragged, facts) = scenario.acting("input.drag", &[arg(from[0]), arg(from[1]), arg(to[0]), arg(to[1])])?;
  std::thread::sleep(Duration::from_millis(200));
  let drag = target.request(json!({"cmd": "state"}))?["pad"].clone();
  let cursor = watch.snapshot().cursor;
  let held_ms = drag["up_ms"].as_u64().zip(drag["down_ms"].as_u64()).map(|(up, down)| up.saturating_sub(down));
  scenario.tally.expect(dragged.completed(), format!("input.drag on the screen failed: {}", dragged.failure()));
  scenario.tally.expect(
    near(from, &drag["down"], 1.0) && near(to, &drag["up"], 1.0) && drag["moves"].as_u64().unwrap_or(0) >= 5,
    format!("the pad recorded the screen drag as {drag}, not from {from:?} to {to:?} with moves"),
  );
  scenario.tally.expect(held_ms.is_some_and(|held| held >= 200), format!("the button was down {held_ms:?} ms for a 300 ms drag"));
  scenario
    .tally
    .expect((cursor.0 - to[0]).abs() <= 1 && (cursor.1 - to[1]).abs() <= 1, format!("the pointer ended at {cursor:?}, not {to:?}"));
  scenario.records.write(
    "input.drag.screen",
    json!({"exit": dragged.exit, "ms": dragged.ms, "from": from, "to": to, "pad": drag, "button_down_ms": held_ms,
      "pointer_after": [cursor.0, cursor.1], "watch": facts}),
  )?;

  scenario.pointer_may_go(&[typing_center])?;
  let (clicked, facts) = scenario.acting("input.clickPoint", &[arg(typing_center[0]), arg(typing_center[1])])?;
  std::thread::sleep(Duration::from_millis(200));
  let typing = target.request(json!({"cmd": "state"}))?["typing"].clone();
  scenario.tally.expect(clicked.completed(), format!("input.clickPoint on the screen failed: {}", clicked.failure()));
  scenario.tally.expect(typing["focused"] == true, "a real click on the text box did not give it the focus");
  scenario.records.write(
    "input.clickPoint.screen",
    json!({"exit": clicked.exit, "ms": clicked.ms, "selected_path": selected_path(&clicked), "at": typing_center,
      "text_box_focused": typing["focused"], "text_box_clicks": typing["button_down"], "watch": facts}),
  )?;

  // Keyboard: each command adds known text to the box, so the text after each one says what the
  // application received. Text that is typed elsewhere cannot appear here.
  let steps: Vec<(&str, Vec<String>, &str)> = vec![
    ("input.typeText", vec![arg("auv typed")], "auv typed"),
    ("input.key", vec![arg("a")], "a"),
    ("input.key", vec![arg("shift+b")], "B"),
    ("input.keys", vec![arg("shift"), arg("c")], "C"),
    (
      "input.keys",
      vec![
        arg("d"),
        arg("--count"),
        arg(3),
        arg("--interval-ms"),
        arg(40),
      ],
      "ddd",
    ),
    ("input.holdKeys", vec![arg("shift"), arg("e"), arg("--duration-ms"), arg(400)], "E"),
    (
      "input.keyboard",
      vec![
        arg("--actions"),
        arg(r#"[{"kind":"press","keys":["f"]},{"kind":"type_text","text":"gh"}]"#),
      ],
      "fgh",
    ),
  ];
  let mut expected = String::new();
  let mut keyboard_ok = true;
  for (command, args, adds) in steps {
    if let Err(error) = scenario.keys_may_go(&mut target) {
      scenario.tally.expect(false, format!("{command} {args:?} skipped: {error:#}"));
      scenario.records.write("skipped", json!({"command": command, "args": args, "reason": format!("{error:#}")}))?;
      keyboard_ok = false;
      break;
    }
    target.request(json!({"cmd": "reset_log"}))?;
    let (sent, facts) = scenario.acting(command, &args)?;
    std::thread::sleep(Duration::from_millis(150));
    let typing = target.request(json!({"cmd": "state"}))?["typing"].clone();
    expected.push_str(adds);
    let text = typing["text"].as_str().unwrap_or_default().to_string();
    scenario.tally.expect(sent.completed(), format!("{command} {args:?} failed: {}", sent.failure()));
    if options.keep_ime {
      // What an active IME made of the key presses is the finding; the next step builds on it.
      expected = text.clone();
    }
    scenario.tally.expect(text == expected, format!("after {command} {args:?} the text box reads {text:?}, not {expected:?}"));
    // How long shift was down, from the key messages the text box itself received.
    let keys = typing["keys"].as_array().cloned().unwrap_or_default();
    let shift_at = |up: bool| keys.iter().find(|key| key["vk"] == VK_SHIFT && key["up"] == up).and_then(|key| key["ms"].as_u64());
    let shift_held_ms = shift_at(true).zip(shift_at(false)).map(|(up, down)| up.saturating_sub(down));
    if command == "input.holdKeys" && !options.keep_ime {
      scenario.tally.expect(
        shift_held_ms.is_some_and(|held| (380..=1_500).contains(&held)),
        format!("input.holdKeys held shift for {shift_held_ms:?} ms, not about 400"),
      );
    }
    scenario.records.write(
      command,
      json!({"args": args, "exit": sent.exit, "ms": sent.ms, "text_after": text, "expected": expected,
        "key_messages": keys.len(), "key_log": keys, "shift_held_ms": shift_held_ms, "result": sent.json.as_ref().map(|json| json["result"].clone()),
        "watch": facts}),
    )?;
  }

  // Paste has no Windows path yet: the commands must refuse and leave the text box alone.
  if keyboard_ok && scenario.keys_may_go(&mut target).is_ok() {
    for (command, args) in [
      ("input.pasteText", vec![arg("ij")]),
      (
        "input.keyboard",
        vec![
          arg("--actions"),
          arg(r#"[{"kind":"paste_text","text":"ij"}]"#),
        ],
      ),
    ] {
      let (refused, facts) = scenario.acting(command, &args)?;
      std::thread::sleep(Duration::from_millis(150));
      let text = target.request(json!({"cmd": "state"}))?["typing"]["text"].as_str().unwrap_or_default().to_string();
      scenario.tally.expect(refused.exit == Some(1), format!("{command} paste ended {:?} on Windows: {}", refused.exit, refused.failure()));
      scenario.tally.expect(text == expected, format!("{command} paste changed the text box to {text:?}"));
      scenario.tally.expect(facts["injected_keys"] == 0, format!("{command} paste injected keys"));
      scenario.records.write(
        "paste.refused",
        json!({"command": command, "exit": refused.exit, "failure": refused.failure(), "text_after": text, "watch": facts}),
      )?;
    }
  }

  // Hand the foreground back and put the pointer where it was, then check both.
  let handed = target.yield_foreground(start.foreground).unwrap_or(false);
  // SAFETY: moving the pointer back to where the user left it.
  unsafe {
    let _ = SetCursorPos(start.cursor.0, start.cursor.1);
  }
  std::thread::sleep(Duration::from_millis(300));
  let finish = watch.snapshot();
  let final_state = target.request(json!({"cmd": "state"}))?;
  let Scenario {
    mut tally,
    mut records,
    desk,
    ..
  } = scenario;
  tally.expect(handed && finish.foreground == start.foreground, "the foreground did not go back to the window that had it");
  tally.expect(finish.cursor == start.cursor, format!("the pointer is at {:?}, not back at {:?}", finish.cursor, start.cursor));
  tally.expect(tally.violated == 0, format!("{} command(s) that should not touch the desktop did", tally.violated));
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({"passed": passed, "quiet_ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "failures": tally.failures, "gap_findings": desk.gaps, "foreground_restored": finish.foreground == start.foreground,
      "pointer_restored": finish.cursor == start.cursor, "injected_mouse_total": finish.injected_mouse,
      "injected_keys_total": finish.injected_keys, "physical_mouse_total": finish.physical_mouse, "physical_keys_total": finish.physical_keys,
      "final_text": final_state["typing"]["text"], "final_clicks": final_state["clicks"]}),
  )?;
  drop(session);
  println!(
    "foreground: {} quiet commands ({} held, {} inconclusive, {} violated) and the acting commands; {}",
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("foreground.jsonl").display());
  Ok(passed)
}
