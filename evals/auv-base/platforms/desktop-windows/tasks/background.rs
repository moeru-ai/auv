//! Background-session scenarios against `win32-receiver` windows.
//!
//! Every operation is measured twice over: what the driver returned, and what the desktop and the
//! receiving application showed afterwards. Delivery is never taken as proof of effect, and a
//! changed foreground window or cursor is only a failure when the user's own input does not
//! explain it.
//!
//! Posted messages are asynchronous: the receiving application reacts to them after the driver
//! has returned. So each operation's window closes only after a *settle barrier* (a state query
//! that the receiver answers behind everything already posted to it), and the gap between one
//! operation and the next is judged as well. A foreground change that lands between two
//! operations is still a change.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use auv_driver_common::geometry::WindowPoint;
use auv_driver_common::input::{
  ClickModifiers, ClickOptions, DisturbanceLevel, InputActionResult, InputDeliveryPath, InputPolicy, Scroll, ScrollDeliveryCandidate,
  ScrollDeliveryStrategy, ScrollOptions, TypeTextOptions, WindowInput,
};
use auv_driver_common::window::Window;
use auv_driver_common::{Driver, DriverError, InputTarget, KeyboardInput, PressKeysOptions};
use auv_driver_windows::{WindowsDriver, WindowsDriverSession};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
  GW_HWNDPREV, GetClassNameW, GetSystemMetrics, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, SM_CXSCREEN, SM_CYSCREEN,
  SPI_GETFOREGROUNDLOCKTIMEOUT, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use super::receiver_process::{Receiver, ReceiverSpec, State, receiver_binary};
use super::records::Records;
use super::watch::{DesktopWatch, Snapshot, Timeline, Verdict};

/// One distinct background color per receiver, so a capture can be matched to its window.
const PALETTE: [&str; 5] = ["2E6F9E", "9E4B2E", "3F8F4F", "8A4F9E", "9E8A2E"];

pub struct Options {
  pub output: PathBuf,
  pub repetitions: u32,
  /// Leave the receiver at the top of the Z-order instead of sinking it under ordinary windows.
  pub visible: bool,
  /// Start the receiver as a piped child of this process, as the control that shows how a
  /// descendant of the foreground application behaves. Off: start it detached.
  pub piped: bool,
}

/// What the harness can see of one target window from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
  /// Top-level windows above it in the Z-order. A raise makes this smaller.
  pub z_rank: usize,
  pub iconic: bool,
  pub rect: [i32; 4],
}

pub fn view_of(hwnd: isize) -> View {
  let hwnd = HWND(hwnd as *mut core::ffi::c_void);
  let mut rect = RECT::default();
  let mut rank = 0;
  // SAFETY: read-only window queries on a handle that may have gone away; each fails harmlessly.
  unsafe {
    let _ = GetWindowRect(hwnd, &mut rect);
    let mut cursor = hwnd;
    while let Ok(above) = GetWindow(cursor, GW_HWNDPREV) {
      rank += 1;
      cursor = above;
      if rank > 4096 {
        break;
      }
    }
    View {
      z_rank: rank,
      iconic: IsIconic(hwnd).as_bool(),
      rect: [rect.left, rect.top, rect.right, rect.bottom],
    }
  }
}

pub(super) struct Measured<T> {
  pub(super) value: T,
  pub(super) latency_ms: f64,
  pub(super) before: Snapshot,
  pub(super) after: Snapshot,
  pub(super) verdict: Verdict,
  pub(super) timeline: Timeline,
  pub(super) view_before: View,
  pub(super) view_after: View,
}

/// Looks at the desktop around each operation, and between them.
pub(super) struct Desk<'a> {
  watch: &'a DesktopWatch,
  target: isize,
  last: Snapshot,
  /// Changes found in the gaps between operations.
  pub(super) gaps: Vec<Value>,
}

impl<'a> Desk<'a> {
  pub(super) fn new(watch: &'a DesktopWatch, target: isize) -> Self {
    Self {
      watch,
      target,
      last: watch.snapshot(),
      gaps: Vec::new(),
    }
  }

  /// Runs `operation`, then `settle`, between two looks at the desktop. Only `operation` is timed.
  /// The gap since the previous operation is judged too.
  pub(super) fn measure<T, S>(&mut self, operation: impl FnOnce() -> T, settle: impl FnOnce() -> S) -> (Measured<T>, S) {
    self.measure_at(self.target, operation, settle)
  }

  /// [`Self::measure`] for an operation on `target`, whichever window the desk was made for.
  pub(super) fn measure_at<T, S>(&mut self, target: isize, operation: impl FnOnce() -> T, settle: impl FnOnce() -> S) -> (Measured<T>, S) {
    self.target = target;
    let before = self.watch.snapshot();
    let (gap_verdict, gap_timeline) = self.watch.judge(&self.last, &before);
    if !matches!(gap_verdict, Verdict::Held) {
      self.gaps.push(json!({"verdict": gap_verdict, "gap_ms": before.taken_ms - self.last.taken_ms,
        "foreground_before": self.last.foreground.to_string(), "foreground_after": before.foreground.to_string(),
        "foreground_events": events(&gap_timeline, self.last.taken_ms, &[self.target]),
        "cursor": [[self.last.cursor.0, self.last.cursor.1], [before.cursor.0, before.cursor.1]],
        "last_physical_input_ms_before_gap": self.last.taken_ms as i64 - before.last_physical_ms as i64,
        "user_input_ms": user_input(&gap_timeline, self.last.taken_ms), "injected_input": injected(&gap_timeline, self.last.taken_ms)}));
    }
    let view_before = view_of(self.target);
    let started = Instant::now();
    let value = operation();
    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let settled = settle();
    let after = self.watch.snapshot();
    let view_after = view_of(self.target);
    let (verdict, timeline) = self.watch.judge(&before, &after);
    self.last = after;
    (
      Measured {
        value,
        latency_ms,
        verdict,
        timeline,
        before,
        after,
        view_before,
        view_after,
      },
      settled,
    )
  }

  pub(super) fn gap_violations(&self) -> usize {
    self.gaps.iter().filter(|gap| gap["verdict"]["verdict"] == "violated").count()
  }
}

/// The foreground events in `timeline`, each with the owner of the window that really was
/// foreground: the process of one of `targets` (`"target <index>"`), this harness, or another
/// process; and that window's class. Its title is not read.
pub(super) fn events(timeline: &Timeline, since_ms: u64, targets: &[isize]) -> Value {
  let processes: Vec<u32> = targets.iter().map(|&hwnd| process_of(hwnd)).collect();
  let owner = |process: u32| match processes.iter().position(|&target| target != 0 && target == process) {
    Some(index) => format!("target {index}"),
    None if process == std::process::id() => "harness".to_string(),
    None => "other process".to_string(),
  };
  Value::Array(
    timeline
      .foreground_events
      .iter()
      .map(|event| {
        json!({"at_ms": event.at.saturating_sub(since_ms), "window": event.window.to_string(), "is_target": targets.contains(&event.window),
          "actual_foreground": event.actual.to_string(), "became_foreground": event.actual == event.window,
          "owner": owner(event.process), "class": class_of(event.actual)})
      })
      .collect(),
  )
}

/// The injected events in `timeline`: when, keyboard or mouse, the injector's tag and integrity.
pub(super) fn injected(timeline: &Timeline, since_ms: u64) -> Value {
  Value::Array(
    timeline
      .injected_input
      .iter()
      .map(|event| {
        json!({"at_ms": event.at.saturating_sub(since_ms), "device": if event.keyboard { "keyboard" } else { "mouse" },
          "tag": format!("{:#x}", event.tag), "lower_integrity": event.lower_integrity,
          "key_kind": event.key.map(|(kind, _)| kind), "key_up": event.key.map(|(_, up)| up)})
      })
      .collect(),
  )
}

/// The user's own clicks and key presses in `timeline`, as offsets from `since_ms` (negative:
/// before it).
pub(super) fn user_input(timeline: &Timeline, since_ms: u64) -> Value {
  json!(timeline.discrete_input.iter().map(|&at| at as i64 - since_ms as i64).collect::<Vec<_>>())
}

fn process_of(hwnd: isize) -> u32 {
  let mut process = 0;
  // SAFETY: a read-only query; a handle that has gone away leaves `process` at zero.
  unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut core::ffi::c_void), Some(&mut process)) };
  process
}

pub(super) fn class_of(hwnd: isize) -> String {
  let mut name = [0u16; 256];
  // SAFETY: a read-only query into a buffer of the length passed; a closed window yields zero.
  let length = unsafe { GetClassNameW(HWND(hwnd as *mut core::ffi::c_void), &mut name) }.max(0) as usize;
  String::from_utf16_lossy(&name[..length])
}

pub(super) fn invariants<T>(measured: &Measured<T>, target: isize) -> Value {
  let (before, after) = (&measured.before, &measured.after);
  let mut value = serde_json::to_value(&measured.verdict).unwrap_or(Value::Null);
  value["foreground_before"] = json!(before.foreground.to_string());
  value["foreground_after"] = json!(after.foreground.to_string());
  value["foreground_event_log"] = events(&measured.timeline, before.taken_ms, &[target]);
  value["injected_input"] = injected(&measured.timeline, before.taken_ms);
  value["cursor_before"] = json!([before.cursor.0, before.cursor.1]);
  value["cursor_after"] = json!([after.cursor.0, after.cursor.1]);
  value["last_physical_input_ms_before"] = json!(before.taken_ms as i64 - after.last_physical_ms as i64);
  value["injected_mouse"] = json!(after.injected_mouse - before.injected_mouse);
  value["injected_keys"] = json!(after.injected_keys - before.injected_keys);
  value["physical_mouse"] = json!(after.physical_mouse - before.physical_mouse);
  value["physical_keys"] = json!(after.physical_keys - before.physical_keys);
  value["target_z_rank"] = json!([measured.view_before.z_rank, measured.view_after.z_rank]);
  value["target_rect_unchanged"] = json!(measured.view_before.rect == measured.view_after.rect);
  value["target_iconic"] = json!([measured.view_before.iconic, measured.view_after.iconic]);
  value
}

pub(super) fn error_kind(error: &DriverError) -> &'static str {
  match error {
    DriverError::Unsupported { .. } => "unsupported",
    DriverError::NotFound { .. } => "not_found",
    DriverError::PermissionDenied { .. } => "permission_denied",
    DriverError::InvalidInput { .. } => "invalid_input",
    DriverError::StaleUiReference { .. } => "stale_ui_reference",
    DriverError::RoleMismatch { .. } => "role_mismatch",
    DriverError::Backend { .. } => "backend",
  }
}

pub(super) fn failure(error: &DriverError) -> Value {
  json!({"status": "failed", "error": {"kind": error_kind(error), "message": error.to_string()}})
}

pub(super) fn delivery(result: &InputActionResult) -> Value {
  json!({
    "status": "delivered",
    "selected_path": result.selected_path.as_str(),
    "attempts": result.attempts.iter().map(|a| json!({"path": a.path.as_str(), "succeeded": a.succeeded, "message": a.message})).collect::<Vec<_>>(),
    "verified": result.verified,
    "mouse_disturbance": result.mouse_disturbance.as_str(),
    "focus_disturbance": result.focus_disturbance.as_str(),
    "clipboard_disturbance": result.clipboard_disturbance.as_str(),
  })
}

/// A delivery that stayed in the background: window-targeted, no foreground attempt, no
/// disturbance, and honest about not having verified anything.
pub(super) fn is_undisturbing(result: &InputActionResult, path: InputDeliveryPath) -> bool {
  result.selected_path == path
    && !result.verified
    && result.mouse_disturbance == DisturbanceLevel::None
    && result.focus_disturbance == DisturbanceLevel::None
    && result.clipboard_disturbance == DisturbanceLevel::None
    && result.attempts.iter().all(|attempt| attempt.path != InputDeliveryPath::ForegroundSystemEvents)
}

pub fn background_click() -> ClickOptions {
  ClickOptions {
    policy: InputPolicy::BackgroundOnly,
    ..ClickOptions::default()
  }
}

fn wheel_only() -> ScrollDeliveryStrategy {
  ScrollDeliveryStrategy {
    candidates: vec![ScrollDeliveryCandidate::WindowTargetedWheel],
  }
}

fn background_scroll(strategy: ScrollDeliveryStrategy) -> ScrollOptions {
  ScrollOptions {
    policy: InputPolicy::BackgroundOnly,
    delivery_strategy: strategy,
    settle: std::time::Duration::ZERO,
  }
}

pub(super) fn run_id() -> String {
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
  format!("{seconds}-{}", std::process::id())
}

pub fn point_in_window(window: &Window, screen: [i32; 2]) -> WindowPoint {
  WindowPoint::new(f64::from(screen[0]) - window.frame.origin.x, f64::from(screen[1]) - window.frame.origin.y)
}

/// Where the `index`th receiver goes: the lower right of the primary display, cascaded.
pub fn placement(index: usize, width: i32, height: i32) -> (i32, i32) {
  // SAFETY: read-only metrics queries.
  let (screen_width, screen_height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
  let step = i32::try_from(index).unwrap_or(0);
  ((screen_width - width - 40 - step * 36).max(0), (screen_height - height - 90 - step * 28).max(0))
}

/// Facts about this machine that decide what the OS lets a window do, so a result can be read
/// against them.
pub(super) fn system_facts() -> Value {
  let mut lock_timeout = 0u32;
  let mut input = LASTINPUTINFO {
    cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
    dwTime: 0,
  };
  // SAFETY: read-only system queries with writable out parameters.
  let idle_ms = unsafe {
    let _ = SystemParametersInfoW(
      SPI_GETFOREGROUNDLOCKTIMEOUT,
      0,
      Some(&mut lock_timeout as *mut u32 as *mut core::ffi::c_void),
      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    GetLastInputInfo(&mut input).as_bool().then(|| GetTickCount().wrapping_sub(input.dwTime))
  };
  json!({"foreground_lock_timeout_ms": lock_timeout, "idle_ms_at_start": idle_ms})
}

pub fn spawn_receiver(options: &Options, index: usize) -> Result<Receiver> {
  receiver_spec(index, !options.visible).and_then(|spec| {
    let binary = receiver_binary()?;
    if options.piped {
      Receiver::spawn(&binary, &spec, &options.output.join(format!("receiver-{}.log", index + 1)))
    } else {
      Receiver::spawn_detached(&binary, &spec)
    }
  })
}

/// A detached receiver, covered by ordinary windows when `sink` is set.
pub(super) fn spawn_receiver_with(_output: &std::path::Path, index: usize, sink: bool) -> Result<Receiver> {
  Receiver::spawn_detached(&receiver_binary()?, &receiver_spec(index, sink)?)
}

fn receiver_spec(index: usize, sink: bool) -> Result<ReceiverSpec> {
  let (width, height) = (380, 300);
  let (left, top) = placement(index, width, height);
  Ok(ReceiverSpec {
    index,
    title: format!("AUV eval receiver {}", index + 1),
    left,
    top,
    width,
    height,
    color: PALETTE[index % PALETTE.len()].to_string(),
    sink,
  })
}

/// The receiver as the driver discovers it. This is the one discovery a session pays.
pub fn discover_by_hwnd(session: &WindowsDriverSession, hwnd: isize) -> Result<(Window, f64)> {
  let started = Instant::now();
  loop {
    let windows = session.window().list().map_err(|error| anyhow::anyhow!("window discovery failed: {error}"))?;
    if let Some(window) = windows.into_iter().find(|window| window.reference.id == hwnd.to_string()) {
      return Ok((window, started.elapsed().as_secs_f64() * 1000.0));
    }
    if started.elapsed().as_secs() >= 5 {
      bail!("driver window discovery never listed the receiver window {hwnd}");
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
  }
}

/// Share of `window`'s frame covered by windows above it in the Z-order, estimated on a grid.
/// `EnumWindows`, and so the driver's list, runs top to bottom.
pub(super) fn occlusion(session: &WindowsDriverSession, window: &Window) -> f64 {
  let Ok(windows) = session.window().list() else {
    return f64::NAN;
  };
  let above: Vec<_> = windows.iter().take_while(|other| other.reference.id != window.reference.id).collect();
  let frame = window.frame;
  let steps = 16;
  let mut covered = 0;
  for row in 0..steps {
    for column in 0..steps {
      let x = frame.origin.x + frame.size.width * (f64::from(column) + 0.5) / f64::from(steps);
      let y = frame.origin.y + frame.size.height * (f64::from(row) + 0.5) / f64::from(steps);
      if above.iter().any(|other| {
        let r = other.frame;
        x >= r.origin.x && x < r.origin.x + r.size.width && y >= r.origin.y && y < r.origin.y + r.size.height
      }) {
        covered += 1;
      }
    }
  }
  f64::from(covered) / f64::from(steps * steps)
}

fn rgb(hex: &str) -> [u8; 3] {
  let value = u32::from_str_radix(hex, 16).unwrap_or(0);
  [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

/// What a capture shows: how much of it is the receiver's own background color (so it is this
/// window and not whatever covers it), how much is black, and a content hash.
pub fn describe_capture(capture: &auv_driver_common::capture::Capture, color: &str) -> Value {
  let target = rgb(color);
  let (mut matching, mut black) = (0usize, 0usize);
  let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
  let pixels = capture.image.as_raw();
  for pixel in pixels.as_chunks::<4>().0 {
    if pixel[0].abs_diff(target[0]) <= 3 && pixel[1].abs_diff(target[1]) <= 3 && pixel[2].abs_diff(target[2]) <= 3 {
      matching += 1;
    }
    if pixel[0] < 11 && pixel[1] < 11 && pixel[2] < 11 {
      black += 1;
    }
  }
  for word in pixels.chunks(8) {
    let mut bytes = [0u8; 8];
    bytes[..word.len()].copy_from_slice(word);
    hash = (hash ^ u64::from_le_bytes(bytes)).wrapping_mul(0x0000_0100_0000_01b3);
  }
  let total = (pixels.len() / 4).max(1) as f64;
  json!({
    "backend": capture.backend,
    "width": capture.image.width(),
    "height": capture.image.height(),
    "own_color_ratio": matching as f64 / total,
    "black_ratio": black as f64 / total,
    "hash": format!("{hash:016x}"),
  })
}

pub(super) fn effect(observed: bool, detail: Value) -> Value {
  json!({"observed": observed, "detail": detail})
}

/// Tallies for the gate decision.
#[derive(Default)]
pub(super) struct Tally {
  pub(super) ops: u64,
  pub(super) held: u64,
  pub(super) inconclusive: u64,
  pub(super) violated: u64,
  pub(super) failures: Vec<String>,
}

impl Tally {
  pub(super) fn record<T>(&mut self, measured: &Measured<T>) {
    self.ops += 1;
    match measured.verdict {
      Verdict::Held => self.held += 1,
      Verdict::Inconclusive { .. } => self.inconclusive += 1,
      Verdict::Violated { .. } => self.violated += 1,
    }
  }

  pub(super) fn expect(&mut self, condition: bool, what: impl Into<String>) {
    if !condition {
      self.failures.push(what.into());
    }
  }
}

/// Phase 1: one background window. Click, wheel and capture reach it without the foreground
/// window, the cursor or the global input stream changing, and everything the driver cannot do
/// in the background fails with a typed error and leaves the desktop alone.
pub fn single(options: &Options) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let run = run_id();
  let mut records = Records::create(&options.output.join("single.jsonl"), &run, "single")?;
  let watch = DesktopWatch::start()?;
  let session = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let mut tally = Tally::default();
  let reps = options.repetitions.max(1);

  let launched = watch.snapshot();
  let mut receiver = spawn_receiver(options, 0)?;
  let color = receiver.spec.color.clone();
  let hwnd = receiver.hwnd();
  let (window, discovery_ms) = discover_by_hwnd(&session, hwnd)?;
  let button = point_in_window(&window, receiver.layout.button_center);
  let edit = point_in_window(&window, receiver.layout.edit_center);

  // Warm up before measuring: Windows can let a process it has just launched take the foreground
  // on its first activation, which says nothing about the driver. The receiver hands it back and
  // counts it; what happened is recorded, and every later takeover fails the run.
  let warm_before = watch.snapshot();
  let warm_click = session.window().click(&window, button, background_click()).is_ok();
  std::thread::sleep(std::time::Duration::from_millis(150));
  let warm_state = receiver.state()?;
  let warm_after = watch.snapshot();
  let warm_up = json!({"click_delivered": warm_click, "user_input_since_launch": warm_after.last_physical_ms > launched.taken_ms,
    "foreground_taken": warm_state.foreground_taken, "foreground_yielded": warm_state.foreground_yielded,
    "verdict": watch.judge(&warm_before, &warm_after).0});
  let start = watch.snapshot();
  let start_view = view_of(hwnd);
  records.write(
    "setup",
    json!({
      "receiver": {"hwnd": receiver.layout.hwnd, "pid": receiver.layout.pid, "detached": receiver.is_detached(), "sunk_under_ordinary_windows": !options.visible},
      "frame": [window.frame.origin.x, window.frame.origin.y, window.frame.size.width, window.frame.size.height],
      "discovery_ms": discovery_ms,
      "foreground": start.foreground.to_string(),
      "receiver_is_foreground": start.foreground == hwnd,
      "occluded_fraction": occlusion(&session, &window),
      "target_z_rank": start_view.z_rank,
      "cursor": [start.cursor.0, start.cursor.1],
      "system": system_facts(),
      "repetitions": reps,
      "warm_up": warm_up,
    }),
  )?;
  tally.expect(start.foreground != hwnd, "the receiver started as the foreground window");

  let target = json!({"hwnd": receiver.layout.hwnd, "pid": receiver.layout.pid});
  let mut desk = Desk::new(&watch, hwnd);

  // Clicks: delivered by window message, and the button's own press logic must run.
  for rep in 0..reps {
    let before = receiver.state()?;
    let (measured, after) = desk.measure(|| session.window().click(&window, button, background_click()), || receiver.state());
    let after = after?;
    tally.record(&measured);
    let (outcome, delivered) = match &measured.value {
      Ok(result) => (delivery(result), is_undisturbing(result, InputDeliveryPath::WindowTargetedMouse)),
      Err(error) => (failure(error), false),
    };
    let clicked = after.clicks == before.clicks + 1;
    let took = after.foreground_taken - before.foreground_taken;
    tally.expect(delivered, format!("click {rep}: not an undisturbing window-targeted mouse delivery"));
    tally.expect(clicked, format!("click {rep}: the button did not report BN_CLICKED"));
    tally.expect(took == 0, format!("click {rep}: the receiver took the foreground (and handed it back)"));
    records.write(
      "op",
      json!({
        "op": "click", "rep": rep, "target": target, "outcome": outcome, "latency_ms": measured.latency_ms,
        "effect": effect(clicked, json!({"clicks_before": before.clicks, "clicks_after": after.clicks, "button_down": after.raw.button_down - before.raw.button_down,
          "receiver_activated": after.activated, "receiver_focus_set": after.focus_set, "receiver_self_reports_foreground": after.foreground,
          "receiver_took_foreground": took})),
        "invariants": invariants(&measured, hwnd),
      }),
    )?;
  }

  // Wheel: down, then back up, each notch moving the edit's first visible line.
  for (direction, delta, label) in [(1i64, 100.0, "scroll_down"), (-1, -100.0, "scroll_up")] {
    for rep in 0..reps {
      let before = receiver.state()?;
      let (measured, after) = desk
        .measure(|| session.window().scroll(&window, edit, Scroll::new(0.0, delta), background_scroll(wheel_only())), || receiver.state());
      let after = after?;
      tally.record(&measured);
      let (outcome, delivered) = match &measured.value {
        Ok(result) => (delivery(result), is_undisturbing(result, InputDeliveryPath::WindowTargetedWheel)),
        Err(error) => (failure(error), false),
      };
      let moved = (after.first_line - before.first_line).signum() == direction;
      let took = after.foreground_taken - before.foreground_taken;
      tally.expect(delivered, format!("{label} {rep}: not an undisturbing window-targeted wheel delivery"));
      tally.expect(moved, format!("{label} {rep}: the edit did not scroll the right way"));
      tally.expect(took == 0, format!("{label} {rep}: the receiver took the foreground (and handed it back)"));
      records.write(
        "op",
        json!({
          "op": label, "rep": rep, "target": target, "outcome": outcome, "latency_ms": measured.latency_ms,
          "effect": effect(moved, json!({"first_line_before": before.first_line, "first_line_after": after.first_line, "edit_wheel": after.raw.edit_wheel - before.raw.edit_wheel,
            "receiver_took_foreground": took})),
          "invariants": invariants(&measured, hwnd),
        }),
      )?;
    }
  }

  // Capture: both backends see this window's own pixels, covered or not.
  for label in ["capture_wgc", "capture_printwindow"] {
    let mut previous_hash = String::new();
    for rep in 0..reps {
      let (measured, ()) = desk.measure(
        || {
          if label == "capture_wgc" {
            session.window().capture_wgc(&window)
          } else {
            session.window().capture(&window)
          }
        },
        || (),
      );
      tally.record(&measured);
      let (outcome, shown) = match &measured.value {
        Ok(capture) => {
          let shown = describe_capture(capture, &color);
          (json!({"status": "captured", "capture": shown.clone()}), Some(shown))
        }
        Err(error) => (failure(error), None),
      };
      let own = shown.as_ref().and_then(|s| s["own_color_ratio"].as_f64()).unwrap_or(0.0);
      let black = shown.as_ref().and_then(|s| s["black_ratio"].as_f64()).unwrap_or(1.0);
      let hash = shown.as_ref().and_then(|s| s["hash"].as_str()).unwrap_or_default().to_string();
      tally.expect(
        own > 0.05 && black < 0.5,
        format!("{label} {rep}: the capture does not show the receiver (own {own:.3}, black {black:.3})"),
      );
      records.write(
        "op",
        json!({
          "op": label, "rep": rep, "target": target, "outcome": outcome, "latency_ms": measured.latency_ms,
          "effect": effect(own > 0.05, json!({"unchanged_since_previous": !previous_hash.is_empty() && previous_hash == hash})),
          "invariants": invariants(&measured, hwnd),
        }),
      )?;
      previous_hash = hash;
    }
  }

  // A capture must change when the window's content does: capture, click, capture again.
  // NOTICE: only PrintWindow is judged. The one-shot WGC capture reads the frame pool in order and
  // can return the window as it was before the click (`TODO(wgc-capture-latest-frame)` in the
  // driver), so its result is recorded as a finding. Sessions read through a frame stream instead,
  // which `five`, `bench` and `faults` judge.
  for label in ["capture_printwindow", "capture_wgc"] {
    let take = || {
      if label == "capture_wgc" {
        session.window().capture_wgc(&window)
      } else {
        session.window().capture(&window)
      }
    };
    let (before_capture, ()) = desk.measure(take, || ());
    tally.record(&before_capture);
    let before_hash = before_capture.value.as_ref().map(|c| describe_capture(c, &color)["hash"].clone()).unwrap_or(Value::Null);
    let (click, _) = desk.measure(|| session.window().click(&window, button, background_click()), || receiver.state());
    tally.record(&click);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let (after_capture, ()) = desk.measure(take, || ());
    tally.record(&after_capture);
    let after_hash = after_capture.value.as_ref().map(|c| describe_capture(c, &color)["hash"].clone()).unwrap_or(Value::Null);
    let changed = before_hash != after_hash && !before_hash.is_null() && !after_hash.is_null();
    let judged = label == "capture_printwindow";
    if judged {
      tally.expect(changed, "a PrintWindow capture after a click did not differ from the one before it");
    }
    records.write(
      "check",
      json!({"check": "capture_changes_with_content", "backend": label, "judged": judged, "before": before_hash, "after": after_hash, "observed": changed}),
    )?;
  }

  // Everything the driver cannot do in the background fails with a typed error, delivers
  // nothing, and leaves the desktop alone.
  let before_negatives = receiver.state()?;
  let press = |keys: &[&str]| KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: keys.iter().map(|key| (*key).to_string()).collect(),
      ..Default::default()
    },
    policy: InputPolicy::BackgroundOnly,
  };
  let type_text = KeyboardInput::TypeText {
    text: "AUV background text".into(),
    options: TypeTextOptions {
      policy: InputPolicy::BackgroundOnly,
      ..Default::default()
    },
  };
  let alt_click = ClickOptions {
    modifiers: ClickModifiers {
      alt: true,
      ..Default::default()
    },
    ..background_click()
  };
  type Negative<'a> = (&'static str, &'static str, Box<dyn Fn() -> Result<(), DriverError> + 'a>);
  let negatives: Vec<Negative> = vec![
    (
      "keyboard_press_to_window",
      "unsupported",
      Box::new(|| {
        session
          .input()
          .input_keyboard(&InputTarget::Window(window.clone()), vec![press(&["a"])], false)
          .map(|_| ())
          .map_err(|error| error.cause)
      }),
    ),
    (
      "keyboard_text_to_window",
      "unsupported",
      Box::new(|| {
        session
          .input()
          .input_keyboard(&InputTarget::Window(window.clone()), vec![type_text.clone()], false)
          .map(|_| ())
          .map_err(|error| error.cause)
      }),
    ),
    (
      "scroll_ax_only",
      "unsupported",
      Box::new(|| {
        session
          .window()
          .scroll(
            &window,
            edit,
            Scroll::new(0.0, 100.0),
            background_scroll(ScrollDeliveryStrategy {
              candidates: vec![ScrollDeliveryCandidate::AxScroll],
            }),
          )
          .map(|_| ())
      }),
    ),
    (
      "scroll_foreground_only_under_background_policy",
      "unsupported",
      Box::new(|| {
        session
          .window()
          .scroll(
            &window,
            edit,
            Scroll::new(0.0, 100.0),
            background_scroll(ScrollDeliveryStrategy {
              candidates: vec![ScrollDeliveryCandidate::ForegroundHid],
            }),
          )
          .map(|_| ())
      }),
    ),
    ("click_with_alt_modifier", "invalid_input", Box::new(|| session.window().click(&window, button, alt_click.clone()).map(|_| ()))),
  ];
  for (name, expected_kind, run) in &negatives {
    for rep in 0..reps.min(5) {
      let (measured, ()) = desk.measure(run, || ());
      tally.record(&measured);
      let (outcome, kind) = match &measured.value {
        Ok(()) => (json!({"status": "delivered"}), "delivered"),
        Err(error) => (failure(error), error_kind(error)),
      };
      tally.expect(kind == *expected_kind, format!("{name} {rep}: expected a {expected_kind} error, got {kind}"));
      records.write(
        "op",
        json!({
          "op": name, "rep": rep, "target": target, "expected_error": expected_kind, "outcome": outcome,
          "latency_ms": measured.latency_ms, "invariants": invariants(&measured, hwnd),
        }),
      )?;
    }
  }
  let after_negatives = receiver.state()?;
  tally.expect(
    after_negatives.clicks == before_negatives.clicks && after_negatives.first_line == before_negatives.first_line,
    "a rejected operation still reached the receiver",
  );

  let end: State = receiver.state()?;
  let finish = watch.snapshot();
  let end_view = view_of(hwnd);
  tally.expect(!end.foreground, "the receiver reports that it is the foreground window");
  tally.expect(
    end.foreground_taken == warm_state.foreground_taken,
    format!("the receiver took the foreground {} time(s) after the warm-up", end.foreground_taken - warm_state.foreground_taken),
  );
  tally.expect(finish.foreground != hwnd, "the receiver is the foreground window at the end");
  tally.expect(end_view.z_rank >= start_view.z_rank, "the receiver rose in the Z-order");
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the run");
  tally.expect(tally.violated == 0, format!("{} operation(s) disturbed the desktop", tally.violated));
  tally.expect(desk.gap_violations() == 0, format!("{} change(s) landed between operations", desk.gap_violations()));
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({
      "passed": passed,
      "ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "gap_findings": desk.gaps,
      "failures": tally.failures,
      "receiver_final_state": end,
      "z_rank": [start_view.z_rank, end_view.z_rank],
      "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "foreground_events_total": finish.foreground_changes,
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys,
      "physical_mouse_total": finish.physical_mouse, "physical_keys_total": finish.physical_keys,
    }),
  )?;

  println!(
    "single ({} receiver): {} ops, {} held, {} inconclusive (user input), {} violated, {} gap finding(s); {}",
    if receiver.is_detached() {
      "detached"
    } else {
      "piped"
    },
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    desk.gaps.len(),
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("single.jsonl").display());
  Ok(passed)
}
