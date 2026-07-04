//! Live validation harness for the Linux driver capabilities.
//!
//! Usage:
//!
//! ```text
//! cargo run -p auv-driver-linux --example validate -- <command> [args] [<command> [args] ...]
//! ```
//!
//! Commands:
//!   permissions                    probe Wayland/XDG portal readiness
//!   displays                       list Wayland xdg-output-visible displays
//!   windows                        list AT-SPI-visible windows
//!   resolve <title-substr>         resolve a window by title substring
//!   capture-screen [out.png]       capture the primary display to a PNG
//!   capture-region <x> <y> <w> <h> [out.png]
//!   capture-window <substr> [out]  capture a window to a PNG
//!   ocr <title-substr>             OCR a window capture and print recognized text
//!   ax <title-substr>              capture a window accessibility tree
//!   coords <title-substr>          round-trip a window/screen point mapping
//!   clipboard                      snapshot -> set -> read-back -> restore
//!   type-text <text>               type text through RemoteDesktop portal
//!   press <key>                    press a key or shortcut through the portal
//!   scroll <x> <y> <delta-y>       scroll through the portal
//!   click <x> <y>                  click through the portal
//!   input-boundary                 print current RemoteDesktop/libei boundary
//!
//! Multiple commands run in one LinuxDriverSession. Use `--` between commands
//! when an optional argument would otherwise be ambiguous:
//!
//! ```text
//! cargo run -p auv-driver-linux --example validate -- permissions clipboard type-text "hello"
//! cargo run -p auv-driver-linux --example validate -- capture-screen -- clipboard
//! ```

use std::error::Error;

use auv_driver::Driver;
use auv_driver::capture::CaptureOptions;
use auv_driver::geometry::{Point, RatioRect, Rect, WindowPoint};
use auv_driver::input::{Click, KeyPressOptions, Scroll, TypeTextOptions};
use auv_driver::selector::{AppSelector, TextMatcher, Window as SelectWindow, WindowSelector};
use auv_driver::window::Window;
use auv_driver_linux::{LinuxDriver, LinuxDriverSession};

type Run = Result<(), Box<dyn Error>>;

#[derive(Debug, PartialEq, Eq)]
struct Invocation {
  command: String,
  args: Vec<String>,
}

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
  if command == "--" {
    return Err("expected command before separator".into());
  }
  let invocations = parse_invocations(&args)?;
  let session = LinuxDriver::new().open_local()?;

  for (index, invocation) in invocations.iter().enumerate() {
    if invocations.len() > 1 {
      println!(
        "\n== validate {} / {}: {} ==",
        index + 1,
        invocations.len(),
        invocation.command
      );
    }
    run_invocation(&session, invocation)?;
  }

  Ok(())
}

fn run_invocation(session: &LinuxDriverSession, invocation: &Invocation) -> Run {
  let rest = &invocation.args;
  match invocation.command.as_str() {
    "permissions" => permissions(session),
    "displays" => displays(session),
    "windows" => windows(session),
    "resolve" => resolve(session, arg(rest, 0, "title-substr")?),
    "capture-screen" => capture_screen(session, rest.first().map(String::as_str)),
    "capture-region" => capture_region(
      session,
      parse(rest, 0)?,
      parse(rest, 1)?,
      parse(rest, 2)?,
      parse(rest, 3)?,
      rest.get(4).map(String::as_str),
    ),
    "capture-window" => capture_window(
      session,
      arg(rest, 0, "title-substr")?,
      rest.get(1).map(String::as_str),
    ),
    "ocr" => ocr(session, arg(rest, 0, "title-substr")?),
    "ax" => ax(session, arg(rest, 0, "title-substr")?),
    "coords" => coords(session, arg(rest, 0, "title-substr")?),
    "clipboard" => clipboard(session),
    "type-text" => type_text(session, arg(rest, 0, "text")?),
    "press" => press(session, arg(rest, 0, "key")?),
    "scroll" => scroll(session, parse(rest, 0)?, parse(rest, 1)?, parse(rest, 2)?),
    "click" => click(session, parse(rest, 0)?, parse(rest, 1)?),
    "input-boundary" => input_boundary(session),
    other => {
      eprintln!("unknown command: {other}\n");
      print_usage();
      Ok(())
    }
  }
}

fn parse_invocations(args: &[String]) -> Result<Vec<Invocation>, Box<dyn Error>> {
  let mut invocations = Vec::new();
  let mut index = 0;

  while index < args.len() {
    if args[index] == "--" {
      return Err("expected command after separator".into());
    }

    let command = args[index].clone();
    let Some((min_args, max_args)) = command_arity(&command) else {
      return Err(format!("unknown command: {command}").into());
    };
    index += 1;

    let mut command_args = Vec::new();
    while index < args.len() && command_args.len() < max_args {
      if args[index] == "--" {
        index += 1;
        break;
      }
      if command_args.len() >= min_args && command_arity(&args[index]).is_some() {
        break;
      }
      command_args.push(args[index].clone());
      index += 1;
    }

    if command_args.len() < min_args {
      return Err(
        format!(
          "{} expects at least {} argument(s), got {}",
          command,
          min_args,
          command_args.len()
        )
        .into(),
      );
    }

    invocations.push(Invocation {
      command,
      args: command_args,
    });
  }

  Ok(invocations)
}

fn command_arity(command: &str) -> Option<(usize, usize)> {
  match command {
    "permissions" | "displays" | "windows" | "clipboard" | "input-boundary" => Some((0, 0)),
    "capture-screen" => Some((0, 1)),
    "resolve" | "ocr" | "ax" | "coords" | "type-text" | "press" => Some((1, 1)),
    "capture-window" => Some((1, 2)),
    "click" => Some((2, 2)),
    "scroll" => Some((3, 3)),
    "capture-region" => Some((4, 5)),
    _ => None,
  }
}

fn permissions(session: &LinuxDriverSession) -> Run {
  let probe = session.permission().probe_linux();
  println!("linux desktop readiness:");
  println!(
    "  wayland_session = {:?} session_type={:?} desktop={:?}",
    probe.wayland_session, probe.session_type, probe.desktop
  );
  println!("  portal_bus      = {:?}", probe.portal_bus);
  println!(
    "  screencast      = {:?} version={:?} details={:?}",
    probe.screencast.available, probe.screencast.version, probe.screencast.details
  );
  println!(
    "  remote_desktop  = {:?} version={:?} details={:?}",
    probe.remote_desktop.available, probe.remote_desktop.version, probe.remote_desktop.details
  );
  println!(
    "  screenshot      = {:?} version={:?} details={:?}",
    probe.screenshot.available, probe.screenshot.version, probe.screenshot.details
  );
  println!(
    "shared permission projection: {:?}",
    session.permission().probe()
  );
  Ok(())
}

fn displays(session: &LinuxDriverSession) -> Run {
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

fn windows(session: &LinuxDriverSession) -> Run {
  let listed = session.window().list()?;
  println!("windows: {}", listed.len());
  for window in &listed {
    println!(
      "  id={} title={:?} app={:?} pid={:?} main={} visible={} frame={:?}",
      window.reference.id,
      window.title,
      window.app_name,
      window.process_id,
      window.is_main,
      window.is_visible,
      window.frame
    );
  }
  Ok(())
}

fn resolve(session: &LinuxDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  println!(
    "resolved: id={} title={:?} app={:?} frame={:?}",
    window.reference.id, window.title, window.app_name, window.frame
  );
  Ok(())
}

fn capture_screen(session: &LinuxDriverSession, out: Option<&str>) -> Run {
  let out = out.unwrap_or("validate-linux-screen.png");
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

fn capture_region(
  session: &LinuxDriverSession,
  x: f64,
  y: f64,
  width: f64,
  height: f64,
  out: Option<&str>,
) -> Run {
  let out = out.unwrap_or("validate-linux-region.png");
  let captured = session.display().capture_region(CaptureOptions {
    region: Some(Rect::new(x, y, width, height)),
    ..CaptureOptions::default()
  })?;
  captured.capture.image.save(out)?;
  println!(
    "captured region {}x{} via {} -> {out}",
    captured.capture.image.width(),
    captured.capture.image.height(),
    captured.capture.backend
  );
  Ok(())
}

fn capture_window(session: &LinuxDriverSession, substr: &str, out: Option<&str>) -> Run {
  let window = find_window(session, substr)?;
  let out = out.unwrap_or("validate-linux-window.png");
  let captured = session.window().capture(&window)?;
  captured.image.save(out)?;
  println!(
    "captured window {:?} {}x{} via {} -> {out}",
    window.title,
    captured.image.width(),
    captured.image.height(),
    captured.backend
  );
  if let Some(reason) = captured.fallback_reason {
    println!("fallback_reason: {reason}");
  }
  Ok(())
}

fn ocr(session: &LinuxDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let captured = session.window().capture(&window)?;
  let recognition = session
    .vision()
    .recognize_text_in_capture(&captured, RatioRect::new(0.0, 0.0, 1.0, 1.0))?;
  println!("recognized {} regions:", recognition.regions.len());
  for region in recognition.regions.iter().take(80) {
    println!(
      "  {:?} conf={:?} bounds={:?}",
      region.text, region.confidence, region.bounds
    );
  }
  if recognition.regions.len() > 80 {
    println!("  ... {} more regions", recognition.regions.len() - 80);
  }
  Ok(())
}

fn ax(session: &LinuxDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let snapshot = session.accessibility().snapshot_window(&window)?;
  println!(
    "ax window {:?} ref={} nodes={}",
    window.title,
    snapshot.window_ref,
    snapshot.nodes.len()
  );
  for node in snapshot.nodes.iter().take(80) {
    println!(
      "  depth={} path={} type={:?} name={:?} id={:?} focused={} bounds={:?}",
      node.depth,
      node.path,
      node.control_type,
      node.name,
      node.automation_id,
      node.focused,
      node.bounds
    );
  }
  if snapshot.nodes.len() > 80 {
    println!("  ... {} more nodes", snapshot.nodes.len() - 80);
  }
  Ok(())
}

fn coords(session: &LinuxDriverSession, substr: &str) -> Run {
  let window = find_window(session, substr)?;
  let local = WindowPoint::new(10.0, 20.0);
  let screen = session.window().to_screen_point(&window, local)?;
  let back = session.window().to_window_point(&window, screen)?;
  println!("window frame origin = {:?}", window.frame.origin);
  println!(
    "window {:?} -> screen {:?} -> window {:?}",
    local, screen, back
  );
  Ok(())
}

fn clipboard(session: &LinuxDriverSession) -> Run {
  let clipboard = session.clipboard();
  let original = clipboard.snapshot()?;
  println!("original clipboard = {original:?}");

  let probe = "auv-validate-linux-clipboard";
  clipboard.set_text(probe)?;
  let read_back = clipboard.snapshot()?;
  println!("read-back clipboard = {read_back:?}");
  assert_eq!(read_back, probe, "clipboard set/read mismatch");

  clipboard.restore(&original)?;
  let restored = clipboard.snapshot()?;
  assert_eq!(restored, original, "clipboard restore mismatch");
  println!("clipboard roundtrip OK");
  Ok(())
}

fn type_text(session: &LinuxDriverSession, text: &str) -> Run {
  let result = session
    .input()
    .type_text(text, TypeTextOptions::default())?;
  println!("type-text result: {result:?}");
  Ok(())
}

fn press(session: &LinuxDriverSession, key: &str) -> Run {
  let result = session.input().press_key(KeyPressOptions {
    key: key.to_string(),
    ..KeyPressOptions::default()
  })?;
  println!("press result: {result:?}");
  Ok(())
}

fn scroll(session: &LinuxDriverSession, x: f64, y: f64, delta_y: f64) -> Run {
  let result = session.input().scroll_at(
    Point::new(x, y),
    Scroll::new(0.0, delta_y),
    std::time::Duration::from_millis(100),
  )?;
  println!("scroll result: {result:?}");
  Ok(())
}

fn click(session: &LinuxDriverSession, x: f64, y: f64) -> Run {
  let result = session.input().click_at(Point::new(x, y), Click::Single)?;
  println!("click result: {result:?}");
  Ok(())
}

fn input_boundary(session: &LinuxDriverSession) -> Run {
  let probe = session.permission().probe_linux();
  println!("RemoteDesktop portal: {:?}", probe.remote_desktop);
  println!(
    "input delivery uses the RemoteDesktop portal Notify* path; click coordinates may fall back to the current pointer until ScreenCast stream mapping lands"
  );
  println!(
    "reserved result shape: {:?}",
    auv_driver_linux::input::reserved_input_result("portal/libei session not wired")
  );
  Ok(())
}

fn find_window(session: &LinuxDriverSession, substr: &str) -> Result<Window, Box<dyn Error>> {
  Ok(
    session
      .window()
      .resolve(SelectWindow::title_contains(substr))
      .or_else(|_| {
        session
          .window()
          .resolve(WindowSelector::default().owned_by(AppSelector {
            bundle: Some(TextMatcher::Contains(substr.to_string())),
            ..AppSelector::default()
          }))
      })
      .or_else(|_| {
        session
          .window()
          .resolve(WindowSelector::default().owned_by(AppSelector {
            name: Some(TextMatcher::Contains(substr.to_string())),
            ..AppSelector::default()
          }))
      })
      .or_else(|_| session.window().resolve(SelectWindow::main_visible()))?,
  )
}

fn arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, Box<dyn Error>> {
  args
    .get(index)
    .map(String::as_str)
    .ok_or_else(|| format!("missing argument {name}").into())
}

fn parse<T>(args: &[String], index: usize) -> Result<T, Box<dyn Error>>
where
  T: std::str::FromStr,
  T::Err: Error + 'static,
{
  Ok(arg(args, index, "number")?.parse()?)
}

fn print_usage() {
  eprintln!(
    "usage: cargo run -p auv-driver-linux --example validate -- <command> [args] [<command> [args] ...]\n\
commands: permissions|displays|windows|resolve|capture-screen|capture-region|capture-window|ocr|ax|coords|clipboard|type-text|press|scroll|click|input-boundary"
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_multiple_zero_arg_invocations() {
    let invocations = parse(["permissions", "windows", "input-boundary"]);

    assert_eq!(
      invocations,
      vec![
        invocation("permissions", []),
        invocation("windows", []),
        invocation("input-boundary", [])
      ]
    );
  }

  #[test]
  fn parses_mixed_invocations_with_fixed_args() {
    let invocations = parse([
      "resolve", "Terminal", "click", "10", "20", "press", "Return",
    ]);

    assert_eq!(
      invocations,
      vec![
        invocation("resolve", ["Terminal"]),
        invocation("click", ["10", "20"]),
        invocation("press", ["Return"])
      ]
    );
  }

  #[test]
  fn optional_args_stop_at_next_command() {
    let invocations = parse(["capture-screen", "clipboard"]);

    assert_eq!(
      invocations,
      vec![
        invocation("capture-screen", []),
        invocation("clipboard", [])
      ]
    );
  }

  #[test]
  fn explicit_separator_disambiguates_optional_args() {
    let invocations = parse(["capture-screen", "--", "clipboard"]);

    assert_eq!(
      invocations,
      vec![
        invocation("capture-screen", []),
        invocation("clipboard", [])
      ]
    );
  }

  fn parse<const N: usize>(args: [&str; N]) -> Vec<Invocation> {
    let args = args
      .into_iter()
      .map(ToString::to_string)
      .collect::<Vec<_>>();
    parse_invocations(&args).expect("args should parse")
  }

  fn invocation<const N: usize>(command: &str, args: [&str; N]) -> Invocation {
    Invocation {
      command: command.to_string(),
      args: args.into_iter().map(ToString::to_string).collect(),
    }
  }
}
