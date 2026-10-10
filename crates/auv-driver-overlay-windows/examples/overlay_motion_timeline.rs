//! Development harness for the overlay animation system. Not a product feature.
//!
//! Plays a scripted timeline of reported actions through the real animator over a backdrop
//! this example owns, samples the composited screen while it runs, and checks what actually
//! reached the screen:
//!
//! ```text
//! cargo run --release -p auv-driver-overlay-windows --example overlay_motion_timeline -- [out-dir]
//! ```
//!
//! The product feeds the animator from real driver input (see
//! `auv_driver_windows::OverlayApi::follow_operations`); this script only stands in for that
//! stream so the animation can be tested without touching anything. It sends no input, takes
//! no focus and restores no window. Pass 1 measures frame time and event latency with nothing
//! else competing for the CPU; pass 2 captures the screen at every step and finds the cursor
//! tip in each capture (the bright cyan only the built-in pointer's body uses). Pass 3 acts in
//! three windows in turn, with a status for each, and checks that every window's cursor and
//! status pill reach the screen in that window's own color.
//!
//! Exits non-zero when a check fails.

#[cfg(target_os = "windows")]
#[path = "support/backdrop.rs"]
mod backdrop;

#[cfg(target_os = "windows")]
fn main() {
  let out_dir = std::env::args().nth(1).unwrap_or_else(|| "overlay-motion".to_string());
  if let Err(error) = harness::run(std::path::Path::new(&out_dir)) {
    eprintln!("overlay_motion_timeline failed: {error}");
    std::process::exit(1);
  }
}

#[cfg(not(target_os = "windows"))]
fn main() {
  eprintln!("overlay_motion_timeline only runs on Windows");
}

#[cfg(target_os = "windows")]
mod harness {
  use std::path::Path;
  use std::time::{Duration, Instant};

  use auv_driver_common::{MouseButton, Rect, ScreenPoint};
  use auv_driver_overlay_common::{ActionEvent, FrameStats, LifecycleOptions, Travel};
  use auv_driver_overlay_windows::Animator;

  use super::backdrop::{Backdrop, count_near, pump, write_bmp};

  const LEFT: i32 = 200;
  const TOP: i32 = 200;
  const WIDTH: i32 = 900;
  const HEIGHT: i32 = 420;

  /// Whether a captured BGRX pixel is the cyan pointer's body: its saturated cyan (`#2fd3df`
  /// fading to `#0794a6`), or the pale cyan of the pressed art (`#8de7ed`).
  ///
  /// No other layer in passes 1 and 2 has those colors. The script's screen cursor acts
  /// first, so it takes cyan, and the two marked windows take the next colors, pink and
  /// violet. The right-click ripple uses the deeper `#009ba6`, which is too dark on its own;
  /// blended into the light backdrop by antialiasing it gets brighter but also grayer, never
  /// as saturated (green over red by 130) or as pale-but-blue (red under 160 with green over
  /// 225) as the pointer. The left-click ripple is lime and the backdrop is gray. So the
  /// pointer can be found in a capture without knowing where the model put it.
  fn is_pointer_body(pixel: &[u8]) -> bool {
    let (blue, green, red) = (i32::from(pixel[0]), i32::from(pixel[1]), i32::from(pixel[2]));
    let body = green >= 180 && blue >= 190 && green - red >= 130;
    let pressed = green >= 225 && blue >= 235 && red <= 160;
    body || pressed
  }

  fn at(x: f64, y: f64) -> ScreenPoint {
    ScreenPoint::new(f64::from(LEFT) + x, f64::from(TOP) + y)
  }

  struct Step {
    at: Duration,
    event: ActionEvent,
  }

  fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
  }

  /// A scripted operation sequence: two windows targeted, two clicks (the second across the
  /// whole backdrop), then a sampled drag and a right click.
  fn timeline() -> Vec<Step> {
    let mut steps = vec![
      Step {
        at: ms(0),
        event: ActionEvent::Moved {
          point: at(60.0, 80.0),
          travel: Travel::Jump,
          window: None,
        },
      },
      Step {
        at: ms(100),
        event: ActionEvent::WindowTargeted {
          id: "window-a".into(),
          frame: Rect::new(f64::from(LEFT) + 30.0, f64::from(TOP) + 30.0, 330.0, 240.0),
          label: Some("window A".into()),
        },
      },
      Step {
        at: ms(150),
        event: ActionEvent::WindowTargeted {
          id: "window-b".into(),
          frame: Rect::new(f64::from(LEFT) + 470.0, f64::from(TOP) + 110.0, 400.0, 270.0),
          label: Some("window B".into()),
        },
      },
      Step {
        at: ms(400),
        event: ActionEvent::Clicked {
          point: at(150.0, 160.0),
          button: MouseButton::Left,
          window: None,
        },
      },
      Step {
        at: ms(1000),
        event: ActionEvent::Clicked {
          point: at(760.0, 250.0),
          button: MouseButton::Left,
          window: None,
        },
      },
      Step {
        at: ms(1600),
        event: ActionEvent::Moved {
          point: at(120.0, 360.0),
          travel: Travel::Jump,
          window: None,
        },
      },
    ];
    // A timed trajectory at 125 Hz: the driver already played this path, so the cursor must
    // follow it point for point.
    for tick in 0..=60u64 {
      let progress = tick as f64 / 60.0;
      steps.push(Step {
        at: ms(1700 + tick * 8),
        event: ActionEvent::Moved {
          point: at(120.0 + 640.0 * progress, 360.0 - 90.0 * progress),
          travel: Travel::Sampled,
          window: None,
        },
      });
    }
    steps.push(Step {
      at: ms(2300),
      event: ActionEvent::Clicked {
        point: at(760.0, 270.0),
        button: MouseButton::Right,
        window: None,
      },
    });
    steps
  }

  const END: Duration = Duration::from_millis(3100);

  /// One cue of the per-window script: a reported action, or a caller's status.
  enum Cue {
    Event(ActionEvent),
    Status {
      window: &'static str,
      text: &'static str,
    },
  }

  /// A window of the per-window script, with the color its cursor takes (the order the
  /// windows first appear: AUV cyan, then pink, then violet) and where it acts last.
  struct Actor {
    id: &'static str,
    fill: [u8; 3],
    last_point: (f64, f64),
  }

  const ACTORS: [Actor; 3] = [
    Actor {
      id: "notes",
      fill: [0x2f, 0xd3, 0xdf],
      last_point: (200.0, 260.0),
    },
    Actor {
      id: "browser",
      fill: [0xff, 0x74, 0xb1],
      last_point: (560.0, 100.0),
    },
    Actor {
      id: "repl",
      fill: [0x9b, 0x7d, 0xff],
      last_point: (560.0, 300.0),
    },
  ];

  fn marked(id: &str, x: f64, y: f64, width: f64, height: f64, label: &str) -> Cue {
    Cue::Event(ActionEvent::WindowTargeted {
      id: id.into(),
      frame: Rect::new(f64::from(LEFT) + x, f64::from(TOP) + y, width, height),
      label: Some(label.into()),
    })
  }

  fn clicked_in(id: &str, x: f64, y: f64) -> Cue {
    Cue::Event(ActionEvent::Clicked {
      point: at(x, y),
      button: MouseButton::Left,
      window: Some(id.into()),
    })
  }

  /// Three windows in turn, like an agent working across apps: each click is aimed at its
  /// window, and the caller says what it is doing there. Notes sits on the light half of
  /// the backdrop, the browser and the REPL on the dark half.
  fn per_window_script() -> Vec<(Duration, Cue)> {
    vec![
      (ms(0), marked("notes", 30.0, 30.0, 380.0, 360.0, "Notes")),
      (
        ms(0),
        Cue::Status {
          window: "notes",
          text: "Clicking “New”",
        },
      ),
      (ms(0), clicked_in("notes", 140.0, 120.0)),
      (ms(800), marked("browser", 470.0, 30.0, 400.0, 170.0, "Browser")),
      (
        ms(800),
        Cue::Status {
          window: "browser",
          text: "Reading the page",
        },
      ),
      (ms(800), clicked_in("browser", 560.0, 100.0)),
      (ms(1600), marked("repl", 470.0, 230.0, 400.0, 160.0, "REPL")),
      (
        ms(1600),
        Cue::Status {
          window: "repl",
          text: "Recording the run",
        },
      ),
      (ms(1600), clicked_in("repl", 560.0, 300.0)),
      (
        ms(2200),
        Cue::Status {
          window: "notes",
          text: "Wrapping up",
        },
      ),
      (
        ms(2200),
        Cue::Event(ActionEvent::Moved {
          point: at(200.0, 260.0),
          travel: Travel::Jump,
          window: Some("notes".into()),
        }),
      ),
      (
        ms(2500),
        Cue::Status {
          window: "browser",
          text: "Almost done",
        },
      ),
    ]
  }

  const PER_WINDOW_END: Duration = Duration::from_millis(3200);
  /// Captures written for the evidence image: the first cursor typing its status, the
  /// second arriving from the first, and all three settled with their statuses.
  const PER_WINDOW_SNAPSHOTS: [u64; 3] = [700, 1500, 3100];

  /// Pass 3: plays the per-window script and returns the captures taken at
  /// `PER_WINDOW_SNAPSHOTS`.
  fn observe_windows(backdrop: &Backdrop) -> Result<Vec<Snapshot>, String> {
    let animator = Animator::start(options())?;
    let script = per_window_script();
    let started = Instant::now();
    let mut next = 0;
    let mut snapshots = Vec::new();
    let mut wanted = PER_WINDOW_SNAPSHOTS.iter().copied().peekable();
    while started.elapsed() < PER_WINDOW_END {
      while next < script.len() && started.elapsed() >= script[next].0 {
        match &script[next].1 {
          Cue::Event(event) => animator.report(event.clone())?,
          Cue::Status { window, text } => animator.set_status(Some(*window), Some(*text))?,
        }
        next += 1;
      }
      if wanted.peek().is_some_and(|due| started.elapsed() >= ms(*due)) {
        snapshots.push(Snapshot {
          at_ms: wanted.next().unwrap(),
          pixels: backdrop.capture()?,
        });
      }
      std::thread::sleep(Duration::from_millis(1));
    }
    animator.stop()?;
    Ok(snapshots)
  }

  /// Each window's cursor sits on its last point in that window's color, with a status pill
  /// of the same color beside it, and shows no other window's color.
  fn check_windows(report: &mut Report, last: &Snapshot) {
    for actor in &ACTORS {
      let (x, y) = (actor.last_point.0 as i32, actor.last_point.1 as i32);
      // The 24 px pointer box hangs below and right of its tip; its pill starts 29 px right.
      let sprite = ((x - 1, x + 19), (y - 1, y + 23));
      let pill = ((x + 28, x + 118), (y - 6, y + 30));
      let body = count_near(&last.pixels, WIDTH, sprite.0, sprite.1, actor.fill, 60.0);
      let pill_fill = count_near(&last.pixels, WIDTH, pill.0, pill.1, actor.fill, 24.0);
      let foreign = ACTORS
        .iter()
        .filter(|other| other.id != actor.id)
        .map(|other| count_near(&last.pixels, WIDTH, sprite.0, sprite.1, other.fill, 60.0))
        .sum::<usize>();
      println!("  {}: {body} body pixels and {pill_fill} pill pixels in its color, {foreign} in another's", actor.id);
      report.check(body >= 12, format!("{}: its cursor is drawn in its own color on its last point ({body} pixels)", actor.id));
      report.check(pill_fill >= 400, format!("{}: its status pill is drawn beside it in its color ({pill_fill} pixels)", actor.id));
      report.check(foreign < 5, format!("{}: its cursor shows no other window's color ({foreign} pixels)", actor.id));
    }
  }

  fn options() -> LifecycleOptions {
    LifecycleOptions::manual()
  }

  /// Pass 1: nothing but the animator and the script, so the numbers are the animator's own.
  fn measure() -> Result<FrameStats, String> {
    let animator = Animator::start(options())?;
    let steps = timeline();
    let started = Instant::now();
    let mut next = 0;
    while started.elapsed() < END {
      while next < steps.len() && started.elapsed() >= steps[next].at {
        animator.report(steps[next].event.clone())?;
        next += 1;
      }
      std::thread::sleep(Duration::from_millis(1));
    }
    animator.stop()
  }

  /// Where the pointer tip is in a capture: the top-most, then left-most pixel of the
  /// pointer's bright body. The body starts about two pixels inside the white rim, so this
  /// is within about two pixels of the hotspot, tilted or not (the tip stays the top of the
  /// pointer at any tilt the scene uses).
  fn find_tip(pixels: &[u8]) -> Option<(i32, i32)> {
    for y in 0..HEIGHT {
      for x in 0..WIDTH {
        let index = ((y * WIDTH + x) * 4) as usize;
        if is_pointer_body(&pixels[index..index + 3]) {
          return Some((x, y));
        }
      }
    }
    None
  }

  struct Sample {
    at: Duration,
    tip: Option<(i32, i32)>,
  }

  /// A full capture kept for the evidence image.
  struct Snapshot {
    at_ms: u64,
    pixels: Vec<u8>,
  }

  struct Observed {
    samples: Vec<Sample>,
    snapshots: Vec<Snapshot>,
  }

  /// Times at which a full capture is written for the evidence image.
  const SNAPSHOTS: [u64; 8] = [250, 520, 600, 760, 1140, 1260, 1900, 2330];

  /// Pass 2: capture the screen continuously and keep the tip position of every capture.
  fn observe(backdrop: &Backdrop) -> Result<Observed, String> {
    let animator = Animator::start(options())?;
    let steps = timeline();
    let started = Instant::now();
    let mut next = 0;
    let mut samples = Vec::new();
    let mut snapshots = Vec::new();
    let mut wanted = SNAPSHOTS.iter().copied().peekable();
    while started.elapsed() < END {
      while next < steps.len() && started.elapsed() >= steps[next].at {
        animator.report(steps[next].event.clone())?;
        next += 1;
      }
      let taken = started.elapsed();
      let pixels = backdrop.capture()?;
      samples.push(Sample {
        at: taken,
        tip: find_tip(&pixels),
      });
      if wanted.peek().is_some_and(|due| taken >= ms(*due)) {
        snapshots.push(Snapshot {
          at_ms: wanted.next().unwrap(),
          pixels,
        });
      }
    }
    animator.stop()?;
    Ok(Observed { samples, snapshots })
  }

  fn distance(a: (i32, i32), b: (i32, i32)) -> f64 {
    f64::from(a.0 - b.0).hypot(f64::from(a.1 - b.1))
  }

  /// Tip positions observed while `from_ms..to_ms`, in order.
  fn window(samples: &[Sample], from_ms: u64, to_ms: u64) -> Vec<(Duration, (i32, i32))> {
    samples.iter().filter(|s| s.at >= ms(from_ms) && s.at < ms(to_ms)).filter_map(|s| s.tip.map(|tip| (s.at, tip))).collect()
  }

  struct Report {
    failures: Vec<String>,
  }

  impl Report {
    fn check(&mut self, ok: bool, message: String) {
      println!("  {} {message}", if ok { "PASS" } else { "FAIL" });
      if !ok {
        self.failures.push(message);
      }
    }
  }

  /// Checks one jump from rest: the pointer leaves `from`, passes through intermediate places
  /// on the way and settles on `to`, never jumping most of the distance between two captures.
  fn check_glide(report: &mut Report, samples: &[Sample], name: &str, from_ms: u64, to_ms: u64, from: (i32, i32), to: (i32, i32)) {
    let seen = window(samples, from_ms, to_ms);
    let total = distance(from, to);
    let between = seen.iter().filter(|(_, tip)| distance(*tip, from) > 6.0 && distance(*tip, to) > 6.0).count();
    let worst = seen.windows(2).map(|pair| distance(pair[0].1, pair[1].1)).fold(0.0, f64::max);
    let overshoot = seen.iter().map(|(_, tip)| (distance(*tip, from) + distance(*tip, to) - total).max(0.0)).fold(0.0, f64::max);
    println!("  {name}: {} captures, {between} between the endpoints, largest step {worst:.0} px of {total:.0} px", seen.len());
    report.check(between >= 4, format!("{name}: at least 4 captures between the endpoints, saw {between}"));
    report.check(worst < total * 0.6, format!("{name}: no step over 60% of the distance, largest was {worst:.0} of {total:.0} px"));
    report.check(overshoot < 8.0, format!("{name}: stays on the path between the endpoints, off by at most {overshoot:.1} px"));
    let last = seen.last().map(|(_, tip)| *tip);
    report.check(last.is_some_and(|tip| distance(tip, to) < 6.0), format!("{name}: settles on the reported point, last seen {last:?}"));
  }

  pub(super) fn run(out_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out_dir).map_err(|error| format!("failed to create {}: {error}", out_dir.display()))?;
    let mut report = Report {
      failures: Vec::new(),
    };

    println!("pass 1: animator alone, {} ms script", END.as_millis());
    let stats = measure()?;
    println!("  frames {}, late {}, present failures {}", stats.frames, stats.late_frames, stats.present_failures);
    println!(
      "  frame time    P50 {:.2} ms  P95 {:.2} ms  max {:.2} ms  (n={})",
      stats.frame_ms.p50, stats.frame_ms.p95, stats.frame_ms.max, stats.frame_ms.count
    );
    println!(
      "  event latency P50 {:.2} ms  P95 {:.2} ms  max {:.2} ms  (n={})",
      stats.event_latency_ms.p50, stats.event_latency_ms.p95, stats.event_latency_ms.max, stats.event_latency_ms.count
    );
    report.check(stats.present_failures == 0, format!("no present failures ({:?})", stats.last_failure));
    report.check(stats.frame_ms.p95 < 16.6, format!("P95 frame time {:.2} ms is inside the 16.6 ms budget", stats.frame_ms.p95));

    println!("pass 2: screen captures");
    let backdrop = Backdrop::show(LEFT, TOP, WIDTH, HEIGHT)?;
    let observed = observe(&backdrop);
    let _ = auv_driver_overlay_windows::remove();
    pump(Duration::from_millis(100));
    let per_window = observed.as_ref().ok().map(|_| observe_windows(&backdrop));
    let _ = auv_driver_overlay_windows::remove();
    pump(Duration::from_millis(100));
    backdrop.close();
    let Observed { samples, snapshots } = observed?;
    let per_window = per_window.expect("pass 3 runs after pass 2 succeeded")?;

    let found = samples.iter().filter(|s| s.tip.is_some()).count();
    println!("  {} captures, pointer found in {found}", samples.len());
    report.check(found * 10 >= samples.len() * 8, format!("pointer visible in at least 80% of captures ({found}/{})", samples.len()));

    let tip = |x: f64, y: f64| (x as i32, y as i32);
    // The jump from the first click's point to the second click's point is the longest move
    // on the script (617 px). The pointer is at rest when it starts, so its path is straight.
    check_glide(&mut report, &samples, "click to click glide", 1000, 1450, tip(150.0, 160.0), tip(760.0, 250.0));

    // The jump at 1600 ms blends into the live stream and has caught up well before 1950 ms;
    // from then on the pointer must be where the driver's samples are, within the pipeline's
    // own delay (up to one frame of pacing, one frame of rendering and one compositor tick,
    // about 50 ms) plus the 25 ms the stream's spring trails it by: 75 ms in all. The stream
    // moves 1.33 px per millisecond, so a jump-length spring per sample (a cursor that falls
    // behind by the 110 ms smooth time and more) would fail this.
    let sampled = window(&samples, 1950, 2150);
    let worst_lag = sampled
      .iter()
      .map(|(at, tip)| {
        let progress = ((at.as_secs_f64() * 1000.0 - 1700.0) / 480.0).clamp(0.0, 1.0);
        let expected = (120.0 + 640.0 * progress, 360.0 - 90.0 * progress);
        f64::from(tip.0 as f32 - expected.0 as f32).abs().max(f64::from(tip.1 as f32 - expected.1 as f32).abs())
      })
      .fold(0.0, f64::max);
    let lag_ms = worst_lag / (640.0 / 480.0);
    println!(
      "  sampled stream: {} captures during the drag, furthest from the driver's sample {worst_lag:.0} px (about {lag_ms:.0} ms)",
      sampled.len()
    );
    report.check(
      !sampled.is_empty() && lag_ms < 75.0,
      format!("sampled stream: the pointer tracks the driver's samples within 75 ms ({lag_ms:.0} ms)"),
    );

    println!("pass 3: one cursor per window, with statuses");
    match per_window.last() {
      Some(last) if last.at_ms == PER_WINDOW_SNAPSHOTS[PER_WINDOW_SNAPSHOTS.len() - 1] => check_windows(&mut report, last),
      _ => report.check(false, "pass 3: the final capture was taken".to_string()),
    }

    for snapshot in &snapshots {
      write_bmp(&out_dir.join(format!("frame-{:04}ms.bmp", snapshot.at_ms)), WIDTH, HEIGHT, &snapshot.pixels)?;
    }
    for snapshot in &per_window {
      write_bmp(&out_dir.join(format!("windows-{:04}ms.bmp", snapshot.at_ms)), WIDTH, HEIGHT, &snapshot.pixels)?;
    }
    println!("saved {} snapshots to {}", snapshots.len() + per_window.len(), out_dir.display());

    if report.failures.is_empty() {
      println!("all checks passed");
      Ok(())
    } else {
      Err(format!("{} check(s) failed", report.failures.len()))
    }
  }
}
