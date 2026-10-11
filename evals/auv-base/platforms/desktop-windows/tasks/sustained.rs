//! Sustained multi-window scenarios: up to five background windows, operated and watched through
//! one [`BackgroundSession`] while the user keeps working elsewhere.
//!
//! - [`five`]: the validation session. Five detached receivers, some covered and some not, are
//!   discovered once and attached. Every operation is checked against what the desktop and the
//!   receiving application showed; every observation is decoded and compared with the receiver's
//!   own state, so a stale frame cannot pass.
//! - [`bench`]: the same act-then-observe cycle, continuously, for ten minutes, with per-minute
//!   throughput, latency and resource gauges, and a check that the last minute is not slower than
//!   the first.
//! - [`faults`]: the same cycle at a gentle pace while a window is closed, one is killed, one is
//!   moved and resized, and one is minimized and restored. The session must report each one
//!   explicitly and keep going.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use auv_driver_common::Driver;
use auv_driver_common::capture::Capture;
use auv_driver_common::geometry::WindowPoint;
use auv_driver_common::input::{InputDeliveryPath, Scroll};
use auv_driver_common::window::Window;
use auv_driver_windows::window::window_enumerations;
use auv_driver_windows::{BackgroundSession, ClickRequest, FrameRead, Outcome, TargetId, TargetState, WindowsDriver, WindowsDriverSession};
use serde_json::{Value, json};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources, GetProcessHandleCount};

use super::background::{
  Desk, Tally, effect, events, injected, invariants, is_undisturbing, occlusion, point_in_window, run_id, spawn_receiver_with, system_facts,
  user_input, view_of,
};
use super::receiver_process::{Receiver, Strip};
use super::records::{Records, percentile};
use super::watch::{DesktopWatch, Snapshot, Verdict};

pub struct FiveOptions {
  pub output: PathBuf,
  /// Act-then-observe rounds over all five windows.
  pub rounds: u32,
  /// How many of the five receivers stay at the top of the Z-order; the rest are covered.
  pub visible: usize,
}

pub struct BenchOptions {
  pub output: PathBuf,
  pub minutes: f64,
  pub visible: usize,
  pub ops: BenchOps,
}

/// Which operations a `bench` round sends to each window. `Click` and `Scroll` exist to tell
/// which of the two a desktop finding follows (see "Injected key presses during `bench`" in
/// `docs/ai/references/driver/2026-10-10-windows-background-session.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchOps {
  Both,
  Click,
  Scroll,
}

#[derive(Clone, Copy)]
enum Op {
  Click,
  Scroll,
}

impl BenchOps {
  fn per_window(self) -> &'static [Op] {
    match self {
      BenchOps::Both => &[Op::Click, Op::Scroll],
      BenchOps::Click => &[Op::Click],
      BenchOps::Scroll => &[Op::Scroll],
    }
  }
}

pub struct FaultOptions {
  pub output: PathBuf,
  /// Total length. Faults fall at fixed fractions of it: 50% close, 60% kill, 70% move and
  /// resize, 80% minimize, 90% restore. Ten minutes puts the close at minute five and the move at
  /// minute seven.
  pub minutes: f64,
  /// Pause between rounds, so a long run stays gentle on the user's machine.
  pub round_pause_ms: u64,
}

const WINDOWS: usize = 5;
/// How long an observation may take to show an action's effect before it counts as a gap.
const REFLECT_TIMEOUT: Duration = Duration::from_millis(1000);
/// Pause between looks while waiting for the compositor to deliver a frame. The wait is about a
/// frame long, so spinning on `capture` would burn a core on the user's machine for nothing (the
/// first ten-minute run did: 249 million probes) while sleeping adds at most one timer tick to the
/// time to reflect. `Duration` is a request: the timer rounds it up to its own resolution.
const POLL_INTERVAL: Duration = Duration::from_micros(250);

/// Five receivers, discovered once and attached to a session.
struct Fleet {
  session: BackgroundSession,
  receivers: Vec<Receiver>,
  ids: Vec<TargetId>,
  windows: Vec<Window>,
  /// The button and the edit as points in each window's own frame, which stay valid when the
  /// window moves.
  button: Vec<WindowPoint>,
  edit: Vec<WindowPoint>,
}

fn start_fleet(driver: &WindowsDriverSession, output: &std::path::Path, visible: usize) -> Result<(Fleet, f64, u64)> {
  let mut receivers = Vec::new();
  for index in 0..WINDOWS {
    receivers.push(spawn_receiver_with(output, index, index >= visible)?);
  }
  let wanted: Vec<String> = receivers.iter().map(|receiver| receiver.layout.hwnd.clone()).collect();
  let enumerations_before = window_enumerations();
  let started = Instant::now();
  // One enumeration finds all five. After this the session never enumerates again.
  let windows = loop {
    let listed = driver.window().list().map_err(|error| anyhow::anyhow!("window discovery failed: {error}"))?;
    let found: Vec<Window> = wanted.iter().filter_map(|id| listed.iter().find(|window| &window.reference.id == id).cloned()).collect();
    if found.len() == wanted.len() {
      break found;
    }
    if started.elapsed() > Duration::from_secs(5) {
      bail!("the driver listed only {} of the {} receiver windows", found.len(), wanted.len());
    }
    std::thread::sleep(Duration::from_millis(50));
  };
  let discovery_ms = started.elapsed().as_secs_f64() * 1000.0;
  let enumerations = window_enumerations() - enumerations_before;
  let mut session = driver.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
  let mut ids = Vec::new();
  for window in &windows {
    ids.push(session.attach(window).map_err(|error| anyhow::anyhow!("attach {}: {error}", window.reference.id))?);
  }
  let button = windows.iter().zip(&receivers).map(|(window, receiver)| point_in_window(window, receiver.layout.button_center)).collect();
  let edit = windows.iter().zip(&receivers).map(|(window, receiver)| point_in_window(window, receiver.layout.edit_center)).collect();
  Ok((
    Fleet {
      session,
      receivers,
      ids,
      windows,
      button,
      edit,
    },
    discovery_ms,
    enumerations,
  ))
}

/// One click per receiver through the session, before anything is measured, and what the desktop
/// did meanwhile.
///
/// Windows lets a freshly started process take the foreground once, until the user's next input.
/// A receiver started while the user is away can therefore take it on its first activation, which
/// a click on its button causes; an application the user started earlier and has since worked
/// around holds no such right. The warm-up spends that right on the record (each receiver hands
/// the foreground straight back), so the measured operations target applications in the state a
/// background session meets them in. A takeover here is reported, not hidden.
/// Also returns each receiver's takeover count afterwards: the baseline that later takeovers are
/// measured from.
fn warm_up(fleet: &mut Fleet, watch: &DesktopWatch, launched: &Snapshot) -> Result<(Value, Vec<u64>)> {
  let before = watch.snapshot();
  let mut delivered = 0;
  for index in 0..fleet.ids.len() {
    if fleet.session.click(fleet.ids[index], fleet.button[index], ClickRequest::default()).is_done() {
      delivered += 1;
    }
    fleet.receivers[index].state()?;
  }
  // The hand-back is posted behind the activation; give it time to run before looking.
  std::thread::sleep(Duration::from_millis(150));
  let mut states = Vec::new();
  for receiver in fleet.receivers.iter_mut() {
    states.push(receiver.state()?);
  }
  let after = watch.snapshot();
  let (verdict, timeline) = watch.judge(&before, &after);
  let taken = states.iter().map(|state| state.foreground_taken).collect();
  let record = json!({
    "clicks_delivered": delivered,
    "user_input_since_launch": after.last_physical_ms > launched.taken_ms,
    "foreground_taken": states.iter().map(|state| state.foreground_taken).collect::<Vec<_>>(),
    "foreground_yielded": states.iter().map(|state| state.foreground_yielded).collect::<Vec<_>>(),
    "foreground_before": before.foreground.to_string(), "foreground_after": after.foreground.to_string(),
    "foreground_events": events(&timeline, before.taken_ms, &fleet.receivers.iter().map(Receiver::hwnd).collect::<Vec<_>>()),
    "user_input_ms": user_input(&timeline, before.taken_ms),
    "verdict": verdict,
  });
  Ok((record, taken))
}

/// The receiver's click count and scroll line (low eight bits each), read back from a capture of it.
pub(super) fn decode_strip(capture: &Capture, strip: &Strip) -> (u8, u8) {
  let scale = capture.scale_factor;
  let mut value = 0u16;
  for cell in 0..strip.cells {
    let x = (f64::from(strip.origin[0]) - capture.bounds.origin.x + (f64::from(cell) + 0.5) * f64::from(strip.cell[0])) * scale;
    let y = (f64::from(strip.origin[1]) - capture.bounds.origin.y + f64::from(strip.cell[1]) / 2.0) * scale;
    let lit = capture.image.get_pixel_checked(x.max(0.0) as u32, y.max(0.0) as u32).is_some_and(|pixel| pixel[0] > 128);
    value = (value << 1) | u16::from(lit);
  }
  ((value >> 8) as u8, value as u8)
}

struct Observed {
  reflected: bool,
  polls: u32,
  fresh: u32,
  ms: f64,
  /// Time spent inside the captures that returned pixels: the cost of copying a frame to the CPU,
  /// not of waiting for the compositor to deliver it.
  fresh_read_ms: f64,
  /// What the session answered if it was not a frame (lost, minimized, ...).
  other: Option<Value>,
  /// The newest frame this observation decoded, with what it was decoded against. A failed
  /// observation is only explicable with it: no frame arrived, or one arrived and showed
  /// something else.
  last_frame: Option<Value>,
  /// The same frame's pixels, kept (moved, not copied) so a failure can be looked at.
  last_capture: Option<Capture>,
}

/// Captures until a frame shows `want` (click count and scroll line, low eight bits), or the
/// timeout passes. A frame that does not show it yet is not an error: the compositor has not
/// delivered the change yet. What matters is that the first frame that does arrive shows it.
fn observe(session: &mut BackgroundSession, id: TargetId, strip: &Strip, want: (u8, u8), timeout: Duration) -> Observed {
  let started = Instant::now();
  let (mut polls, mut fresh, mut fresh_read_ms) = (0, 0, 0.0);
  let mut last_frame = None;
  let mut last_capture = None;
  loop {
    polls += 1;
    let polled = Instant::now();
    let answer = session.capture(id);
    if matches!(
      answer,
      Outcome::Done {
        value: FrameRead::Fresh { .. }
      }
    ) {
      fresh_read_ms += polled.elapsed().as_secs_f64() * 1000.0;
    }
    match answer {
      Outcome::Done {
        value: FrameRead::Fresh {
          capture,
          frame_number,
          ..
        },
      } => {
        fresh += 1;
        let decoded = decode_strip(&capture, strip);
        last_frame = Some(json!({
          "frame_number": frame_number, "image": [capture.image.width(), capture.image.height()],
          "bounds": [capture.bounds.origin.x, capture.bounds.origin.y, capture.bounds.size.width, capture.bounds.size.height],
          "scale_factor": capture.scale_factor, "decoded": [decoded.0, decoded.1], "wanted": [want.0, want.1], "strip_origin": strip.origin,
        }));
        if decoded == want {
          return Observed {
            reflected: true,
            polls,
            fresh,
            ms: started.elapsed().as_secs_f64() * 1000.0,
            fresh_read_ms,
            other: None,
            last_frame,
            last_capture: None,
          };
        }
        last_capture = Some(capture);
      }
      Outcome::Done {
        value: FrameRead::Unchanged { .. },
      } => {}
      other => {
        return Observed {
          reflected: false,
          polls,
          fresh,
          ms: started.elapsed().as_secs_f64() * 1000.0,
          fresh_read_ms,
          other: serde_json::to_value(other.map(|_| ())).ok(),
          last_frame,
          last_capture,
        };
      }
    }
    if started.elapsed() >= timeout {
      return Observed {
        reflected: false,
        polls,
        fresh,
        ms: started.elapsed().as_secs_f64() * 1000.0,
        fresh_read_ms,
        other: None,
        last_frame,
        last_capture,
      };
    }
    std::thread::sleep(POLL_INTERVAL);
  }
}

fn typed<T: serde::Serialize>(outcome: &Outcome<T>) -> Value {
  serde_json::to_value(outcome).unwrap_or(Value::Null)
}

/// Gauges that would grow if the session leaked or slowed: memory, handles and GUI objects.
fn gauges() -> Value {
  // SAFETY: read-only queries on this process with writable out parameters.
  unsafe {
    let process = GetCurrentProcess();
    let mut memory = PROCESS_MEMORY_COUNTERS {
      cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
      ..Default::default()
    };
    let _ = GetProcessMemoryInfo(process, &mut memory, memory.cb);
    let mut handles = 0u32;
    let _ = GetProcessHandleCount(process, &mut handles);
    json!({
      "working_set_mb": memory.WorkingSetSize as f64 / 1_048_576.0,
      "pagefile_mb": memory.PagefileUsage as f64 / 1_048_576.0,
      "handles": handles,
      "gdi_objects": GetGuiResources(process, GR_GDIOBJECTS),
      "user_objects": GetGuiResources(process, GR_USEROBJECTS),
    })
  }
}

#[derive(Default)]
struct Series(Vec<f64>);

impl Series {
  fn push(&mut self, ms: f64) {
    self.0.push(ms);
  }

  fn summary(&self) -> Value {
    json!({
      "n": self.0.len(),
      "p50_ms": percentile(&self.0, 50.0),
      "p95_ms": percentile(&self.0, 95.0),
      "p99_ms": percentile(&self.0, 99.0),
      "max_ms": self.0.iter().cloned().fold(f64::NAN, f64::max),
    })
  }
}

// ---------------------------------------------------------------------------------------------
// five
// ---------------------------------------------------------------------------------------------

/// The validation session: five windows, foreground untouched, a record per operation.
pub fn five(options: &FiveOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let mut records = Records::create(&options.output.join("five.jsonl"), &run_id(), "five")?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let mut tally = Tally::default();
  let launched = watch.snapshot();
  let (mut fleet, discovery_ms, discovery_enumerations) = start_fleet(&driver, &options.output, options.visible)?;
  let (warm, _) = warm_up(&mut fleet, &watch, &launched)?;
  let start = watch.snapshot();
  let hwnds: Vec<isize> = fleet.receivers.iter().map(Receiver::hwnd).collect();
  records.write(
    "setup",
    json!({
      "warm_up": warm,
      "receivers": fleet.receivers.iter().zip(&fleet.windows).enumerate().map(|(index, (receiver, window))| json!({
        "index": index, "hwnd": receiver.layout.hwnd, "pid": receiver.layout.pid, "color": receiver.spec.color,
        "covered": index >= options.visible, "frame": window.frame, "occluded_fraction": occlusion(&driver, window),
      })).collect::<Vec<_>>(),
      "discovery": {"ms": discovery_ms, "enumerations": discovery_enumerations},
      "foreground": start.foreground.to_string(),
      "foreground_is_a_receiver": hwnds.contains(&start.foreground),
      "system": system_facts(),
      "rounds": options.rounds,
    }),
  )?;
  tally.expect(!hwnds.contains(&start.foreground), "a receiver started as the foreground window");
  tally.expect(discovery_enumerations == 1, format!("discovery took {discovery_enumerations} window enumerations, expected 1"));

  let mut desk = Desk::new(&watch, hwnds[0]);
  let Fleet {
    session,
    receivers,
    ids,
    windows: _,
    button,
    edit,
  } = &mut fleet;

  // Warm up: the first read of each target opens its capture stream.
  for index in 0..WINDOWS {
    let started = Instant::now();
    let (measured, ()) = desk.measure_at(hwnds[index], || session.capture(ids[index]), || ());
    tally.record(&measured);
    let first_frame = matches!(
      measured.value,
      Outcome::Done {
        value: FrameRead::Fresh { .. }
      }
    );
    tally.expect(first_frame, format!("the first capture of window {index} did not return a frame"));
    records.write(
      "op",
      json!({
        "op": "capture_first", "target": index, "hwnd": receivers[index].layout.hwnd, "outcome": typed(&measured.value.without_value()),
        "latency_ms": started.elapsed().as_secs_f64() * 1000.0, "effect": effect(first_frame, json!({})), "invariants": invariants(&measured, hwnds[index]),
      }),
    )?;
  }
  let enumerations_after_warmup = window_enumerations();

  // Rounds: for each window, click and observe it, then scroll and observe it.
  let (mut observe_click, mut observe_scroll) = (Series::default(), Series::default());
  for round in 0..options.rounds {
    let down = round < options.rounds.div_ceil(2);
    for index in 0..WINDOWS {
      let (id, hwnd) = (ids[index], hwnds[index]);
      let strip = receivers[index].layout.strip.clone();
      let before = receivers[index].state()?;
      let (measured, after) =
        desk.measure_at(hwnd, || session.click(id, button[index], ClickRequest::default()), || receivers[index].state());
      let after = after?;
      tally.record(&measured);
      let delivered =
        matches!(&measured.value, Outcome::Done { value } if is_undisturbing(&value.result, InputDeliveryPath::WindowTargetedMouse));
      let clicked = after.clicks == before.clicks + 1;
      tally.expect(delivered, format!("round {round} window {index}: the click was not an undisturbing window-targeted delivery"));
      tally.expect(clicked, format!("round {round} window {index}: the button did not report BN_CLICKED"));
      let took = after.foreground_taken - before.foreground_taken;
      tally.expect(
        !after.foreground && !hwnds.contains(&measured.after.foreground) && took == 0,
        format!("round {round} window {index}: the receiver took the foreground"),
      );
      records.write(
        "op",
        json!({
          "op": "click", "round": round, "target": index, "hwnd": receivers[index].layout.hwnd, "outcome": typed(&measured.value),
          "latency_ms": measured.latency_ms, "effect": effect(clicked, json!({"clicks_before": before.clicks, "clicks_after": after.clicks,
            "receiver_self_reports_foreground": after.foreground, "receiver_took_foreground": took})), "invariants": invariants(&measured, hwnd),
        }),
      )?;

      let want = ((after.clicks & 0xFF) as u8, (after.first_line & 0xFF) as u8);
      let (observing, ()) = desk.measure_at(hwnd, || observe(session, id, &strip, want, REFLECT_TIMEOUT), || ());
      let observed = &observing.value;
      tally.record(&observing);
      tally.expect(observed.reflected, format!("round {round} window {index}: no frame showed the click within {REFLECT_TIMEOUT:?}"));
      if observed.reflected {
        observe_click.push(observed.ms);
      }
      records.write(
        "op",
        json!({
          "op": "observe_click", "round": round, "target": index, "hwnd": receivers[index].layout.hwnd, "latency_ms": observed.ms,
          "effect": effect(observed.reflected, json!({"polls": observed.polls, "fresh_frames_seen": observed.fresh, "want": [want.0, want.1], "other": observed.other.clone()})),
          "invariants": invariants(&observing, hwnd),
        }),
      )?;

      let delta = if down { 100.0 } else { -100.0 };
      let before = receivers[index].state()?;
      let (measured, after) =
        desk.measure_at(hwnd, || session.scroll(id, edit[index], Scroll::new(0.0, delta)), || receivers[index].state());
      let after = after?;
      tally.record(&measured);
      let delivered =
        matches!(&measured.value, Outcome::Done { value } if is_undisturbing(&value.result, InputDeliveryPath::WindowTargetedWheel));
      let moved = (after.first_line - before.first_line).signum() == if down { 1 } else { -1 };
      tally.expect(delivered, format!("round {round} window {index}: the scroll was not an undisturbing window-targeted delivery"));
      tally.expect(moved, format!("round {round} window {index}: the edit did not scroll the right way"));
      let took = after.foreground_taken - before.foreground_taken;
      tally.expect(took == 0, format!("round {round} window {index}: the receiver took the foreground on a scroll"));
      records.write(
        "op",
        json!({
          "op": "scroll", "round": round, "target": index, "hwnd": receivers[index].layout.hwnd, "outcome": typed(&measured.value),
          "latency_ms": measured.latency_ms, "effect": effect(moved, json!({"first_line_before": before.first_line, "first_line_after": after.first_line,
            "receiver_took_foreground": took})),
          "invariants": invariants(&measured, hwnd),
        }),
      )?;

      let want = ((after.clicks & 0xFF) as u8, (after.first_line & 0xFF) as u8);
      let (observing, ()) = desk.measure_at(hwnd, || observe(session, id, &strip, want, REFLECT_TIMEOUT), || ());
      let observed = &observing.value;
      tally.record(&observing);
      tally.expect(observed.reflected, format!("round {round} window {index}: no frame showed the scroll within {REFLECT_TIMEOUT:?}"));
      if observed.reflected {
        observe_scroll.push(observed.ms);
      }
      records.write(
        "op",
        json!({
          "op": "observe_scroll", "round": round, "target": index, "hwnd": receivers[index].layout.hwnd, "latency_ms": observed.ms,
          "effect": effect(observed.reflected, json!({"polls": observed.polls, "fresh_frames_seen": observed.fresh, "want": [want.0, want.1], "other": observed.other.clone()})),
          "invariants": invariants(&observing, hwnd),
        }),
      )?;

      // R6: keyboard input has no background route. The answer is typed, and nothing is typed
      // into whatever has the foreground.
      if round as usize % WINDOWS == index {
        let (measured, ()) = desk.measure_at(hwnd, || session.type_text(id, "AUV background text"), || ());
        tally.record(&measured);
        let unsupported = matches!(
          measured.value,
          Outcome::Unsupported {
            operation: "type_text",
            ..
          }
        );
        tally.expect(unsupported, "type_text was not answered as unsupported");
        records.write(
          "op",
          json!({"op": "type_text", "round": round, "target": index, "outcome": typed(&measured.value), "latency_ms": measured.latency_ms,
            "effect": effect(false, json!({"expected": "unsupported"})), "invariants": invariants(&measured, hwnd)}),
        )?;
      }
    }
  }

  // R4: with nothing changing, captures copy nothing and the compositor delivers nothing.
  std::thread::sleep(Duration::from_millis(300));
  let before_static = session.stats();
  let static_started = Instant::now();
  let mut static_reads = 0u64;
  while static_started.elapsed() < Duration::from_secs(2) {
    for (index, &id) in ids.iter().enumerate() {
      let outcome = session.capture(id);
      static_reads += 1;
      tally.expect(
        matches!(
          outcome,
          Outcome::Done {
            value: FrameRead::Unchanged { .. }
          }
        ),
        format!("a capture of the still window {index} copied pixels"),
      );
    }
  }
  let after_static = session.stats();
  records.write(
    "check",
    json!({
      "check": "static_windows_are_not_recaptured", "reads": static_reads, "seconds": 2,
      "fresh_frames": after_static.frames_fresh - before_static.frames_fresh,
      "unchanged_reads": after_static.frames_unchanged - before_static.frames_unchanged,
      "frames_delivered_by_the_compositor": after_static.frames_delivered - before_static.frames_delivered,
      "observed": after_static.frames_fresh == before_static.frames_fresh && after_static.frames_delivered == before_static.frames_delivered,
    }),
  )?;
  tally.expect(after_static.frames_fresh == before_static.frames_fresh, "pixels were copied while every window stood still");
  tally.expect(
    after_static.frames_delivered == before_static.frames_delivered,
    "the compositor delivered frames while every window stood still",
  );

  // The session's own part is done: it enumerated no windows after the one discovery. The burst
  // comparison below resolves a window through the driver, which does enumerate.
  let enumerations_after_rounds = window_enumerations();
  let stats = session.stats();
  tally.expect(
    enumerations_after_rounds == enumerations_after_warmup,
    format!("windows were enumerated {} time(s) while the rounds ran", enumerations_after_rounds - enumerations_after_warmup),
  );
  tally.expect(
    stats.streams_opened == WINDOWS as u64 && stats.streams_reopened == 0,
    format!("expected one stream per window, opened {} reopened {}", stats.streams_opened, stats.streams_reopened),
  );
  records.write(
    "check",
    json!({"check": "no_rediscovery", "enumerations_for_discovery": discovery_enumerations, "enumerations_after_warmup": enumerations_after_warmup,
      "enumerations_after_all_rounds": enumerations_after_rounds, "session_probes": stats.probes, "streams_opened": stats.streams_opened,
      "note": "the harness's own occlusion estimate enumerates once per window before warmup; the session itself never enumerates"}),
  )?;

  // R3: a burst of actions, then one look. The on-demand capture reads a two-buffer pool in order,
  // so after several changes it hands out an older state and, once the pool is full, never gets
  // the newest. The stream keeps only the newest.
  let burst_window = fleet_window(&driver, receivers[0].hwnd())?;
  let mut burst = |method: &str, index: usize| -> Result<Value> {
    let (mut first_showed_truth, mut never_showed_truth) = (0, 0);
    let mut sequences = Vec::new();
    for _ in 0..10 {
      for _ in 0..3 {
        let click = session.click(ids[index], button[index], ClickRequest::default());
        tally.expect(click.is_done(), "a burst click was not delivered");
        receivers[index].state()?;
        // One compositor frame apart, so each click produces a frame of its own.
        std::thread::sleep(Duration::from_millis(16));
      }
      std::thread::sleep(Duration::from_millis(100));
      let truth = receivers[index].state()?;
      let want = ((truth.clicks & 0xFF) as u8, (truth.first_line & 0xFF) as u8);
      let strip = receivers[index].layout.strip.clone();
      let mut shown = Vec::new();
      for _ in 0..5 {
        if method == "legacy_capture_window_wgc" {
          let capture = driver.window().capture_wgc(&burst_window).map_err(|error| anyhow::anyhow!("legacy capture: {error}"))?;
          shown.push(i32::from(decode_strip(&capture, &strip).0));
        } else {
          match session.capture(ids[index]) {
            Outcome::Done {
              value: FrameRead::Fresh { capture, .. },
            } => shown.push(i32::from(decode_strip(&capture, &strip).0)),
            _ => shown.push(-1),
          }
        }
        std::thread::sleep(Duration::from_millis(30));
      }
      let truth_low = i32::from(want.0);
      if shown.first() == Some(&truth_low) {
        first_showed_truth += 1;
      }
      if !shown.contains(&truth_low) && !(method != "legacy_capture_window_wgc" && shown.contains(&-1) && first_showed_truth > 0) {
        never_showed_truth += 1;
      }
      sequences.push(json!({"truth": truth_low, "captures_showed": shown}));
    }
    Ok(json!({"method": method, "trials": 10, "first_capture_showed_the_latest_state": first_showed_truth,
      "latest_state_never_shown_in_five_captures": never_showed_truth, "examples": sequences.into_iter().take(3).collect::<Vec<_>>()}))
  };
  let legacy = burst("legacy_capture_window_wgc", 0)?;
  let streamed = burst("session_frame_stream", 1)?;
  records.write("check", json!({"check": "burst_then_observe", "legacy": legacy, "stream": streamed}))?;
  tally.expect(
    streamed["first_capture_showed_the_latest_state"] == 10,
    "the session's first capture after a burst did not show the latest state",
  );

  // R2: what reading a fresh frame of a large window costs. Window 4 is resized to 1920x1080
  // (the session follows it), and each trial clicks, waits for the compositor to deliver the new
  // frame, and times the read that copies it.
  {
    let index = 4;
    receivers[index].request(json!({"cmd": "resize", "width": 1920, "height": 1080}))?;
    receivers[index].refresh_layout()?;
    let mut reads = Series::default();
    let mut sizes = Vec::new();
    for _ in 0..30 {
      let before = session.stats().frames_delivered;
      let click = session.click(ids[index], button[index], ClickRequest::default());
      tally.expect(click.is_done(), "a click on the enlarged window was not delivered");
      receivers[index].state()?;
      let waited = Instant::now();
      while session.stats().frames_delivered == before && waited.elapsed() < REFLECT_TIMEOUT {
        std::thread::sleep(POLL_INTERVAL);
      }
      let started = Instant::now();
      match session.capture(ids[index]) {
        Outcome::Done {
          value: FrameRead::Fresh { capture, .. },
        } => {
          reads.push(started.elapsed().as_secs_f64() * 1000.0);
          sizes.push((capture.image.width(), capture.image.height()));
        }
        other => tally.expect(false, format!("the enlarged window did not return a frame: {:?}", typed(&other.without_value()))),
      }
    }
    let mut distinct: Vec<(u32, u32)> = sizes.clone();
    distinct.sort_unstable();
    distinct.dedup();
    records.write(
      "check",
      json!({"check": "large_frame_read", "frame_sizes_seen": distinct, "last_frame_size": sizes.last(), "megapixels": sizes.last().map(|(w, h)| f64::from(w * h) / 1e6), "reads": reads.summary(),
        "reference": "the frozen Warm Fast baseline is P50 8.20 ms; that figure is a different operation (QQ Music replay), shown for scale only"}),
    )?;
  }

  let finish = watch.snapshot();
  let mut final_states = Vec::new();
  for receiver in receivers.iter_mut() {
    final_states.push(receiver.state()?);
  }
  tally.expect(final_states.iter().all(|state| !state.foreground), "a receiver reports it is the foreground window at the end");
  tally.expect(!hwnds.contains(&finish.foreground), "a receiver is the foreground window at the end");
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the run");
  tally.expect(tally.violated == 0, format!("{} operation(s) disturbed the desktop", tally.violated));
  tally.expect(desk.gap_violations() == 0, format!("{} change(s) landed between operations", desk.gap_violations()));
  let delivered_clicks = final_states.iter().map(|state| state.clicks).sum::<u64>();
  let delivered_by_session = session.stats().clicks;
  tally.expect(
    delivered_clicks == delivered_by_session,
    format!("the session delivered {delivered_by_session} clicks but the receivers saw {delivered_clicks}"),
  );
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({
      "passed": passed, "ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "gap_findings": desk.gaps, "failures": tally.failures,
      "observe_click_ms": observe_click.summary(), "observe_scroll_ms": observe_scroll.summary(),
      "session": session.stats(), "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "foreground_events_total": finish.foreground_changes,
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys,
      "physical_mouse_total": finish.physical_mouse, "physical_keys_total": finish.physical_keys,
      "receivers_final": final_states, "gauges": gauges(),
    }),
  )?;
  println!(
    "five: {} ops, {} held, {} inconclusive (user input), {} violated, {} gap finding(s); {}",
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    desk.gaps.len(),
    if passed { "PASS" } else { "FAIL" }
  );
  println!(
    "  time to reflect (ms): click p50 {:.1} p95 {:.1}; scroll p50 {:.1} p95 {:.1}",
    percentile(&observe_click.0, 50.0),
    percentile(&observe_click.0, 95.0),
    percentile(&observe_scroll.0, 50.0),
    percentile(&observe_scroll.0, 95.0)
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("five.jsonl").display());
  Ok(passed)
}

/// The window as the driver lists it now. Only the legacy comparison does this: the session itself
/// never enumerates.
fn fleet_window(driver: &WindowsDriverSession, hwnd: isize) -> Result<Window> {
  driver
    .window()
    .list()
    .map_err(|error| anyhow::anyhow!("window discovery failed: {error}"))?
    .into_iter()
    .find(|window| window.reference.id == hwnd.to_string())
    .context("the receiver window is not listed")
}

// ---------------------------------------------------------------------------------------------
// bench
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Minute {
  rounds: u64,
  clicks: Series,
  scrolls: Series,
  observes: Series,
  /// Time inside the captures that returned pixels, per observation.
  fresh_reads: Series,
  delivered_clicks: u64,
  reflected: u64,
  unreflected: u64,
  /// Foreground takeovers the receivers counted (each one handed straight back).
  takeovers: u64,
  held: u64,
  inconclusive: u64,
  violated: u64,
}

impl Minute {
  fn ops(&self) -> usize {
    self.clicks.0.len() + self.scrolls.0.len() + self.observes.0.len()
  }
}

/// The sustained benchmark: act then observe, continuously, across five windows.
pub fn bench(options: &BenchOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let mut records = Records::create(&options.output.join("bench.jsonl"), &run_id(), "bench")?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let mut tally = Tally::default();
  let launched = watch.snapshot();
  let (mut fleet, discovery_ms, discovery_enumerations) = start_fleet(&driver, &options.output, options.visible)?;
  let (warm, mut taken) = warm_up(&mut fleet, &watch, &launched)?;
  let start = watch.snapshot();
  let hwnds: Vec<isize> = fleet.receivers.iter().map(Receiver::hwnd).collect();
  records.write(
    "setup",
    json!({
      "warm_up": warm,
      "minutes": options.minutes, "visible_receivers": options.visible, "ops": options.ops,
      "discovery": {"ms": discovery_ms, "enumerations": discovery_enumerations},
      "foreground": start.foreground.to_string(), "system": system_facts(), "gauges_at_start": gauges(),
      "cycle": match options.ops {
        BenchOps::Both => "per window: click, observe until a frame shows it, scroll, observe until a frame shows it; one ops count each (click, scroll, two observations)",
        BenchOps::Click => "per window: click, observe until a frame shows it; one ops count each",
        BenchOps::Scroll => "per window: scroll, observe until a frame shows it; one ops count each",
      },
      "observe_poll_interval_us": POLL_INTERVAL.as_micros() as u64,
    }),
  )?;
  tally.expect(!hwnds.contains(&start.foreground), "a receiver started as the foreground window");

  let Fleet {
    session,
    receivers,
    ids,
    windows: _,
    button,
    edit,
  } = &mut fleet;
  // Warm up before the clock starts: open each stream and take each window once.
  for &id in ids.iter() {
    let _ = session.capture(id);
  }
  let enumerations_after_warmup = window_enumerations();

  let total = Duration::from_secs_f64(options.minutes * 60.0);
  let run_started = Instant::now();
  let mut minute_number = 1u32;
  let mut minute_started = Instant::now();
  let mut minute = Minute::default();
  let mut minutes: Vec<Value> = Vec::new();
  let mut per_minute_summaries: Vec<(f64, f64, f64, f64)> = Vec::new();
  let mut gauges_by_minute: Vec<Value> = Vec::new();
  let mut round = 0u64;
  let mut last: Snapshot = watch.snapshot();
  let mut interrupted = Value::Null;

  'run: while run_started.elapsed() < total {
    let down = (round / 40).is_multiple_of(2);
    let before = last;
    for index in 0..WINDOWS {
      let (id, strip) = (ids[index], receivers[index].layout.strip.clone());
      for &op in options.ops.per_window() {
        let started = Instant::now();
        // Each operation answers whether it was delivered without disturbing the desktop, and
        // whether the window was minimized or closed.
        let (name, delivered, gone, answer) = match op {
          Op::Click => {
            let click = session.click(id, button[index], ClickRequest::default());
            minute.clicks.push(started.elapsed().as_secs_f64() * 1000.0);
            let delivered =
              matches!(&click, Outcome::Done { value } if is_undisturbing(&value.result, InputDeliveryPath::WindowTargetedMouse));
            if delivered {
              minute.delivered_clicks += 1;
            }
            ("click", delivered, matches!(click, Outcome::SkippedMinimized | Outcome::TargetLost { .. }), typed(&click))
          }
          Op::Scroll => {
            let delta = if down { 100.0 } else { -100.0 };
            let scroll = session.scroll(id, edit[index], Scroll::new(0.0, delta));
            minute.scrolls.push(started.elapsed().as_secs_f64() * 1000.0);
            let delivered =
              matches!(&scroll, Outcome::Done { value } if is_undisturbing(&value.result, InputDeliveryPath::WindowTargetedWheel));
            ("scroll", delivered, matches!(scroll, Outcome::SkippedMinimized | Outcome::TargetLost { .. }), typed(&scroll))
          }
        };
        if gone {
          // Nothing in the bench minimizes or closes a receiver, so someone else did (observed: the
          // user's show-desktop minimized all five in turn). The run stops measuring what it says it
          // measures; it ends here with what was acting on the desktop in the seconds before.
          let now = watch.snapshot();
          let since = now.taken_ms.saturating_sub(3000);
          let timeline = watch.timeline(since, now.taken_ms);
          interrupted = json!({
            "minute": minute_number, "round": round, "window": index, "answer": answer,
            "looked_back_ms": now.taken_ms - since, "user_input_ms": user_input(&timeline, since),
            "injected_input": injected(&timeline, since), "foreground_events": events(&timeline, since, &hwnds),
          });
          records.write("interrupted", interrupted.clone())?;
          tally.expect(
            false,
            format!("minute {minute_number} round {round}: window {index} answered {answer:?}, which nothing in the bench causes; the run was interrupted from outside"),
          );
          break 'run;
        }
        if !delivered {
          tally.expect(false, format!("minute {minute_number} round {round} window {index}: {name} answered {answer:?}"));
        }
        let state = receivers[index].state()?;
        if state.foreground_taken != taken[index] {
          minute.takeovers += state.foreground_taken - taken[index];
          taken[index] = state.foreground_taken;
          tally.expect(false, format!("minute {minute_number} round {round} window {index}: the receiver took the foreground"));
        }
        let started = Instant::now();
        let observed = observe(session, id, &strip, ((state.clicks & 0xFF) as u8, (state.first_line & 0xFF) as u8), REFLECT_TIMEOUT);
        minute.observes.push(started.elapsed().as_secs_f64() * 1000.0);
        minute.fresh_reads.push(observed.fresh_read_ms);
        if observed.reflected {
          minute.reflected += 1
        } else {
          minute.unreflected += 1
        }
      }
    }
    // The desktop is judged once per round: foreground, cursor and injected input.
    let after = watch.snapshot();
    let (verdict, timeline) = watch.judge(&before, &after);
    match verdict {
      Verdict::Held => minute.held += 1,
      Verdict::Inconclusive { .. } => minute.inconclusive += 1,
      Verdict::Violated { ref reasons } => {
        minute.violated += 1;
        tally.expect(false, format!("minute {minute_number} round {round}: the desktop changed: {reasons:?}"));
        // Rounds are judged as a whole, so a violation is recorded with what explains it: who
        // took the foreground, which input was injected and when the user last acted.
        records.write(
          "desktop_violation",
          json!({
            "minute": minute_number, "round": round, "verdict": verdict, "round_ms": after.taken_ms - before.taken_ms,
            "foreground_before": before.foreground.to_string(), "foreground_after": after.foreground.to_string(),
            "foreground_events": events(&timeline, before.taken_ms, &hwnds), "injected_input": injected(&timeline, before.taken_ms),
            "user_input_ms": user_input(&timeline, before.taken_ms),
            "physical_mouse": after.physical_mouse - before.physical_mouse, "physical_keys": after.physical_keys - before.physical_keys,
            "cursor": [[before.cursor.0, before.cursor.1], [after.cursor.0, after.cursor.1]],
            "last_physical_input_ms_before": before.taken_ms as i64 - after.last_physical_ms as i64,
          }),
        )?;
      }
    }
    last = after;
    minute.rounds += 1;
    round += 1;

    if minute_started.elapsed() >= Duration::from_secs(60) || run_started.elapsed() >= total {
      let seconds = minute_started.elapsed().as_secs_f64();
      let stats = session.stats();
      let ops = minute.ops();
      let received: u64 = {
        let mut total_clicks = 0;
        for receiver in receivers.iter_mut() {
          total_clicks += receiver.state()?.clicks;
        }
        total_clicks
      };
      let gauge = gauges();
      let per_minute = ops as f64 / seconds * 60.0;
      per_minute_summaries.push((
        per_minute,
        percentile(&minute.clicks.0, 50.0),
        percentile(&minute.scrolls.0, 50.0),
        percentile(&minute.observes.0, 50.0),
      ));
      gauges_by_minute.push(gauge.clone());
      let record = json!({
        "minute": minute_number, "seconds": seconds, "rounds": minute.rounds, "ops": ops, "ops_per_minute": per_minute,
        "click": minute.clicks.summary(), "scroll": minute.scrolls.summary(), "observe": minute.observes.summary(),
        "fresh_frame_read": minute.fresh_reads.summary(),
        "effects": {"clicks_delivered_this_minute": minute.delivered_clicks, "clicks_seen_by_receivers_total": received,
          "clicks_delivered_total": stats.clicks, "observations_reflected": minute.reflected, "observations_not_reflected": minute.unreflected},
        "desktop": {"held": minute.held, "inconclusive": minute.inconclusive, "violated": minute.violated, "receiver_takeovers": minute.takeovers},
        "session": stats, "window_enumerations_since_warmup": window_enumerations() - enumerations_after_warmup, "gauges": gauge,
      });
      println!(
        "minute {minute_number:>2}: {:>9.0} ops/min  click p50 {:.3} ms  scroll p50 {:.3} ms  observe p50 {:.1} ms p95 {:.1} ms  reflected {}/{}  desktop held {} inconclusive {} violated {}",
        per_minute,
        percentile(&minute.clicks.0, 50.0),
        percentile(&minute.scrolls.0, 50.0),
        percentile(&minute.observes.0, 50.0),
        percentile(&minute.observes.0, 95.0),
        minute.reflected,
        minute.reflected + minute.unreflected,
        minute.held,
        minute.inconclusive,
        minute.violated
      );
      tally
        .expect(minute.unreflected == 0, format!("minute {minute_number}: {} observation(s) never showed the action", minute.unreflected));
      tally.expect(
        received == stats.clicks,
        format!("minute {minute_number}: {} clicks delivered but the receivers saw {received}", stats.clicks),
      );
      records.write("minute", record.clone())?;
      minutes.push(record);
      minute = Minute::default();
      minute_number += 1;
      minute_started = Instant::now();
    }
  }

  // Degradation: the last minute against the first.
  let (first, last_minute) = (per_minute_summaries.first().copied(), per_minute_summaries.last().copied());
  let (degradation, within) = match (first, last_minute) {
    (Some(first), Some(last)) if per_minute_summaries.len() > 1 => {
      let ratio = last.0 / first.0;
      let latency_ratios = json!({"click_p50": last.1 / first.1, "scroll_p50": last.2 / first.2, "observe_p50": last.3 / first.3});
      (
        json!({"ops_per_minute_first": first.0, "ops_per_minute_last": last.0, "ratio_last_to_first": ratio, "latency_p50_ratio_last_to_first": latency_ratios}),
        ratio >= 0.9,
      )
    }
    _ => (json!({"note": "fewer than two full minutes; no degradation comparison"}), true),
  };
  tally.expect(within, "throughput in the last minute is more than 10% below the first minute");
  let growth = |key: &str| -> Option<f64> {
    let a = gauges_by_minute.first()?.get(key)?.as_f64()?;
    let b = gauges_by_minute.last()?.get(key)?.as_f64()?;
    Some(if a == 0.0 { 0.0 } else { (b - a) / a })
  };
  let stats = session.stats();
  let finish = watch.snapshot();
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the run");
  tally.expect(!hwnds.contains(&finish.foreground), "a receiver is the foreground window at the end");
  tally.expect(window_enumerations() == enumerations_after_warmup, "the session re-discovered windows during the run");
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({
      "passed": passed, "minutes_run": minutes.len(), "degradation": degradation, "within_ten_percent": within,
      "gauge_growth_first_to_last_minute": {"working_set_mb": growth("working_set_mb"), "handles": growth("handles"), "gdi_objects": growth("gdi_objects"), "user_objects": growth("user_objects")},
      "failures": tally.failures, "session": stats, "gap_findings": 0, "interrupted": interrupted,
      "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys,
      "physical_mouse_total": finish.physical_mouse, "physical_keys_total": finish.physical_keys,
    }),
  )?;
  println!(
    "bench: {} minute(s); throughput last/first {}; {}",
    minutes.len(),
    degradation["ratio_last_to_first"],
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("bench.jsonl").display());
  Ok(passed)
}

// ---------------------------------------------------------------------------------------------
// faults
// ---------------------------------------------------------------------------------------------

/// The fault-injection run: windows are closed, killed, moved, resized and minimized while the
/// session works, and it must say so explicitly and carry on.
pub fn faults(options: &FaultOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let mut records = Records::create(&options.output.join("faults.jsonl"), &run_id(), "faults")?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let mut tally = Tally::default();
  let launched = watch.snapshot();
  let (mut fleet, discovery_ms, discovery_enumerations) = start_fleet(&driver, &options.output, 0)?;
  let (warm, _) = warm_up(&mut fleet, &watch, &launched)?;
  let start = watch.snapshot();
  let hwnds: Vec<isize> = fleet.receivers.iter().map(Receiver::hwnd).collect();
  let total = Duration::from_secs_f64(options.minutes * 60.0);
  // (fraction of the run, what happens, which window)
  let plan = [
    (0.5, "close", 1usize),
    (0.6, "kill", 2),
    (0.7, "move", 3),
    (0.75, "resize", 3),
    (0.8, "minimize", 4),
    (0.9, "restore", 4),
  ];
  records.write(
    "setup",
    json!({"warm_up": warm, "minutes": options.minutes, "discovery": {"ms": discovery_ms, "enumerations": discovery_enumerations}, "foreground": start.foreground.to_string(),
      "system": system_facts(), "plan": plan.iter().map(|(fraction, what, window)| json!({"at_minutes": fraction * options.minutes, "fault": what, "window": window})).collect::<Vec<_>>()}),
  )?;
  tally.expect(!hwnds.contains(&start.foreground), "a receiver started as the foreground window");

  let Fleet {
    session,
    receivers,
    ids,
    windows: _,
    button,
    edit: _,
  } = &mut fleet;
  for &id in ids.iter() {
    let _ = session.capture(id);
  }
  let enumerations_after_warmup = window_enumerations();
  let mut desk = Desk::new(&watch, hwnds[0]);

  let run_started = Instant::now();
  let mut next_fault = 0usize;
  let mut round = 0u64;
  // How many times each (window, answer) pair has been recorded, to sample long stretches of the
  // same answer instead of writing thousands of identical lines.
  let mut seen: std::collections::HashMap<(usize, String), u64> = std::collections::HashMap::new();
  let mut answers: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
  let mut followed_reports = 0u64;
  let mut live_cycles = [0u64; WINDOWS];
  let mut unreflected_written = 0u32;

  while run_started.elapsed() < total {
    // Inject whichever fault is due.
    while next_fault < plan.len() && run_started.elapsed().as_secs_f64() >= plan[next_fault].0 * options.minutes * 60.0 {
      let (_, what, index) = plan[next_fault];
      next_fault += 1;
      let injected = match what {
        "close" => receivers[index].close().map(|()| json!({})),
        "kill" => {
          receivers[index].kill();
          Ok(json!({}))
        }
        "move" => {
          let rect = receivers[index].layout.window_rect;
          receivers[index]
            .request(json!({"cmd": "move", "left": rect[0] - 160, "top": rect[1] - 90}))
            .map(|reply| json!({"layout": reply["window_rect"]}))
        }
        "resize" => receivers[index]
          .request(json!({"cmd": "resize", "width": 460, "height": 340}))
          .map(|reply| json!({"layout": reply["window_rect"]})),
        "minimize" => receivers[index].request(json!({"cmd": "minimize"})).map(|_| json!({})),
        "restore" => receivers[index].request(json!({"cmd": "restore"})).map(|_| json!({})),
        _ => unreachable!("plan only names the faults above"),
      };
      tally.expect(injected.is_ok(), format!("injecting {what} into window {index} failed: {injected:?}"));
      if matches!(what, "move" | "resize") {
        // The window-relative points stay valid; only the strip moved, so decode from a fresh layout.
        let _ = receivers[index].refresh_layout();
      }
      records.write(
        "fault",
        json!({"fault": what, "window": index, "at_seconds": run_started.elapsed().as_secs_f64(), "injected": injected.ok(),
        "view": view_of(hwnds[index]).rect}),
      )?;
      println!("fault injected at {:.0}s: {what} window {index}", run_started.elapsed().as_secs_f64());
    }

    for index in 0..WINDOWS {
      let (id, hwnd) = (ids[index], hwnds[index]);
      let expected_lost = (index == 1 && next_fault > 0) || (index == 2 && next_fault > 1);
      let expected_minimized = index == 4 && next_fault > 4 && next_fault < 6;
      // Does the receiver still exist to answer? Not after a close or kill.
      let alive = !expected_lost;
      let before = if alive {
        receivers[index].state().ok()
      } else {
        None
      };

      let gaps_before = desk.gaps.len();
      let (measured, state) = desk.measure_at(
        hwnd,
        || session.click(id, button[index], ClickRequest::default()),
        || {
          if alive {
            receivers[index].state().ok()
          } else {
            None
          }
        },
      );
      tally.record(&measured);
      let kind = match &measured.value {
        Outcome::Done { value } => {
          if value.followed.is_some() {
            followed_reports += 1;
          }
          "done"
        }
        Outcome::SkippedMinimized => "skipped_minimized",
        Outcome::TargetLost { .. } => "target_lost",
        Outcome::Unsupported { .. } => "unsupported",
        Outcome::Failed { .. } => "failed",
      };
      *answers.entry(format!("window {index}: click {kind}")).or_default() += 1;
      let expected_kind = if expected_lost {
        "target_lost"
      } else if expected_minimized {
        "skipped_minimized"
      } else {
        "done"
      };
      tally.expect(kind == expected_kind, format!("round {round} window {index}: click answered {kind}, expected {expected_kind}"));
      if kind == "done" {
        live_cycles[index] += 1;
        let clicked = matches!((&before, &state), (Some(before), Some(after)) if after.clicks == before.clicks + 1);
        tally.expect(clicked, format!("round {round} window {index}: a delivered click did not reach the button"));
        let took = matches!((&before, &state), (Some(before), Some(after)) if after.foreground_taken != before.foreground_taken);
        tally.expect(!took, format!("round {round} window {index}: the receiver took the foreground"));
        if let Some(state) = &state {
          let strip = receivers[index].layout.strip.clone();
          let want = ((state.clicks & 0xFF) as u8, (state.first_line & 0xFF) as u8);
          let observed = observe(session, id, &strip, want, REFLECT_TIMEOUT);
          tally.expect(
            observed.reflected,
            format!("round {round} window {index}: no frame showed the click (window state {:?})", session.targets()[index].state),
          );
          if !observed.reflected && unreflected_written < 5 {
            unreflected_written += 1;
            if let Some(capture) = &observed.last_capture {
              let _ = capture.image.save(options.output.join(format!("observe-failure-round-{round}-window-{index}.png")));
            }
            records.write(
              "observe_failure",
              json!({"round": round, "target": index, "polls": observed.polls, "fresh_frames": observed.fresh, "waited_ms": observed.ms,
                "answer_other_than_a_frame": observed.other, "last_frame_decoded": observed.last_frame, "session_target": session.targets()[index],
                "receiver_layout": {"window_rect": receivers[index].layout.window_rect, "strip_origin": strip.origin}, "stats": session.stats()}),
            )?;
          }
        }
      } else {
        // A minimized window must stay minimized: the session never restores it.
        if kind == "skipped_minimized" {
          tally.expect(view_of(hwnd).iconic, format!("round {round} window {index}: a skipped window was restored"));
        }
        let observation = session.capture(id);
        let capture_kind = match observation {
          Outcome::Done { .. } => "done",
          Outcome::SkippedMinimized => "skipped_minimized",
          Outcome::TargetLost { .. } => "target_lost",
          Outcome::Unsupported { .. } => "unsupported",
          Outcome::Failed { .. } => "failed",
        };
        tally.expect(
          capture_kind == expected_kind,
          format!("round {round} window {index}: capture answered {capture_kind}, expected {expected_kind}"),
        );
      }
      // A gap the witness failed is written at once, with the round it preceded.
      for gap in desk.gaps[gaps_before..].iter().filter(|gap| gap["verdict"]["verdict"] == "violated") {
        records.write("gap_violation", json!({"round": round, "before_target": index, "gap": gap}))?;
      }
      // Sample the typed answers: the first few, then one in a hundred; every operation the
      // witness failed is kept.
      let count = seen.entry((index, kind.to_string())).or_default();
      *count += 1;
      if *count <= 3 || count.is_multiple_of(100) || measured.verdict.is_violation() {
        records.write(
          "op",
          json!({"op": "click", "round": round, "target": index, "hwnd": receivers[index].layout.hwnd, "outcome": typed(&measured.value), "latency_ms": measured.latency_ms,
            "answer_number": *count, "invariants": invariants(&measured, hwnd), "target_state": session.targets()[index].state}),
        )?;
      }
    }
    round += 1;
    std::thread::sleep(Duration::from_millis(options.round_pause_ms));
  }

  // What the session knew at the end.
  let report = session.refresh();
  let states: Vec<TargetState> = report.iter().map(|(info, _)| info.state).collect();
  tally.expect(
    matches!(states[1], TargetState::Lost { .. }) && matches!(states[2], TargetState::Lost { .. }),
    "the closed and killed windows are not marked lost",
  );
  tally.expect(
    states[0] == TargetState::Live && states[3] == TargetState::Live && states[4] == TargetState::Live,
    "a window that was never lost, or was only minimized, is not live",
  );
  tally.expect(
    followed_reports >= 2,
    format!("the session reported following the moved and resized window {followed_reports} time(s), expected at least 2"),
  );
  tally.expect(live_cycles[0] > 0 && live_cycles[3] > 0 && live_cycles[4] > 0, "a surviving window stopped being operated on");
  tally.expect(window_enumerations() == enumerations_after_warmup, "the session re-discovered windows during the run");
  let finish = watch.snapshot();
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the run");
  tally.expect(tally.violated == 0, format!("{} operation(s) disturbed the desktop", tally.violated));
  tally.expect(desk.gap_violations() == 0, format!("{} change(s) landed between operations", desk.gap_violations()));
  let stats = session.stats();
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({
      "passed": passed, "rounds": round, "ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "answers": answers, "final_states": report.iter().map(|(info, _)| json!({"id": info.id, "window": info.window, "state": info.state})).collect::<Vec<_>>(),
      "followed_reports": followed_reports, "live_cycles": live_cycles, "failures": tally.failures, "session": stats,
      "gap_findings": desk.gaps, "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys,
    }),
  )?;
  println!("faults: {round} round(s), {} ops, {} violated; {}", tally.ops, tally.violated, if passed { "PASS" } else { "FAIL" });
  for (answer, count) in &answers {
    println!("  {count:>7} x {answer}");
  }
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("faults.jsonl").display());
  Ok(passed)
}
