//! Diagnostic: does a posted click make the receiving window take the foreground, and does that
//! depend on how the receiver was started?
//!
//! A posted `WM_LBUTTONDOWN` makes a classic push button call `SetFocus`, which activates its
//! top-level window. Whether Windows then lets that window become the *foreground* window is
//! decided from the receiving process, not from AUV (see [`super::receiver_process`]). This probe
//! runs the same click against a receiver started as a descendant of this process and against
//! receivers started detached, and prints what the OS did.
//!
//! Each receiver is fresh, so the click is its first activation: the moment Windows may still let a
//! just-launched process take the foreground. A receiver that takes it hands it straight back and
//! counts it, which a sampled foreground can miss, so the receiver's own count decides. Whether
//! the user gave any input since the launch is printed with it.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use auv_driver_common::Driver;
use auv_driver_common::capture::Capture;
use auv_driver_common::input::WindowInput;
use auv_driver_windows::{ClickRequest, FrameRead, Outcome, WindowsDriver};

use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use serde_json::json;

use super::background::{Options, background_click, class_of, discover_by_hwnd, injected, placement, point_in_window, run_id, user_input};
use super::receiver_process::{Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub fn foreground_rights(options: &Options) -> Result<bool> {
  std::fs::create_dir_all(&options.output)?;
  let session = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let watch = DesktopWatch::start()?;
  let binary = receiver_binary()?;
  println!(
    "{:<26} {:>9} -> {:<9} {:<8} {:<42} receiver says",
    "variant", "fg before", "fg after", "verdict", "foreground events (ms, window)"
  );
  let mut stole = 0;
  let variants: Vec<(&str, bool)> = std::iter::once(("direct (control)", false)).chain((1..=5).map(|_| ("detached", true))).collect();
  for (name, detached) in variants {
    let (width, height) = (380, 300);
    let (left, top) = placement(0, width, height);
    let spec = ReceiverSpec {
      index: 0,
      title: "AUV foreground probe".into(),
      left,
      top,
      width,
      height,
      color: "2E6F9E".into(),
      sink: true,
    };
    let before = watch.snapshot();
    let mut receiver = if detached {
      Receiver::spawn_detached(&binary, &spec)?
    } else {
      Receiver::spawn(&binary, &spec, &options.output.join("probe-receiver.log"))?
    };
    let (window, _) = discover_by_hwnd(&session, receiver.hwnd())?;
    let button = point_in_window(&window, receiver.layout.button_center);
    std::thread::sleep(Duration::from_millis(300));
    let armed = watch.snapshot();
    // Sample the real foreground window in a tight loop across the click, so a change that
    // lasts a few milliseconds is not missed between two looks.
    let mut transitions: Vec<(u128, isize)> = Vec::new();
    let mut last = isize::MIN;
    let started = Instant::now();
    let mut clicked = false;
    let mut state = None;
    while started.elapsed() < Duration::from_millis(600) {
      let now = unsafe { GetForegroundWindow() }.0 as isize;
      if now != last {
        transitions.push((started.elapsed().as_micros(), now));
        last = now;
      }
      if !clicked && started.elapsed() >= Duration::from_millis(150) {
        clicked = true;
        transitions.push((started.elapsed().as_micros(), -1));
        session.window().click(&window, button, background_click()).map_err(|error| anyhow::anyhow!("click: {error}"))?;
        // The barrier: the reply comes after the receiver processed everything posted to it.
        state = Some(receiver.state()?);
        transitions.push((started.elapsed().as_micros(), -2));
      }
    }
    let state = state.expect("clicked");
    let after = watch.snapshot();
    let timeline = watch.timeline(armed.taken_ms, after.taken_ms);
    let took = state.foreground_taken > 0 || after.foreground == receiver.hwnd();
    let user_input = after.last_physical_ms > before.taken_ms;
    stole += usize::from(took);
    let events: Vec<_> = timeline
      .foreground_events
      .iter()
      .map(|event| {
        format!(
          "{}ms:{}{}",
          event.at - armed.taken_ms,
          event.window,
          if event.actual == event.window {
            ""
          } else {
            "(not foreground)"
          }
        )
      })
      .collect();
    let path: Vec<String> = transitions
      .iter()
      .map(|&(at, hwnd)| match hwnd {
        -1 => format!("[click @{}us]", at),
        -2 => format!("[barrier @{}us]", at),
        h if h == receiver.hwnd() => format!("RECEIVER@{}us", at),
        h => format!("{h}@{}us", at),
      })
      .collect();
    println!("    sampled foreground: {}", path.join("  "));
    println!(
      "{name:<26} {:>9} -> {:<9} {:<8} {:<42} clicks={} activated={} focus_set={} taken={} yielded={} user_input_since_launch={user_input}",
      armed.foreground,
      after.foreground,
      if took { "STOLE" } else { "held" },
      events.join(" "),
      state.clicks,
      state.activated,
      state.focus_set,
      state.foreground_taken,
      state.foreground_yielded
    );
    // Hand the foreground back to the user's window before the next variant, if the receiver
    // still has it.
    if after.foreground == receiver.hwnd() {
      let _ = receiver.yield_foreground(before.foreground);
    }
    drop(receiver);
    std::thread::sleep(Duration::from_millis(400));
  }
  println!("receivers that took the foreground: {stole}");
  Ok(true)
}

/// Diagnostic: does WGC keep delivering fresh frames for a window nobody is looking at?
///
/// Clicks the receiver's button, which repaints its label, and then captures the window by WGC and
/// by `PrintWindow` for two seconds. `PrintWindow` asks the window to paint itself, so it always
/// shows the truth; WGC shows whatever frame the compositor last handed over.
pub fn capture_freshness(options: &Options) -> Result<bool> {
  use super::background::describe_capture;

  std::fs::create_dir_all(&options.output)?;
  let session = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let binary = receiver_binary()?;
  for (name, sink) in [
    ("covered (bottom of the Z-order)", true),
    ("uncovered (top of the Z-order)", false),
  ] {
    let (width, height) = (380, 300);
    let (left, top) = placement(0, width, height);
    let spec = ReceiverSpec {
      index: 0,
      title: "AUV freshness probe".into(),
      left,
      top,
      width,
      height,
      color: "2E6F9E".into(),
      sink,
    };
    let mut receiver = Receiver::spawn_detached(&binary, &spec)?;
    let (window, _) = discover_by_hwnd(&session, receiver.hwnd())?;
    let button = point_in_window(&window, receiver.layout.button_center);
    println!("== {name}");
    let hash = |capture: auv_driver_common::error::DriverResult<auv_driver_common::capture::Capture>| -> String {
      capture
        .map(|c| describe_capture(&c, "2E6F9E")["hash"].as_str().unwrap_or("?").chars().take(6).collect())
        .unwrap_or_else(|e| format!("err:{e}"))
    };
    for round in 0..3 {
      let before = (hash(session.window().capture(&window)), hash(session.window().capture_wgc(&window)));
      session.window().click(&window, button, background_click()).map_err(|error| anyhow::anyhow!("click: {error}"))?;
      let state = receiver.state()?;
      let started = Instant::now();
      let mut line = Vec::new();
      let mut previous = (String::new(), String::new());
      while started.elapsed() < Duration::from_millis(1500) {
        let now = (hash(session.window().capture(&window)), hash(session.window().capture_wgc(&window)));
        if now != previous {
          line.push(format!("{}ms pw={} wgc={}", started.elapsed().as_millis(), now.0, now.1));
          previous = now;
        }
        std::thread::sleep(Duration::from_millis(40));
      }
      println!("  round {round}: clicks={} before pw={} wgc={} | after: {}", state.clicks, before.0, before.1, line.join(" | "));
    }
  }
  Ok(true)
}

/// Where a window's visible edges fall in a frame, from each capture path, after the window
/// moves and is resized. A frame must line up with the bounds it is reported with: a caller maps
/// pixels back to the screen with them. The left edge is the first column of the middle row that
/// is not transparent black (the invisible resize border captures as black).
pub fn capture_geometry(options: &Options) -> Result<bool> {
  std::fs::create_dir_all(&options.output)?;
  let session = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let binary = receiver_binary()?;
  let (width, height) = (380, 300);
  let (left, top) = placement(0, width, height);
  let spec = ReceiverSpec {
    index: 0,
    title: "AUV geometry probe".into(),
    left,
    top,
    width,
    height,
    color: "2E6F9E".into(),
    sink: true,
  };
  let mut receiver = Receiver::spawn_detached(&binary, &spec)?;
  let (window, _) = discover_by_hwnd(&session, receiver.hwnd())?;
  let mut background = session.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
  let id = background.attach(&window).map_err(|error| anyhow::anyhow!("attach: {error}"))?;
  let button = point_in_window(&window, receiver.layout.button_center);
  let edges = |capture: &Capture| -> (u32, u32, Option<u32>, Option<u32>) {
    let y = capture.image.height() / 2;
    let lit = |x: u32| capture.image.get_pixel_checked(x, y).is_some_and(|p| p[0] > 8 || p[1] > 8 || p[2] > 8);
    let width = capture.image.width();
    (width, capture.image.height(), (0..width).find(|&x| lit(x)), (0..width).rev().find(|&x| lit(x)))
  };
  let steps: Vec<(&str, Option<serde_json::Value>)> = vec![
    ("start", None),
    ("move", Some(serde_json::json!({"cmd": "move", "left": left - 160, "top": top - 90}))),
    ("move back", Some(serde_json::json!({"cmd": "move", "left": left, "top": top}))),
    ("resize", Some(serde_json::json!({"cmd": "resize", "width": 460, "height": 340}))),
    ("move again", Some(serde_json::json!({"cmd": "move", "left": left - 200, "top": top - 60}))),
  ];
  println!(
    "{:<11} {:<26} {:<26} {:<34} {:<34} printwindow",
    "step", "window rect", "session bounds", "stream frame (w h left right)", "legacy wgc"
  );
  for (label, command) in steps {
    if let Some(command) = command {
      receiver.request(command)?;
    }
    std::thread::sleep(Duration::from_millis(300));
    // A click repaints the window, so every path has a frame from after the change.
    let _ = background.click(id, button, ClickRequest::default());
    receiver.state()?;
    std::thread::sleep(Duration::from_millis(200));
    let mut stream = None;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(800) {
      match background.capture(id) {
        Outcome::Done {
          value: FrameRead::Fresh { capture, .. },
        } => stream = Some(capture),
        Outcome::Done {
          value: FrameRead::Unchanged { .. },
        } if stream.is_some() => break,
        _ => {}
      }
      std::thread::sleep(Duration::from_millis(20));
    }
    let (now, _) = discover_by_hwnd(&session, receiver.hwnd())?;
    let describe = |capture: Option<&Capture>| {
      capture.map(|c| format!("{:?} bounds@{},{}", edges(c), c.bounds.origin.x, c.bounds.origin.y)).unwrap_or_else(|| "none".into())
    };
    let legacy = session.window().capture_wgc(&now).ok();
    let printed = session.window().capture(&now).ok();
    let rect = super::background::view_of(receiver.hwnd()).rect;
    let bounds = background.targets()[0].frame;
    println!(
      "{label:<11} {:<26} {:<26} {:<34} {:<34} {}",
      format!("{rect:?}"),
      format!("{},{} {}x{}", bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height),
      describe(stream.as_ref()),
      describe(legacy.as_ref()),
      describe(printed.as_ref())
    );
  }
  Ok(true)
}

/// Diagnostic: what reaches the desktop while nothing of AUV runs. Only the witness is started, with
/// no receiver and no session, so any injected input it counts came from another program. Run it
/// beside normal use of the machine to tell whether injected input seen during an evaluation is
/// the machine's own.
pub fn quiet(output: &Path, minutes: f64) -> Result<bool> {
  std::fs::create_dir_all(output)?;
  let mut records = Records::create(&output.join("quiet.jsonl"), &run_id(), "quiet")?;
  let watch = DesktopWatch::start()?;
  let total = Duration::from_secs_f64(minutes * 60.0);
  let started = Instant::now();
  let mut last = watch.snapshot();
  while started.elapsed() < total {
    std::thread::sleep(Duration::from_secs(1));
    let now = watch.snapshot();
    if now.injected_keys != last.injected_keys || now.injected_mouse != last.injected_mouse {
      let timeline = watch.timeline(last.taken_ms, now.taken_ms);
      let seconds = started.elapsed().as_secs_f64();
      records.write(
        "injected",
        json!({"at_s": seconds, "injected_input": injected(&timeline, last.taken_ms), "user_input_ms": user_input(&timeline, last.taken_ms),
          "last_physical_input_ms_before": last.taken_ms as i64 - now.last_physical_ms as i64, "foreground_class": class_of(now.foreground)}),
      )?;
      println!(
        "{seconds:>7.1}s: {} injected key event(s), {} injected mouse event(s)",
        now.injected_keys - last.injected_keys,
        now.injected_mouse - last.injected_mouse
      );
    }
    last = now;
  }
  let finish = watch.snapshot();
  records.write(
    "summary",
    json!({"minutes": minutes, "injected_keys_total": finish.injected_keys, "injected_mouse_total": finish.injected_mouse,
      "physical_keys_total": finish.physical_keys, "physical_mouse_total": finish.physical_mouse}),
  )?;
  println!(
    "quiet: {minutes} minute(s) with nothing of AUV running: injected keys {}, injected mouse {}, physical keys {}, physical mouse {}",
    finish.injected_keys, finish.injected_mouse, finish.physical_keys, finish.physical_mouse
  );
  Ok(true)
}
