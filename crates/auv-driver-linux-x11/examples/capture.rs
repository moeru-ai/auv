//! Read-only capture probe; output is an explicitly supplied PNG path.

/// Opens the selected X11 session and saves one monitor image.
///
/// Call stack: main -> X11Driver::open_local -> DisplayApi::capture -> xcap.
#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  use auv_driver_common::{CaptureOptions, Driver};
  use auv_driver_linux_x11::X11Driver;
  let output = std::env::args_os().nth(1).ok_or("usage: capture <output.png>")?;
  let session = X11Driver.open_local()?;
  let capture = session.display().capture(CaptureOptions::default())?;
  capture.capture.image.save(std::path::PathBuf::from(output))?;
  println!("{}: {}x{} ({})", capture.display.id, capture.capture.image.width(), capture.capture.image.height(), capture.capture.backend);
  Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
  eprintln!("This capture probe requires Linux with an X11 session.");
  std::process::exit(1);
}
