//! Live validation harness for the Windows driver capabilities.
//!
//! Each capability is exercised against the real desktop and produces an
//! inspectable artifact (printed report, PNG, or an observable side effect) so
//! a human can confirm it behaves as intended. Unit tests prove the headless
//! logic; this binary proves the platform integration end to end.
//!
//! Usage (from the repo root):
//!
//! ```text
//! cargo run -p auv-driver-windows --example validate -- <command> [args]
//! ```
//!
//! Commands:
//!   displays                       list displays
//!   windows                        list top-level windows (id + title)
//!   permissions                    probe automation readiness (UAC/UIAccess/session)
//!   resolve <title-substr>         resolve a window by title substring
//!   capture-screen [out.png]       capture the primary display to a PNG
//!   capture-window <substr> [out]  capture a window to a PNG
//!   ocr <title-substr>             OCR a window capture and print recognized text
//!   ax <title-substr>              dump a window's UI Automation tree
//!   coords <title-substr>          round-trip a window/screen point mapping
//!   clipboard                      snapshot -> set -> read-back -> restore
//!   type <text>                    type text into the foreground window (3s focus delay)
//!   press <key>                    press a key/shortcut (e.g. "enter", "ctrl+a")
//!   click <x> <y>                  left click at screen coordinates
//!   scroll <x> <y> <delta_y>       scroll at screen coordinates
//!   move <substr> <x> <y>          move a window to a screen position
//!   resize <substr> <w> <h>        resize a window
//!   minimize <substr>              minimize a window
//!   unminimize <substr>            restore a minimized window

use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

use auv_driver_common::Driver;
use auv_driver_common::capture::CaptureOptions;
use auv_driver_common::geometry::{Point, RatioRect, WindowPoint};
use auv_driver_common::input::{Click, KeyPressOptions, Scroll, TypeTextOptions};
use auv_driver_common::window::{Window, WindowMutationOptions};
use auv_driver_windows::{WindowsDriver, WindowsDriverSession};

type Run = Result<(), Box<dyn Error>>;

fn main() {
  if let Err(error) = run() {
    eprintln!("error: {error}");
    std::process::exit(1);
  }
}

fn run() -> Run {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let Some(command) = args.first() else {
    print_usage();
    return Ok(());
  };

  let session = WindowsDriver::new().open_local()?;
  let rest = &args[1..];

  match command.as_str() {
    "displays" => displays(&session),
    "windows" => windows(&session),
    "permissions" => permissions(&session),
    "resolve" => resolve(&session, arg(rest, 0, "title-substr")?),
    "capture-screen" => capture_screen(&session, rest.first().map(String::as_str)),
    "capture-window" => capture_window(&session, arg(rest, 0, "title-substr")?, rest.get(1).map(String::as_str)),
    "ocr" => ocr(&session, arg(rest, 0, "title-substr")?),
    "ax" => ax(&session, arg(rest, 0, "title-substr")?),
    "coords" => coords(&session, arg(rest, 0, "title-substr")?),
    "clipboard" => clipboard(&session),
    "type" => type_text(&session, arg(rest, 0, "text")?),
    "press" => press(&session, arg(rest, 0, "key")?),
    "click" => click(&session, parse(rest, 0)?, parse(rest, 1)?),
    "scroll" => scroll(&session, parse(rest, 0)?, parse(rest, 1)?, parse(rest, 2)?),
    "move" => move_window(&session, arg(rest, 0, "substr")?, parse(rest, 1)?, parse(rest, 2)?),
    "resize" => resize_window(&session, arg(rest, 0, "substr")?, parse(rest, 1)?, parse(rest, 2)?),
    "minimize" => minimize(&session, arg(rest, 0, "substr")?),
    "unminimize" => unminimize(&session, arg(rest, 0, "substr")?),
    other => {
      eprintln!("unknown command: {other}\n");
      print_usage();
      Ok(())
    }
  }
}

fn displays(session: &WindowsDriverSession) -> Run {
  let observed = session.display().list()?;
  println!("displays: {}", observed.displays.len());
  for display in &observed.displays {
    println!(
      "  id={} name={:?} primary={} scale={} frame={:?}",
      display.id, display.name, display.is_primary, display.scale_factor, display.frame
    );
  }
  Ok(())
}

fn windows(session: &WindowsDriverSession) -> Run {
  let listed = session.window().list()?;
  println!("windows: {}", listed.len());
  for window in &listed {
    println!(
      "  id={} title={:?} app={:?} main={} visible={} frame={:?}",
      window.reference.id, window.title, window.app_name, window.is_main, window.is_visible, window.frame
    );
  }
  Ok(())
}

fn permissions(session: &WindowsDriverSession) -> Run {
  let probe = session.permission().probe();
  println!("automation readiness:");
  println!("  elevated            = {:?}", probe.elevated);
  println!("  ui_access           = {:?}", probe.ui_access);
  println!("  interactive_session = {:?}", probe.interactive_session);
  Ok(())
}

fn resolve(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  println!("resolved: id={} title={:?} frame={:?}", window.reference.id, window.title, window.frame);
  Ok(())
}

fn capture_screen(session: &WindowsDriverSession, out: Option<&str>) -> Run {
  let out = out.unwrap_or("validate-screen.png");
  let captured = session.display().capture(CaptureOptions::default())?;
  captured.capture.image.save(out)?;
  println!(
    "captured display {}x{} via {} -> {out}",
    captured.capture.image.width(),
    captured.capture.image.height(),
    captured.capture.backend
  );
  Ok(())
}

fn capture_window(session: &WindowsDriverSession, substr: &str, out: Option<&str>) -> Run {
  let window = find_window(session, substr)?;
  let out = out.unwrap_or("validate-window.png");
  let captured = session.window().capture(&window)?;
  captured.image.save(out)?;
  println!("captured window {:?} {}x{} via {} -> {out}", window.title, captured.image.width(), captured.image.height(), captured.backend);
  Ok(())
}

fn ocr(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let captured = session.window().capture(&window)?;
  let full = RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let recognition = session.vision().recognize_text_in_capture(&captured, full)?;
  println!("recognized {} regions:", recognition.regions.len());
  for region in &recognition.regions {
    println!("  {:?} @ {:?}", region.text, region.bounds);
  }
  Ok(())
}

fn ax(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let snapshot = session.accessibility().snapshot_window(&window)?;
  println!("ax tree for window {} -> {} nodes", snapshot.window_ref, snapshot.nodes.len());
  for node in &snapshot.nodes {
    let indent = "  ".repeat(node.depth + 1);
    println!(
      "{indent}[{}] {} name={:?} id={:?} class={:?} focused={} bounds={:?}",
      node.path, node.control_type, node.name, node.automation_id, node.class_name, node.focused, node.bounds
    );
  }
  Ok(())
}

fn coords(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let local = WindowPoint::new(10.0, 20.0);
  let screen = session.window().to_screen_point(&window, local)?;
  let back = session.window().to_window_point(&window, screen)?;
  println!("window frame origin = {:?}", window.frame.origin);
  println!("window {:?} -> screen {:?} -> window {:?}", local, screen, back);
  Ok(())
}

fn clipboard(session: &WindowsDriverSession) -> Run {
  let clipboard = session.clipboard();
  let original = clipboard.snapshot()?;
  println!("original clipboard = {original:?}");

  let probe = "auv-validate-clipboard";
  clipboard.set_text(probe)?;
  let read_back = clipboard.snapshot()?;
  println!("after set_text     = {read_back:?}");
  assert_eq!(read_back, probe, "clipboard set/read mismatch");

  clipboard.restore(&original)?;
  let restored = clipboard.snapshot()?;
  println!("after restore      = {restored:?}");
  assert_eq!(restored, original, "clipboard restore mismatch");
  println!("clipboard roundtrip OK");
  Ok(())
}

fn type_text(session: &WindowsDriverSession, text: &str) -> Run {
  focus_countdown("type into");
  session.input().type_text(text, TypeTextOptions::default())?;
  println!("typed {text:?}");
  Ok(())
}

fn press(session: &WindowsDriverSession, key: &str) -> Run {
  focus_countdown("send the key to");
  let options = KeyPressOptions {
    key: key.to_string(),
    settle: Duration::ZERO,
  };
  session.input().press_key(options)?;
  println!("pressed {key:?}");
  Ok(())
}

fn click(session: &WindowsDriverSession, x: f64, y: f64) -> Run {
  session.input().click_at(Point::new(x, y), Click::Single)?;
  println!("clicked at ({x}, {y})");
  Ok(())
}

fn scroll(session: &WindowsDriverSession, x: f64, y: f64, delta_y: f64) -> Run {
  session.input().scroll_at(Point::new(x, y), Scroll::new(0.0, delta_y), Duration::ZERO)?;
  println!("scrolled {delta_y} at ({x}, {y})");
  Ok(())
}

fn move_window(session: &WindowsDriverSession, substr: &str, x: f64, y: f64) -> Run {
  let window = find_window(session, substr)?;
  let result = session.window().move_to(&window, Point::new(x, y), WindowMutationOptions::default())?;
  println!("moved {:?} -> ({x}, {y}); result {:?}", window.title, result);
  Ok(())
}

fn resize_window(session: &WindowsDriverSession, substr: &str, w: f64, h: f64) -> Run {
  let window = find_window(session, substr)?;
  let result = session.window().resize(&window, auv_driver_common::geometry::Size::new(w, h), WindowMutationOptions::default())?;
  println!("resized {:?} -> {w}x{h}; result {:?}", window.title, result);
  Ok(())
}

fn minimize(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let result = session.window().minimize(&window, WindowMutationOptions::default())?;
  println!("minimized {:?}; result {:?}", window.title, result);
  Ok(())
}

fn unminimize(session: &WindowsDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let result = session.window().restore(&window, WindowMutationOptions::default())?;
  println!("restored {:?}; result {:?}", window.title, result);
  Ok(())
}

/// Finds the first visible window whose title contains `substr`
/// (case-insensitive), printing available titles when nothing matches so the
/// operator can pick a valid target.
fn find_window(session: &WindowsDriverSession, substr: &str) -> Result<Window, Box<dyn Error>> {
  let needle = substr.to_lowercase();
  let listed = session.window().list()?;
  let found = listed.iter().find(|window| window.title.as_deref().is_some_and(|title| title.to_lowercase().contains(&needle)));
  match found {
    Some(window) => Ok(window.clone()),
    None => {
      eprintln!("no window title contains {substr:?}. Available titles:");
      for window in &listed {
        if let Some(title) = &window.title {
          eprintln!("  {title}");
        }
      }
      Err(format!("window not found: {substr}").into())
    }
  }
}

/// Brief delay so the operator can focus the intended target window before a
/// foreground input event is delivered.
fn focus_countdown(action: &str) {
  for remaining in (1..=3).rev() {
    println!("focus the window to {action} ... {remaining}");
    sleep(Duration::from_secs(1));
  }
}

fn arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, Box<dyn Error>> {
  args.get(index).map(String::as_str).ok_or_else(|| format!("missing argument: <{name}>").into())
}

fn parse(args: &[String], index: usize) -> Result<f64, Box<dyn Error>> {
  let raw = args.get(index).ok_or_else(|| format!("missing numeric argument at position {index}"))?;
  raw.parse::<f64>().map_err(|error| format!("invalid number {raw:?}: {error}").into())
}

fn print_usage() {
  println!(
    "usage: cargo run -p auv-driver-windows --example validate -- <command> [args]\n\
     commands: displays | windows | permissions | resolve <substr> |\n\
     \tcapture-screen [out.png] | capture-window <substr> [out.png] |\n\
     \tocr <substr> | ax <substr> | coords <substr> | clipboard |\n\
     \ttype <text> | press <key> | click <x> <y> | scroll <x> <y> <dy> |\n\
     \tmove <substr> <x> <y> | resize <substr> <w> <h> |\n\
     \tminimize <substr> | unminimize <substr>"
  );
}
