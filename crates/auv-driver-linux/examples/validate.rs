//! Live validation harness for the Linux driver capabilities.
//!
//! Usage:
//!
//! ```text
//! cargo run -p auv-driver-linux --example validate -- <command> [args]
//! ```
//!
//! Commands:
//!   permissions                    probe Wayland/XDG portal readiness
//!   displays                       list xcap-visible displays
//!   windows                        list xcap-visible windows
//!   resolve <title-substr>         resolve a window by title substring
//!   capture-screen [out.png]       capture the primary display to a PNG
//!   capture-region <x> <y> <w> <h> [out.png]
//!   capture-window <substr> [out]  capture a window to a PNG
//!   coords <title-substr>          round-trip a window/screen point mapping
//!   input-boundary                 print current RemoteDesktop/libei boundary

use std::error::Error;

use auv_driver::Driver;
use auv_driver::capture::CaptureOptions;
use auv_driver::geometry::{Rect, WindowPoint};
use auv_driver::selector::Window as SelectWindow;
use auv_driver::window::Window;
use auv_driver_linux::{LinuxDriver, LinuxDriverSession};

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
  let session = LinuxDriver::new().open_local()?;
  let rest = &args[1..];

  match command.as_str() {
    "permissions" => permissions(&session),
    "displays" => displays(&session),
    "windows" => windows(&session),
    "resolve" => resolve(&session, arg(rest, 0, "title-substr")?),
    "capture-screen" => capture_screen(&session, rest.first().map(String::as_str)),
    "capture-region" => capture_region(
      &session,
      parse(rest, 0)?,
      parse(rest, 1)?,
      parse(rest, 2)?,
      parse(rest, 3)?,
      rest.get(4).map(String::as_str),
    ),
    "capture-window" => capture_window(
      &session,
      arg(rest, 0, "title-substr")?,
      rest.get(1).map(String::as_str),
    ),
    "coords" => coords(&session, arg(rest, 0, "title-substr")?),
    "input-boundary" => input_boundary(&session),
    other => {
      eprintln!("unknown command: {other}\n");
      print_usage();
      Ok(())
    }
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

fn input_boundary(session: &LinuxDriverSession) -> Run {
  let probe = session.permission().probe_linux();
  println!("RemoteDesktop portal: {:?}", probe.remote_desktop);
  println!(
    "input delivery is intentionally unsupported in this slice; wire portal/libei before using click/type/scroll"
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
    "usage: cargo run -p auv-driver-linux --example validate -- <permissions|displays|windows|resolve|capture-screen|capture-region|capture-window|coords|input-boundary> ..."
  );
}
