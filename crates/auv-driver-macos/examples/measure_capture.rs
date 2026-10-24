//! Opt-in capture timing: `--example measure_capture -- <display-id> [samples]`.
//! Pixels stay in memory; no screenshots are saved and no overlays are changed.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  use auv_driver_common::{CaptureOptions, Driver};
  use auv_driver_macos::MacosDriver;
  use std::time::Instant;

  let mut args = std::env::args().skip(1);
  let display = args.next().ok_or("usage: measure_capture <display-id> [samples]")?;
  let samples: u8 = args.next().map(|value| value.parse()).transpose()?.unwrap_or(5);
  if !(2..=30).contains(&samples) || args.next().is_some() {
    return Err("expected a display id and 2 to 30 samples".into());
  }
  let session = MacosDriver::new().open_local()?;
  let mut timings = [vec![], vec![]];
  let mut dimensions = None;
  for sample in 0..samples {
    // Alternate order to avoid always measuring one backend after the other.
    for backend in [usize::from(sample % 2), usize::from(1 - sample % 2)] {
      let options = CaptureOptions {
        display: Some(display.clone()),
        ..Default::default()
      };
      let start = Instant::now();
      let captured = if backend == 0 {
        session.display().capture(options)?
      } else {
        session.display().capture_excluding_self(options)?
      };
      timings[backend].push(start.elapsed().as_secs_f64() * 1000.);
      let size = captured.capture.image.dimensions();
      if dimensions.replace(size).is_some_and(|previous| previous != size) {
        return Err("capture dimensions changed between samples/backends".into());
      }
    }
  }
  println!("image={dimensions:?} warm_samples={}", samples - 1);
  for (name, values) in ["normal", "excluding_self"].into_iter().zip(&timings) {
    let mut warm = values[1..].to_vec();
    warm.sort_by(f64::total_cmp);
    let median = (warm[(warm.len() - 1) / 2] + warm[warm.len() / 2]) / 2.;
    println!("{name}: first={:.3} median={median:.3} ms", values[0]);
  }
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("macOS-only capture timing probe");
}
