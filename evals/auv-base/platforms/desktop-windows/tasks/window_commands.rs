//! `window`: the `window.*` invoke commands on Windows, run as the real `auv` binary against
//! windows this harness owns, while an independent watch checks the user's desktop.
//!
//! The target is a receiver covered by a second harness window, so the commands meet a window
//! nobody can see. Every selector names the receiver's title: a command that resolved "the main
//! window" would capture whatever the user is working in. The commands record their runs into a
//! store inside the output directory, which is removed at the end because `window.list` records
//! every window on the desktop.
//!
//! What each command shows is judged against the receiver's own facts, never against what the
//! driver reports about itself:
//!
//! - the window's frame against the DWM extended frame the harness reads itself;
//! - the saved capture by decoding the receiver's click strip from its pixels, which only lines
//!   up when the capture's bounds and scale place the image on the screen correctly;
//! - recognized text against the screen rectangle of the control that draws it.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use auv_driver_common::Driver;
use auv_driver_common::capture::Capture;
use auv_driver_common::geometry::Rect;
use auv_driver_windows::{ClickRequest, WindowsDriver};
use serde_json::{Value, json};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_SHOWMINNOACTIVE, ShowWindow};

use super::background::{Desk, Tally, discover_by_hwnd, invariants, occlusion, point_in_window, run_id, spawn_receiver_with, system_facts};
use super::receiver_process::{Layout, Receiver, ReceiverSpec, receiver_binary};
use super::records::Records;
use super::sustained::decode_strip;
use super::watch::DesktopWatch;

/// Keeps a child from opening a console window on the user's desktop.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct WindowCommandsOptions {
  pub output: PathBuf,
  /// The `auv` executable under test.
  pub auv: PathBuf,
}

/// What the OCR box of a caption may differ from its control by, in pixels: the caption sits a
/// few pixels inside the control, and the recognizer's box is not tight.
pub(super) const TEXT_SLACK: f64 = 4.0;

pub(super) struct Invocation {
  pub(super) exit: Option<i32>,
  pub(super) ms: f64,
  pub(super) json: Option<Value>,
}

impl Invocation {
  pub(super) fn completed(&self) -> bool {
    self.exit == Some(0) && self.json.as_ref().is_some_and(|json| json["status"] == "completed")
  }

  pub(super) fn failure(&self) -> String {
    self.json.as_ref().and_then(|json| json["failure"].as_str()).unwrap_or("").to_string()
  }
}

pub(super) fn invoke(auv: &Path, store: &Path, command: &str, args: &[&str]) -> Result<Invocation> {
  let started = Instant::now();
  let output = Command::new(auv)
    .arg("invoke")
    .arg(command)
    .args(args)
    .args(["--json", "--no-overlay", "--store-root"])
    .arg(store)
    .stdin(Stdio::null())
    .creation_flags(CREATE_NO_WINDOW)
    .output()
    .with_context(|| format!("run {} invoke {command}", auv.display()))?;
  Ok(Invocation {
    exit: output.status.code(),
    ms: started.elapsed().as_secs_f64() * 1000.0,
    json: serde_json::from_slice(&output.stdout).ok(),
  })
}

/// `[x, y, width, height]` of a serialized `Rect`.
pub(super) fn rect_of(value: &Value) -> Option<[f64; 4]> {
  Some([
    value["origin"]["x"].as_f64()?,
    value["origin"]["y"].as_f64()?,
    value["size"]["width"].as_f64()?,
    value["size"]["height"].as_f64()?,
  ])
}

/// The window's visible rectangle as DWM reports it, `[x, y, width, height]`: the harness's own
/// reading, so the driver's frame is not judged against itself.
fn visible_frame(hwnd: isize) -> Option<[f64; 4]> {
  let mut rect = RECT::default();
  // SAFETY: a read-only attribute query with a writable rectangle of the size DWM expects.
  unsafe {
    DwmGetWindowAttribute(
      HWND(hwnd as *mut c_void),
      DWMWA_EXTENDED_FRAME_BOUNDS,
      &mut rect as *mut RECT as *mut c_void,
      size_of::<RECT>() as u32,
    )
  }
  .ok()?;
  Some([
    f64::from(rect.left),
    f64::from(rect.top),
    f64::from(rect.right - rect.left),
    f64::from(rect.bottom - rect.top),
  ])
}

fn same_rect(left: [f64; 4], right: [f64; 4], tolerance: f64) -> bool {
  left.iter().zip(right.iter()).all(|(a, b)| (a - b).abs() <= tolerance)
}

/// Whether `inner` (`[x, y, w, h]`) lies inside `outer` (`[left, top, right, bottom]`) grown by
/// `slack` on every side.
pub(super) fn inside(inner: [f64; 4], outer: [i32; 4], slack: f64) -> bool {
  inner[0] >= f64::from(outer[0]) - slack
    && inner[1] >= f64::from(outer[1]) - slack
    && inner[0] + inner[2] <= f64::from(outer[2]) + slack
    && inner[1] + inner[3] <= f64::from(outer[3]) + slack
}

/// Whether left-aligned text sits where a caption does inside its control: a few pixels from the
/// top-left corner. `offset` is the text box's origin minus the control's.
pub(super) fn near_corner([x, y]: [f64; 2]) -> bool {
  (-2.0..=5.0).contains(&x) && (-1.0..=7.0).contains(&y)
}

/// How recognized text is judged against the control that draws it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
  /// Near the control's top-left corner, as left-aligned text is.
  Corner,
  /// Anywhere inside the control.
  Inside,
  /// Written to the record only.
  Recorded,
}

/// The first match of `query` in a `window.findText` result, as `[x, y, width, height]`.
fn first_match(run: &Invocation) -> Option<[f64; 4]> {
  rect_of(&run.json.as_ref()?["result"]["matches"]["matches"][0]["bounds"])
}

/// Runs the window commands and writes `window.jsonl`. Passes when every command showed what the
/// receiver knows to be true and the desktop was never disturbed.
pub fn window_commands(options: &WindowCommandsOptions) -> Result<bool> {
  std::fs::create_dir_all(&options.output).with_context(|| format!("create {}", options.output.display()))?;
  let auv = options.auv.canonicalize().with_context(|| format!("find the auv executable {}", options.auv.display()))?;
  let run = run_id();
  let mut records = Records::create(&options.output.join("window.jsonl"), &run, "window")?;
  let store = options.output.join("store");
  let watch = DesktopWatch::start()?;
  let driver = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let mut tally = Tally::default();

  // The window the commands act on, and a second harness window over it.
  let mut target = spawn_receiver_with(&options.output, 0, true)?;
  let spec = target.spec.clone();
  // A window that has been moved reports a visible frame inside its window rectangle: the
  // invisible resize border is left out. A window that never moved does not, so the geometry
  // checks below could not fail on it.
  for (left, top) in [(spec.left - 160, spec.top - 90), (spec.left, spec.top)] {
    target.request(json!({"cmd": "move", "left": left, "top": top}))?;
    std::thread::sleep(Duration::from_millis(300));
  }
  let [left, top, right, bottom] = target.refresh_layout()?.window_rect;
  let cover_spec = ReceiverSpec {
    index: 4,
    title: "AUV eval cover".into(),
    left: left - 12,
    top: top - 12,
    width: right - left + 24,
    height: bottom - top + 24,
    color: "9E8A2E".into(),
    sink: false,
  };
  let mut cover = Receiver::spawn_detached(&receiver_binary()?, &cover_spec)?;
  let hwnd: isize = target.layout.hwnd.parse().context("receiver window handle")?;
  let (window, _) = discover_by_hwnd(&driver, hwnd)?;
  let covered = occlusion(&driver, &window);

  // Give the receiver a state worth reading back: three clicks, posted to the window.
  let mut session = driver.background().map_err(|error| anyhow::anyhow!("open background session: {error}"))?;
  let id = session.attach(&window).map_err(|error| anyhow::anyhow!("attach the receiver: {error}"))?;
  let button = point_in_window(&window, target.layout.button_center);
  let delivered = (0..3).filter(|_| session.click(id, button, ClickRequest::default()).is_done()).count();
  std::thread::sleep(Duration::from_millis(200));
  let state = target.state()?;
  let layout: Layout = target.refresh_layout()?;
  tally.expect(delivered == 3 && state.clicks == 3, format!("the receiver counted {} of 3 posted clicks", state.clicks));

  let frame_truth = visible_frame(hwnd).context("read the receiver's DWM frame")?;
  let border = [
    frame_truth[0] - f64::from(layout.window_rect[0]),
    frame_truth[1] - f64::from(layout.window_rect[1]),
    f64::from(layout.window_rect[2]) - frame_truth[0] - frame_truth[2],
    f64::from(layout.window_rect[3]) - frame_truth[1] - frame_truth[3],
  ];
  tally.expect(border.iter().any(|edge| *edge >= 1.0), "the receiver has no invisible resize border, so the geometry checks cannot fail");
  let start = watch.snapshot();
  records.write(
    "setup",
    json!({"system": system_facts(), "auv": auv.file_name().map(|name| name.to_string_lossy().into_owned()),
      "receiver": {"title": spec.title, "pid": layout.pid, "window_rect": layout.window_rect, "dwm_frame": frame_truth,
        "invisible_border": border, "button_rect": layout.button_rect, "label_rect": layout.label_rect,
        "edit_rect": layout.edit_rect, "clicks": state.clicks},
      "cover_fraction": covered, "foreground": start.foreground.to_string()}),
  )?;
  tally.expect(covered >= 0.99, format!("the receiver was only {covered:.2} covered"));

  let title = spec.title.as_str();
  let mut desk = Desk::new(&watch, hwnd);
  std::fs::create_dir_all(&store)?;

  // window.list: the receiver is found by its handle, with the facts the receiver knows.
  let (listed, _) = desk.measure(|| invoke(&auv, &store, "window.list", &[]), || ());
  tally.record(&listed);
  let list = listed.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  let entries = list.json.as_ref().and_then(|json| json["result"].as_array()).cloned().unwrap_or_default();
  let hwnd_text = hwnd.to_string();
  let found = entries.iter().find(|entry| entry["reference"]["id"].as_str() == Some(hwnd_text.as_str()));
  let listed_frame = found.and_then(|entry| rect_of(&entry["frame"]));
  tally.expect(list.completed(), format!("window.list failed: {}", list.failure()));
  tally.expect(found.is_some(), "window.list did not list the receiver");
  tally.expect(
    // The receiver appends its own counters to its title.
    found.is_some_and(|entry| {
      entry["app_name"] == "win32-receiver.exe"
        && entry["process_id"] == layout.pid
        && entry["title"].as_str().is_some_and(|text| text.starts_with(title))
    }),
    "window.list gave the receiver another app name, process or title",
  );
  tally.expect(listed_frame.is_some_and(|frame| same_rect(frame, frame_truth, 0.5)), "window.list gave the receiver another frame");
  // Only the receivers are kept; the other entries are the user's windows and are counted, not saved.
  records.write(
    "window.list",
    json!({"exit": list.exit, "ms": list.ms, "windows_listed": entries.len(),
      "receivers_listed": entries.iter().filter(|entry| entry["app_name"] == "win32-receiver.exe").count(),
      "receiver": found, "frame_truth": frame_truth, "invariants": invariants(&listed, hwnd), "verdict": listed.verdict}),
  )?;

  // window.capture: its saved pixels decode to the receiver's own state only when the bounds and
  // the scale place the image on the screen correctly.
  let (captured, _) = desk.measure(|| invoke(&auv, &store, "window.capture", &["--target", "win32-receiver.exe", "--title", title]), || ());
  tally.record(&captured);
  let capture_run = captured.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  let result = capture_run.json.as_ref().map(|json| &json["result"]);
  let bounds = result.and_then(|result| rect_of(&result["capture"]["bounds"]));
  let pixels =
    result.map(|result| (result["capture"]["pixel_dimensions"]["width"].as_u64(), result["capture"]["pixel_dimensions"]["height"].as_u64()));
  let scale = result.and_then(|result| result["capture"]["scale_factor"].as_f64());
  tally.expect(capture_run.completed(), format!("window.capture failed: {}", capture_run.failure()));
  tally.expect(
    result.is_some_and(|result| result["window"]["reference"]["id"].as_str() == Some(hwnd_text.as_str())),
    "window.capture resolved a window other than the receiver",
  );
  tally
    .expect(bounds.is_some_and(|bounds| same_rect(bounds, frame_truth, 0.5)), "window.capture bounds are not the receiver's visible frame");
  tally.expect(
    bounds.zip(pixels).is_some_and(|(bounds, (width, height))| {
      width.is_some_and(|width| (width as f64 - bounds[2]).abs() <= 1.0)
        && height.is_some_and(|height| (height as f64 - bounds[3]).abs() <= 1.0)
    }),
    "window.capture pixels are not the size of its bounds",
  );
  let artifact = capture_run
    .json
    .as_ref()
    .and_then(|json| json["artifacts"].as_array())
    .and_then(|artifacts| artifacts.iter().find_map(|artifact| artifact["file_path"].as_str()))
    .map(PathBuf::from);
  let decoded = match (&artifact, bounds, scale) {
    (Some(path), Some(bounds), Some(scale)) => {
      let image = image::open(path).with_context(|| format!("read {}", path.display()))?.to_rgba8();
      let capture = Capture {
        origin: None,
        image,
        bounds: Rect::new(bounds[0], bounds[1], bounds[2], bounds[3]),
        scale_factor: scale,
        backend: String::new(),
        fallback_reason: None,
      };
      image::DynamicImage::ImageRgba8(capture.image.clone())
        .save(options.output.join("window-capture.png"))
        .context("keep the receiver capture")?;
      Some(decode_strip(&capture, &layout.strip))
    }
    _ => None,
  };
  tally.expect(decoded == Some((3, 0)), format!("the saved capture decodes to {decoded:?}, not the receiver's 3 clicks"));
  records.write(
    "window.capture",
    json!({"exit": capture_run.exit, "ms": capture_run.ms, "bounds": bounds, "frame_truth": frame_truth, "scale_factor": scale,
      "pixel_dimensions": pixels, "decoded_strip": decoded, "wanted_strip": [3, 0],
      "backend": result.map(|result| result["capture"]["backend"].clone()), "artifact_saved": artifact.is_some(),
      "invariants": invariants(&captured, hwnd), "verdict": captured.verdict}),
  )?;

  // window.findText: text is placed against the control that draws it.
  // - "clicks 3" starts at the left edge of its label, near its top, so a capture that is shifted
  //   or stretched moves it out of that corner.
  // - "line 000" is the first line of the edit control, which only has to contain it.
  // - "Press" is the button's caption. The system OCR does not read a caption that short on a
  //   button face, so it is recorded and not asserted.
  let probes = [
    ("clicks 3", "label", layout.label_rect, Placement::Corner),
    ("line 000", "edit", layout.edit_rect, Placement::Inside),
    ("Press", "button", layout.button_rect, Placement::Recorded),
  ];
  for (query, name, rect, placement) in probes {
    let (found_text, _) =
      desk.measure(|| invoke(&auv, &store, "window.findText", &[query, "--target", "win32-receiver.exe", "--title", title]), || ());
    tally.record(&found_text);
    let run = found_text.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
    let matched = first_match(run);
    let from_corner = matched.map(|matched| {
      [
        matched[0] - f64::from(rect[0]),
        matched[1] - f64::from(rect[1]),
      ]
    });
    if placement != Placement::Recorded {
      tally.expect(run.completed(), format!("window.findText {query:?} failed: {}", run.failure()));
      tally.expect(matched.is_some(), format!("window.findText did not find {query:?}"));
    }
    match placement {
      Placement::Corner => tally.expect(
        from_corner.is_some_and(near_corner),
        format!("the {name} text {query:?} is {from_corner:?} px from its control's corner, not within the few pixels its caption sits"),
      ),
      Placement::Inside => tally.expect(
        matched.is_some_and(|matched| inside(matched, rect, TEXT_SLACK)),
        format!("the {name} text {query:?} is placed outside its control"),
      ),
      Placement::Recorded => {}
    }
    records.write(
      "window.findText",
      json!({"query": query, "control": name, "control_rect": rect, "asserted": placement != Placement::Recorded,
        "exit": run.exit, "ms": run.ms, "match": matched, "offset_from_control_corner": from_corner,
        "invariants": invariants(&found_text, hwnd), "verdict": found_text.verdict}),
    )?;
  }

  // window.waitForText: present now, absent for good, and appearing while it waits.
  let (present, _) = desk.measure(
    || {
      invoke(
        &auv,
        &store,
        "window.waitForText",
        &[
          "clicks 3",
          "--target",
          "win32-receiver.exe",
          "--title",
          title,
        ],
      )
    },
    || (),
  );
  tally.record(&present);
  let present_run = present.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  tally.expect(present_run.completed(), format!("window.waitForText failed on text that is there: {}", present_run.failure()));
  records.write(
    "window.waitForText.present",
    json!({"exit": present_run.exit, "ms": present_run.ms, "invariants": invariants(&present, hwnd), "verdict": present.verdict}),
  )?;

  let (absent, _) = desk.measure(
    || {
      invoke(
        &auv,
        &store,
        "window.waitForText",
        &[
          "clicks 99",
          "--target",
          "win32-receiver.exe",
          "--title",
          title,
        ],
      )
    },
    || (),
  );
  tally.record(&absent);
  let absent_run = absent.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  tally.expect(
    absent_run.exit == Some(1) && absent_run.failure().contains("did not find text"),
    format!("window.waitForText on absent text ended {:?}: {}", absent_run.exit, absent_run.failure()),
  );
  tally.expect(absent_run.ms >= 4_500.0, format!("window.waitForText gave up after {:.0} ms, before its timeout", absent_run.ms));
  records.write(
    "window.waitForText.absent",
    json!({"exit": absent_run.exit, "ms": absent_run.ms, "failure": absent_run.failure(), "invariants": invariants(&absent, hwnd), "verdict": absent.verdict}),
  )?;

  // The label reads "clicks 3" until the clicks below arrive, 1.5 s after the command starts.
  let (appearing, _) = desk.measure(
    || -> Result<(Invocation, usize)> {
      std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
          invoke(
            &auv,
            &store,
            "window.waitForText",
            &[
              "clicks 6",
              "--target",
              "win32-receiver.exe",
              "--title",
              title,
            ],
          )
        });
        std::thread::sleep(Duration::from_millis(1_500));
        let sent = (0..3).filter(|_| session.click(id, button, ClickRequest::default()).is_done()).count();
        let run = waiter.join().map_err(|_| anyhow::anyhow!("the waiting command panicked"))??;
        Ok((run, sent))
      })
    },
    || (),
  );
  tally.record(&appearing);
  let (appearing_run, sent) = appearing.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  tally.expect(*sent == 3, format!("{sent} of 3 clicks reached the receiver while the command waited"));
  tally
    .expect(appearing_run.completed(), format!("window.waitForText missed text that appeared while it waited: {}", appearing_run.failure()));
  records.write(
    "window.waitForText.appearing",
    json!({"exit": appearing_run.exit, "ms": appearing_run.ms, "clicks_sent": sent, "invariants": invariants(&appearing, hwnd), "verdict": appearing.verdict}),
  )?;

  // A selector that names nothing is an error, not another window.
  let (missing, _) = desk.measure(|| invoke(&auv, &store, "window.capture", &["--target", "no-such-program.exe", "--title", title]), || ());
  tally.record(&missing);
  let missing_run = missing.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  tally.expect(missing_run.exit == Some(1), "window.capture of an executable with no windows did not fail");
  records.write(
    "window.capture.missing_target",
    json!({"exit": missing_run.exit, "failure": missing_run.failure(), "verdict": missing.verdict}),
  )?;

  // A minimized window is not restored to be read.
  // SAFETY: minimizing a window this harness owns, without activating anything.
  let _ = unsafe { ShowWindow(HWND(hwnd as *mut c_void), SW_SHOWMINNOACTIVE) };
  std::thread::sleep(Duration::from_millis(300));
  let minimized_before = unsafe { IsIconic(HWND(hwnd as *mut c_void)) }.as_bool();
  let (minimized, _) = desk.measure(|| invoke(&auv, &store, "window.capture", &["--target", "win32-receiver.exe", "--title", title]), || ());
  tally.record(&minimized);
  let minimized_run = minimized.value.as_ref().map_err(|error| anyhow::anyhow!("{error:#}"))?;
  let still_minimized = unsafe { IsIconic(HWND(hwnd as *mut c_void)) }.as_bool();
  tally.expect(minimized_before && still_minimized, "a command restored the minimized receiver");
  // A minimized window has nothing to capture: a command that answers with the 146x28 stub at
  // (-32000, -32000) as if it were the window gives the caller a false picture.
  tally.expect(
    minimized_run.exit == Some(1) && minimized_run.failure().contains("minimized"),
    format!("window.capture of a minimized window ended {:?}: {:?}", minimized_run.exit, minimized_run.failure()),
  );
  records.write(
    "window.capture.minimized",
    json!({"minimized_before": minimized_before, "minimized_after": still_minimized, "exit": minimized_run.exit,
      "ms": minimized_run.ms, "failure": minimized_run.failure(),
      "bounds": minimized_run.json.as_ref().and_then(|json| rect_of(&json["result"]["capture"]["bounds"])),
      "invariants": invariants(&minimized, hwnd), "verdict": minimized.verdict}),
  )?;

  // What the commands did to the desktop.
  let finish = watch.snapshot();
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
  let _ = cover.close();
  // The store holds a record of every window on the desktop; none of it is evidence.
  let _ = std::fs::remove_dir_all(&store);
  println!(
    "window: {} commands, {} held, {} inconclusive, {} violated; {}",
    tally.ops,
    tally.held,
    tally.inconclusive,
    tally.violated,
    if passed { "PASS" } else { "FAIL" }
  );
  for failure in &tally.failures {
    println!("  FAIL {failure}");
  }
  println!("records: {}", options.output.join("window.jsonl").display());
  Ok(passed)
}
