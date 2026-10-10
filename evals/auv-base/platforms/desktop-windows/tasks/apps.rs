//! Real applications, one per toolkit: what a background session can do with each.
//!
//! The receivers prove the session's own behavior against windows whose state is known exactly.
//! This survey asks the next question: which *applications* react to it. Each one is launched
//! detached and without activation, put under ordinary windows, covered by a receiver window laid
//! over it, and attached to a [`BackgroundSession`]. Then, per operation:
//!
//! - **capture**: the first frame shows the application, although it is covered. For the Edge test
//!   page it also shows whether the page content was painted, which a browser decides for itself.
//! - **click / double-click / scroll**: alternated between two targets, so each trial should change
//!   what the window shows. The effect is a change in a region of interest of the frames that
//!   follow, judged against a control that waits the same time without acting. Where the
//!   application reports its own state (the Edge test page puts its counts in its title), that is
//!   recorded too, so "the frame did not change" can be told apart from "the input was ignored".
//! - **keyboard**: answered `unsupported` without sending anything.
//!
//! The user's desktop comes first. An application is skipped when the user runs it already (a
//! launch could reuse their window), never typed into, and closed afterwards. Edge runs in a fresh
//! profile, which makes it a browser process of its own beside any Edge the user has open. The
//! harness moves only the windows it launched, and only under ordinary windows. Windows can let a
//! just-launched application take the foreground, which the harness cannot prevent: every
//! foreground change during a launch is recorded, and an application still holding the
//! foreground once it has settled is closed at once. Frames are saved only for applications whose
//! content is the harness's own; Notepad is not surveyed at all, because it can reopen the user's
//! unsaved documents.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use auv_driver_common::Driver;
use auv_driver_common::capture::Capture;
use auv_driver_common::geometry::WindowPoint;
use auv_driver_common::input::{Click, Scroll};
use auv_driver_common::window::Window;
use auv_driver_windows::{
  BackgroundSession, ClickRequest, FrameRead, Outcome, TargetId, WindowsDriver, WindowsDriverSession, snapshot_window,
};
use serde_json::{Value, json};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
  CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
  GetWindowTextW, GetWindowThreadProcessId, HWND_BOTTOM, IsWindow, PostMessageW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
  WM_CLOSE,
};

use super::background::{Desk, Tally, invariants, occlusion, placement, point_in_window, run_id, system_facts, view_of};
use super::receiver_process::{Receiver, ReceiverSpec, create_with_shell_parent, quote, receiver_binary};
use super::records::Records;
use super::watch::DesktopWatch;

pub struct AppsOptions {
  pub output: PathBuf,
  /// Survey only these applications (by name); all when empty.
  pub only: Vec<String>,
  /// Trials per operation. Targets alternate, so an even number keeps the window where it began.
  pub trials: u32,
}

const PAGE: &str = include_str!("../test-objects/web/receiver.html");
const EDGE: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
const COVER_COLOR: &str = "505050";
/// How long a frame may take to show an effect.
const EFFECT_WAIT: Duration = Duration::from_millis(1500);
/// A pixel changed when one of its channels moved by more than this.
const PIXEL_DELTA: u8 = 32;
/// A region changed when at least this share of its pixels, and at least `MIN_CHANGED_PIXELS`, did.
const CHANGED_SHARE: f64 = 0.002;
const MIN_CHANGED_PIXELS: usize = 25;

/// Where to aim in a window.
#[derive(Clone, Copy, Debug)]
enum Aim {
  /// A point given as fractions of the window's frame.
  At(f64, f64),
  /// The center of the UI Automation element with this AutomationId.
  AutomationId(&'static str),
}

#[derive(Clone, Debug)]
enum Act {
  Click(Click),
  /// Alternately down and up by this many wheel units.
  Scroll(f64),
}

struct Check {
  name: &'static str,
  act: Act,
  /// Alternated between trials.
  aims: [Aim; 2],
  /// Where the effect shows, as fractions of the frame: left, top, right, bottom.
  region: [f64; 4],
}

struct App {
  name: &'static str,
  toolkit: &'static str,
  /// Image names of a running instance that would be the user's.
  exclusive: &'static [&'static str],
  launch: fn(&Fixtures) -> Option<String>,
  /// Whether a window that appeared after the launch is the application's.
  is_window: fn(&Window) -> bool,
  checks: Vec<Check>,
  /// Operations that do not apply, and why.
  not_applicable: &'static [(&'static str, &'static str)],
  /// The window title carries the application's own counts.
  title_truth: bool,
  /// The background color of content the harness supplied (the Edge test page), so a frame can
  /// show whether that content was painted at all.
  content_color: Option<[u8; 3]>,
  /// The part of a frame that may be saved, as fractions of the frame (left, top, right, bottom):
  /// only what shows the harness's own content. `None` saves nothing.
  saved_region: Option<[f64; 4]>,
}

struct Fixtures {
  dir: PathBuf,
  page: PathBuf,
}

fn is_app(window: &Window, image: &str) -> bool {
  window.app_name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(image))
}

fn catalog() -> Vec<App> {
  vec![
    App {
      name: "charmap",
      toolkit: "classic Win32 dialog with an owner-drawn grid",
      exclusive: &["charmap.exe"],
      launch: |_| Some(r"C:\Windows\System32\charmap.exe".into()).filter(|path: &String| Path::new(path).exists()),
      is_window: |window| is_app(window, "charmap.exe"),
      checks: vec![
        Check {
          name: "click",
          act: Act::Click(Click::Single),
          aims: [Aim::At(0.18, 0.28), Aim::At(0.62, 0.40)],
          region: [0.0, 0.08, 1.0, 0.78],
        },
        Check {
          name: "scroll",
          act: Act::Scroll(360.0),
          aims: [Aim::At(0.45, 0.40), Aim::At(0.45, 0.40)],
          region: [0.0, 0.08, 1.0, 0.78],
        },
      ],
      not_applicable: &[],
      title_truth: false,
      content_color: None,
      saved_region: Some([0.0, 0.0, 1.0, 1.0]),
    },
    App {
      name: "calculator",
      toolkit: "UWP / XAML (ApplicationFrameHost)",
      exclusive: &["CalculatorApp.exe", "Calculator.exe"],
      launch: |_| Some(r"C:\Windows\System32\calc.exe".into()).filter(|path: &String| Path::new(path).exists()),
      is_window: |window| is_app(window, "ApplicationFrameHost.exe"),
      checks: vec![Check {
        name: "click",
        act: Act::Click(Click::Single),
        aims: [
          Aim::AutomationId("num7Button"),
          Aim::AutomationId("num8Button"),
        ],
        region: [0.0, 0.05, 1.0, 0.32],
      }],
      not_applicable: &[("scroll", "standard mode has nothing to scroll")],
      title_truth: false,
      content_color: None,
      saved_region: Some([0.0, 0.0, 1.0, 1.0]),
    },
    edge("edge", "Chromium (Microsoft Edge, default flags)", |fixtures| edge_command(fixtures, "edge-default", &[])),
    edge("edge-occlusion-off", "Chromium (Microsoft Edge, native window occlusion tracking off)", |fixtures| {
      edge_command(fixtures, "edge-occlusion-off", &["CalculateNativeWinOcclusion"])
    }),
  ]
}

/// Edge on the local test page, in a fresh profile so it runs as its own browser process beside
/// any Edge the user has open.
fn edge(name: &'static str, toolkit: &'static str, launch: fn(&Fixtures) -> Option<String>) -> App {
  App {
    name,
    toolkit,
    exclusive: &[],
    launch,
    is_window: |window| is_app(window, "msedge.exe") && window.title.as_deref().is_some_and(|title| title.contains("AUV page")),
    checks: vec![
      // The page's bar, with the buttons and its own counts.
      Check {
        name: "click",
        act: Act::Click(Click::Single),
        aims: [Aim::At(0.096, 0.218), Aim::At(0.267, 0.218)],
        region: [0.0, 0.15, 1.0, 0.31],
      },
      // The rows under the bar.
      Check {
        name: "scroll",
        act: Act::Scroll(360.0),
        aims: [Aim::At(0.5, 0.65), Aim::At(0.5, 0.65)],
        region: [0.0, 0.31, 1.0, 1.0],
      },
    ],
    not_applicable: &[],
    title_truth: true,
    content_color: Some([0x1e, 0x3a, 0x5f]),
    // NOTICE: the toolbar is left out. A fresh Edge profile can sign in to the Windows account on
    // its own and show that account's picture there; `msImplicitSignin` is disabled at launch,
    // and the crop keeps the picture out of saved frames if a build ignores that.
    saved_region: Some([0.0, 0.15, 1.0, 1.0]),
  }
}

/// `disabled`: Chromium features to turn off besides implicit sign-in, which would sign the fresh
/// profile in to the Windows account.
fn edge_command(fixtures: &Fixtures, profile: &str, disabled: &[&str]) -> Option<String> {
  if !Path::new(EDGE).exists() {
    return None;
  }
  let (left, top) = placement(0, 760, 560);
  let features = ["msImplicitSignin"].iter().chain(disabled).copied().collect::<Vec<_>>().join(",");
  Some(format!(
    "{} --user-data-dir={} --no-first-run --no-default-browser-check --disable-sync --disable-features={features} --window-position={left},{top} --window-size=760,560 --new-window {}",
    quote(EDGE),
    quote(&fixtures.dir.join(profile).display().to_string()),
    quote(&format!("file:///{}", fixtures.page.display().to_string().replace('\\', "/")))
  ))
}

fn running_images() -> HashSet<String> {
  let mut names = HashSet::new();
  // SAFETY: a process snapshot read with a correctly sized entry, then closed.
  unsafe {
    let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
      return names;
    };
    let mut entry = PROCESSENTRY32W {
      dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
      ..Default::default()
    };
    if Process32FirstW(snapshot, &mut entry).is_ok() {
      loop {
        let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        names.insert(String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase());
        if Process32NextW(snapshot, &mut entry).is_err() {
          break;
        }
      }
    }
    let _ = CloseHandle(snapshot);
  }
  names
}

fn hwnd(window: &Window) -> isize {
  window.reference.id.parse().unwrap_or(0)
}

fn owner(hwnd: isize) -> u32 {
  let mut pid = 0;
  // SAFETY: a read-only query on a handle that may be gone.
  unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid)) };
  pid
}

fn title(hwnd: isize) -> String {
  let mut buffer = [0u16; 512];
  // SAFETY: a read-only query into a buffer of the length given.
  let length = unsafe { GetWindowTextW(HWND(hwnd as *mut core::ffi::c_void), &mut buffer) };
  String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

/// The Edge test page's own counts, from the window title.
fn page_counts(hwnd: isize) -> Option<(u64, u64)> {
  let title = title(hwnd);
  let number = |key: &str| title.split(key).nth(1)?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok();
  Some((number("clicks=")?, number("scroll=")?))
}

/// Harness fixture placement for a window it launched: under ordinary windows, never activated.
fn restack(hwnd: isize) {
  // SAFETY: Z-order only; no move, no resize, no activation.
  unsafe {
    let _ = SetWindowPos(HWND(hwnd as *mut core::ffi::c_void), HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
  }
}

/// Closes a window the harness launched, and ends its process if it is still there and the harness
/// started it.
fn close_launched(hwnd: isize, pid: u32, preexisting: &HashSet<u32>) -> Value {
  let handle = HWND(hwnd as *mut core::ffi::c_void);
  // SAFETY: a close request to a window this harness launched.
  unsafe {
    let _ = PostMessageW(handle, WM_CLOSE, WPARAM(0), LPARAM(0));
  }
  let started = Instant::now();
  // SAFETY: read-only handle checks.
  while unsafe { IsWindow(handle) }.as_bool() && started.elapsed() < Duration::from_secs(5) {
    std::thread::sleep(Duration::from_millis(50));
  }
  let mut ended = false;
  if pid != 0 && !preexisting.contains(&pid) {
    // SAFETY: ending a process this harness started, and waiting for it.
    unsafe {
      if let Ok(process) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
        if WaitForSingleObject(process, 2000).0 != 0 {
          let _ = TerminateProcess(process, 0);
          let _ = WaitForSingleObject(process, 2000);
          ended = true;
        }
        let _ = CloseHandle(process);
      }
    }
  }
  json!({"window_closed": !unsafe { IsWindow(handle) }.as_bool(), "process_terminated": ended})
}

/// Share of the region's pixels that differ between two frames of the same size.
fn changed(a: &Capture, b: &Capture, region: [f64; 4]) -> (f64, usize) {
  let (width, height) = (a.image.width(), a.image.height());
  if (width, height) != (b.image.width(), b.image.height()) {
    return (1.0, usize::MAX);
  }
  let x = ((region[0] * f64::from(width)) as u32, (region[2] * f64::from(width)) as u32);
  let y = ((region[1] * f64::from(height)) as u32, (region[3] * f64::from(height)) as u32);
  let (mut total, mut count) = (0usize, 0usize);
  for row in y.0..y.1.min(height) {
    for column in x.0..x.1.min(width) {
      total += 1;
      let (p, q) = (a.image.get_pixel(column, row).0, b.image.get_pixel(column, row).0);
      if (0..3).any(|channel| p[channel].abs_diff(q[channel]) > PIXEL_DELTA) {
        count += 1;
      }
    }
  }
  (count as f64 / total.max(1) as f64, count)
}

/// Saves the part of `frame` that `app` allows.
fn save(app: &App, frame: &Capture, path: &Path) {
  let Some(region) = app.saved_region else {
    return;
  };
  let (width, height) = (f64::from(frame.image.width()), f64::from(frame.image.height()));
  let (left, top) = ((region[0] * width) as u32, (region[1] * height) as u32);
  let (right, bottom) = ((region[2] * width) as u32, (region[3] * height) as u32);
  let _ = image::imageops::crop_imm(&frame.image, left, top, right.saturating_sub(left), bottom.saturating_sub(top)).to_image().save(path);
}

fn is_change((share, count): (f64, usize)) -> bool {
  share >= CHANGED_SHARE && count >= MIN_CHANGED_PIXELS
}

/// The newest frame: reads until the stream has nothing newer, keeping `latest` when nothing
/// arrived at all.
fn drain(session: &mut BackgroundSession, id: TargetId, latest: &mut Option<Capture>, wait: Duration) {
  let started = Instant::now();
  loop {
    match session.capture(id) {
      Outcome::Done {
        value: FrameRead::Fresh { capture, .. },
      } => *latest = Some(capture),
      _ if latest.is_some() => return,
      _ => {}
    }
    if started.elapsed() >= wait {
      return;
    }
    std::thread::sleep(Duration::from_millis(15));
  }
}

/// The first frame after `since` whose region differs from `baseline`, within the wait.
fn await_change(session: &mut BackgroundSession, id: TargetId, baseline: &Capture, region: [f64; 4]) -> (Option<f64>, Option<Capture>, f64) {
  let started = Instant::now();
  let mut newest = None;
  let mut largest = 0.0f64;
  while started.elapsed() < EFFECT_WAIT {
    if let Outcome::Done {
      value: FrameRead::Fresh { capture, .. },
    } = session.capture(id)
    {
      let difference = changed(baseline, &capture, region);
      largest = largest.max(difference.0);
      if is_change(difference) {
        return (Some(started.elapsed().as_secs_f64() * 1000.0), Some(capture), largest);
      }
      newest = Some(capture);
    }
    std::thread::sleep(Duration::from_millis(5));
  }
  (None, newest, largest)
}

fn resolve(aim: Aim, window: &Window) -> Result<WindowPoint> {
  let element = |matches: &dyn Fn(&auv_driver_windows::AxNode) -> bool| -> Result<WindowPoint> {
    let snapshot = snapshot_window(window).map_err(|error| anyhow::anyhow!("UI Automation snapshot: {error}"))?;
    let node = snapshot.nodes.iter().find(|node| matches(node)).context("no such UI Automation element")?;
    let center = [
      (node.bounds.origin.x + node.bounds.size.width / 2.0) as i32,
      (node.bounds.origin.y + node.bounds.size.height / 2.0) as i32,
    ];
    Ok(point_in_window(window, center))
  };
  match aim {
    Aim::At(x, y) => Ok(WindowPoint::new(window.frame.size.width * x, window.frame.size.height * y)),
    Aim::AutomationId(id) => element(&|node| node.automation_id == id),
  }
}

fn outcome_kind<T>(outcome: &Outcome<T>) -> &'static str {
  match outcome {
    Outcome::Done { .. } => "done",
    Outcome::SkippedMinimized => "skipped_minimized",
    Outcome::TargetLost { .. } => "target_lost",
    Outcome::Unsupported { .. } => "unsupported",
    Outcome::Failed { .. } => "failed",
  }
}

struct Survey<'a> {
  driver: &'a WindowsDriverSession,
  watch: &'a DesktopWatch,
  records: &'a mut Records,
  tally: &'a mut Tally,
  output: &'a Path,
  trials: u32,
}

impl Survey<'_> {
  /// Runs every check of `app` against its window in its current placement. `latest` is the
  /// newest frame so far: a window whose placement changed but whose content did not delivers no
  /// new frame.
  fn checks(
    &mut self,
    app: &App,
    session: &mut BackgroundSession,
    id: TargetId,
    window: &Window,
    placement: &str,
    latest: &mut Option<Capture>,
  ) -> Result<Value> {
    let target = hwnd(window);
    let mut desk = Desk::new(self.watch, target);
    drain(session, id, latest, Duration::from_secs(3));
    let Some(first) = latest.clone() else {
      return Ok(json!({"placement": placement, "capture": {"result": "no frame within 3 s"}}));
    };
    let black = first.image.pixels().filter(|pixel| pixel.0[0] < 11 && pixel.0[1] < 11 && pixel.0[2] < 11).count() as f64
      / f64::from(first.image.width() * first.image.height()).max(1.0);
    let cover = first
      .image
      .pixels()
      .filter(|pixel| pixel.0[0].abs_diff(0x50) < 4 && pixel.0[1].abs_diff(0x50) < 4 && pixel.0[2].abs_diff(0x50) < 4)
      .count() as f64
      / f64::from(first.image.width() * first.image.height()).max(1.0);
    save(app, &first, &self.output.join(format!("{}-{placement}.png", app.name)));
    let mut capture = json!({"result": "done", "size": [first.image.width(), first.image.height()], "black_share": black, "cover_color_share": cover,
      "shows_the_application": black < 0.5 && cover < 0.2});
    if let Some(color) = app.content_color {
      let share = first.image.pixels().filter(|pixel| (0..3).all(|channel| pixel.0[channel].abs_diff(color[channel]) < 6)).count() as f64
        / f64::from(first.image.width() * first.image.height()).max(1.0);
      capture["content_color_share"] = json!(share);
      capture["shows_the_content"] = json!(share > 0.1);
    }

    let mut results = Vec::new();
    for check in &app.checks {
      let points: Vec<Result<WindowPoint>> = check.aims.iter().map(|aim| resolve(*aim, window)).collect();
      if let Some(Err(error)) = points.iter().find(|point| point.is_err()) {
        results.push(json!({"operation": check.name, "result": "not_located", "reason": format!("{error:#}")}));
        continue;
      }
      let points: Vec<WindowPoint> = points.into_iter().map(|point| point.expect("checked above")).collect();
      let (mut observed, mut delivered, mut truth_moved) = (0, 0, 0);
      let mut latencies = Vec::new();
      for trial in 0..self.trials {
        let point = points[trial as usize % 2];
        drain(session, id, latest, Duration::from_millis(300));
        let baseline = latest.clone().context("lost the baseline frame")?;
        let truth_before = if app.title_truth {
          page_counts(target)
        } else {
          None
        };
        let (measured, ()) = desk.measure_at(
          target,
          || match &check.act {
            Act::Click(click) => session.click(
              id,
              point,
              ClickRequest {
                click: click.clone(),
                ..ClickRequest::default()
              },
            ),
            Act::Scroll(amount) => session.scroll(id, point, Scroll::new(0.0, if trial % 2 == 0 { *amount } else { -amount })),
          },
          || (),
        );
        self.tally.record(&measured);
        let kind = outcome_kind(&measured.value);
        if kind == "done" {
          delivered += 1;
        }
        let (latency, frame, largest) = await_change(session, id, &baseline, check.region);
        if let Some(frame) = frame {
          *latest = Some(frame);
        }
        std::thread::sleep(Duration::from_millis(250));
        if trial < 2 {
          drain(session, id, latest, Duration::from_millis(100));
          if let Some(frame) = &*latest {
            save(app, frame, &self.output.join(format!("{}-{placement}-{}-{trial}.png", app.name, check.name)));
          }
        }
        let truth_after = if app.title_truth {
          page_counts(target)
        } else {
          None
        };
        let moved = matches!((truth_before, truth_after), (Some(before), Some(after)) if before != after);
        if moved {
          truth_moved += 1;
        }
        if let Some(latency) = latency {
          observed += 1;
          latencies.push(latency);
        }
        self.records.write(
          "op",
          json!({"app": app.name, "placement": placement, "operation": check.name, "trial": trial, "point": [point.0.x, point.0.y],
            "outcome": serde_json::to_value(measured.value.without_value()).unwrap_or(Value::Null), "latency_ms": measured.latency_ms,
            "effect": {"frame_changed": latency.is_some(), "after_ms": latency, "largest_changed_share": largest,
              "application_state": truth_after.map(|(clicks, scroll)| json!({"clicks": clicks, "scroll": scroll})), "application_state_changed": moved},
            "invariants": invariants(&measured, target)}),
        )?;
      }
      // Control: the same wait with nothing sent. A region that changes anyway is noise.
      let mut noise = 0;
      for _ in 0..2 {
        drain(session, id, latest, Duration::from_millis(300));
        let baseline = latest.clone().context("lost the baseline frame")?;
        let (latency, frame, _) = await_change(session, id, &baseline, check.region);
        if latency.is_some() {
          noise += 1;
        }
        if let Some(frame) = frame {
          *latest = Some(frame);
        }
      }
      let verdict = match (observed, noise) {
        (_, noise) if noise > 0 => "inconclusive: the region changes without input",
        (observed, _) if observed == self.trials => "effect",
        (0, _) if app.title_truth && truth_moved == self.trials => "handled, frames did not change",
        (0, _) if app.title_truth && truth_moved > 0 => "partly handled, frames did not change",
        (0, _) => "no effect",
        _ => "partial",
      };
      results.push(json!({"operation": check.name, "trials": self.trials, "delivered": delivered, "frame_changed": observed,
        "application_state_changed": app.title_truth.then_some(truth_moved), "control_changes": noise, "verdict": verdict,
        "after_ms": latencies}));
    }
    let typed = session.type_text(id, "x");
    results.push(json!({"operation": "type_text", "result": outcome_kind(&typed), "verdict": if matches!(typed, Outcome::Unsupported { .. }) { "unsupported (typed, nothing sent)" } else { "unexpected" }}));
    self.tally.expect(desk.gap_violations() == 0, format!("{}: the desktop changed between operations", app.name));
    let listed = self.driver.window().list().map_err(|error| anyhow::anyhow!("window list: {error}"))?;
    let above = listed.iter().position(|other| other.reference.id == window.reference.id);
    Ok(
      json!({"placement": placement, "occluded_fraction": occlusion(self.driver, window), "listed_windows_above": above, "capture": capture, "operations": results,
        "gap_findings": desk.gaps}),
    )
  }

  fn app(&mut self, app: &App, fixtures: &Fixtures) -> Result<Value> {
    let running = running_images();
    if let Some(image) = app.exclusive.iter().find(|image| running.contains(&image.to_ascii_lowercase())) {
      return Ok(
        json!({"app": app.name, "toolkit": app.toolkit, "skipped": format!("{image} is already running for the user; a launch could reuse their window")}),
      );
    }
    let Some(command) = (app.launch)(fixtures) else {
      return Ok(json!({"app": app.name, "toolkit": app.toolkit, "skipped": "not installed"}));
    };
    let listed = |driver: &WindowsDriverSession| driver.window().list().map_err(|error| anyhow::anyhow!("window list: {error}"));
    let before: HashSet<String> = listed(self.driver)?.into_iter().map(|window| window.reference.id).collect();
    let preexisting: HashSet<u32> = listed(self.driver)?.iter().filter_map(|window| window.process_id).collect();
    let launched = self.watch.snapshot();
    let _launcher = create_with_shell_parent(&command).with_context(|| format!("launch {}", app.name))?;

    let started = Instant::now();
    let window = loop {
      let found = listed(self.driver)?
        .into_iter()
        .filter(|window| !before.contains(&window.reference.id) && window.is_visible && (app.is_window)(window))
        .max_by(|a, b| (a.frame.size.width * a.frame.size.height).total_cmp(&(b.frame.size.width * b.frame.size.height)));
      if let Some(window) = found {
        break window;
      }
      anyhow::ensure!(started.elapsed() < Duration::from_secs(20), "{}: no window appeared within 20 s", app.name);
      std::thread::sleep(Duration::from_millis(100));
    };
    let target = hwnd(&window);
    let pid = window.process_id.unwrap_or_else(|| owner(target));
    let appeared_ms = started.elapsed().as_secs_f64() * 1000.0;
    // Let it finish starting, then see whether it took the foreground.
    std::thread::sleep(Duration::from_millis(1500));
    let settled = self.watch.snapshot();
    let (launch_verdict, timeline) = self.watch.judge(&launched, &settled);
    let holds_foreground = owner(settled.foreground) == pid && settled.foreground != launched.foreground;
    // Whose windows came to the foreground while it started: the application's, the window that
    // was foreground before, or another. Only ownership is recorded, never the user's titles.
    let foreground_events: Vec<Value> = timeline
      .foreground_events
      .iter()
      .map(|event| {
        let whose = match event.actual {
          actual if owner(actual) == pid => "application",
          actual if actual == launched.foreground => "previous_foreground",
          _ => "other",
        };
        json!({"at_ms": event.at.saturating_sub(launched.taken_ms), "foreground": whose})
      })
      .collect();
    let user_input = timeline.discrete_input.len() as u64 + (settled.physical_mouse - launched.physical_mouse).min(1);
    // A launch that brought the application to the foreground disturbed the user even if the
    // foreground has gone back since. It is the launch's doing, not the session's: recorded here,
    // and the run goes on when the foreground is no longer the application's.
    let took_foreground = holds_foreground || foreground_events.iter().any(|event| event["foreground"] == "application");
    let mut row = json!({"app": app.name, "toolkit": app.toolkit, "command": command, "window": {"hwnd": window.reference.id, "pid": pid,
      "app_name": window.app_name, "frame": window.frame, "appeared_after_ms": appeared_ms},
      "launch": {"took_foreground": took_foreground, "holds_foreground": holds_foreground, "verdict": launch_verdict, "foreground_events": foreground_events, "user_input": user_input > 0}, "not_applicable": app.not_applicable.iter().map(|(operation, why)| json!({"operation": operation, "why": why})).collect::<Vec<_>>()});
    if holds_foreground {
      self.tally.expect(false, format!("{}: the application took the foreground when launched; it was closed", app.name));
      row["cleanup"] = close_launched(target, pid, &preexisting);
      return Ok(row);
    }

    let mut session = self.driver.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
    let id = session.attach(&window).map_err(|error| anyhow::anyhow!("attach: {error}"))?;
    let mut latest = None;
    // NOTICE: there is no uncovered pass. Raising a window without activating it does not lift it
    // above the user's foreground window (measured: `SetWindowPos(HWND_TOP, SWP_NOACTIVATE)` from
    // the harness left Edge below a maximized foreground window), and activating it is the
    // disturbance this work rules out. A background target is covered, so that is what is measured.
    // Under ordinary windows, and under a receiver laid exactly over it.
    restack(target);
    let rect = view_of(target).rect;
    let cover = Receiver::spawn_detached(
      &receiver_binary()?,
      &ReceiverSpec {
        index: 9,
        title: "AUV cover".into(),
        left: rect[0],
        top: rect[1],
        width: rect[2] - rect[0],
        height: rect[3] - rect[1],
        color: COVER_COLOR.into(),
        sink: true,
      },
    )?;
    restack(target);
    std::thread::sleep(Duration::from_millis(500));
    let window = listed(self.driver)?
      .into_iter()
      .find(|listed| listed.reference.id == window.reference.id)
      .context("the application window went away")?;
    let pass = self.checks(app, &mut session, id, &window, "covered", &mut latest)?;
    drop(cover);
    row["passes"] = json!([pass]);
    row["session"] = serde_json::to_value(session.stats())?;
    session.detach(id);
    row["cleanup"] = close_launched(target, pid, &preexisting);
    Ok(row)
  }
}

/// Surveys the applications and writes `apps.jsonl`. Passes when the desktop was never disturbed;
/// what each application does with the input is a finding, not a pass condition.
pub fn apps(options: &AppsOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let run = run_id();
  let mut records = Records::create(&options.output.join("apps.jsonl"), &run, "apps")?;
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let fixtures = Fixtures {
    dir: std::env::temp_dir().join(format!("auv-apps-{run}")),
    page: std::env::temp_dir().join(format!("auv-apps-{run}")).join("receiver.html"),
  };
  std::fs::create_dir_all(&fixtures.dir)?;
  std::fs::write(&fixtures.page, PAGE)?;
  let start = watch.snapshot();
  records.write("setup", json!({"system": system_facts(), "foreground": start.foreground.to_string(), "trials": options.trials}))?;

  let mut tally = Tally::default();
  let mut rows = Vec::new();
  for app in catalog().iter().filter(|app| options.only.is_empty() || options.only.iter().any(|name| name == app.name)) {
    let mut survey = Survey {
      driver: &driver,
      watch: &watch,
      records: &mut records,
      tally: &mut tally,
      output: &options.output,
      trials: options.trials.max(2),
    };
    let row =
      survey.app(app, &fixtures).unwrap_or_else(|error| json!({"app": app.name, "toolkit": app.toolkit, "error": format!("{error:#}")}));
    records.write("app", row.clone())?;
    println!("{}", serde_json::to_string(&row)?);
    rows.push(row);
  }

  let finish = watch.snapshot();
  tally.expect(finish.injected_mouse == 0 && finish.injected_keys == 0, "global input was injected during the survey");
  tally.expect(tally.violated == 0, format!("{} operation(s) disturbed the desktop", tally.violated));
  let passed = tally.failures.is_empty();
  records.write(
    "summary",
    json!({"passed": passed, "ops": tally.ops, "held": tally.held, "inconclusive": tally.inconclusive, "violated": tally.violated,
      "failures": tally.failures, "foreground_start": start.foreground.to_string(), "foreground_end": finish.foreground.to_string(),
      "injected_mouse_total": finish.injected_mouse, "injected_keys_total": finish.injected_keys}),
  )?;
  // The Edge profile is a scratch copy of a browser profile; nothing in it is evidence.
  for _ in 0..10 {
    if std::fs::remove_dir_all(&fixtures.dir).is_ok() {
      break;
    }
    std::thread::sleep(Duration::from_millis(500));
  }
  println!(
    "apps: {} ops, {} held, {} inconclusive, {} violated; {}",
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("apps.jsonl").display());
  Ok(passed)
}
