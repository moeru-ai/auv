//! Times `render()` per frame: a full virtual-screen frame plus `UpdateLayeredWindow`.
//!
//! ```text
//! cargo run --release -p auv-driver-overlay-windows --example present_latency_probe -- [frames] [basic]
//! ```
//!
//! `basic` draws only layers every Windows renderer accepts (disc cursor, outline, status),
//! so the same scene can run against an older renderer. Without it, an SVG cursor with a
//! glow is added. Prints P50/P95/mean with linear-interpolation percentiles and no outlier
//! filtering. Needs an interactive desktop; the overlay stays visible while it runs.

#[cfg(target_os = "windows")]
fn main() {
  use auv_driver_common::{Rect, ScreenPoint};
  use auv_driver_overlay_common::layers::{BuiltInCursor, Cursor, CursorImage, Outline, Status};
  use auv_driver_overlay_common::style::{CursorStyle, Shadow};
  use auv_driver_overlay_common::{LifecycleOptions, Overlay, ShowOptions};

  let iterations: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(30);
  let basic = std::env::args().nth(2).as_deref() == Some("basic");
  let mut overlay = Overlay::new()
    .with_layer(Cursor::new(ScreenPoint::new(40.0, 40.0)).with_label("auv").with_label_visible())
    .with_layer(Outline::new(Rect::new(20.0, 130.0, 120.0, 60.0)).with_label("outline").with_label_visible())
    .with_layer(Status::new(ScreenPoint::new(20.0, 220.0), "status"));
  if !basic {
    overlay = overlay.with_layer(
      Cursor::new(ScreenPoint::new(40.0, 90.0))
        .with_image(CursorImage::svg(BuiltInCursor::Auv.svg_source().unwrap()))
        .with_style(CursorStyle::default().with_shadow(Some(Shadow::auv())))
        .with_label("svg")
        .with_label_visible(),
    );
  }
  let manual = ShowOptions::new().with_lifecycle_options(LifecycleOptions::manual());

  auv_driver_overlay_windows::render(&overlay, manual).expect("warm-up render");
  let mut samples = Vec::with_capacity(iterations);
  for _ in 0..iterations {
    let started = std::time::Instant::now();
    auv_driver_overlay_windows::render(&overlay, manual).expect("render");
    samples.push(started.elapsed().as_secs_f64() * 1000.0);
  }
  auv_driver_overlay_windows::remove().ok();

  samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
  let pct = |p: f64| {
    let rank = p * (samples.len() as f64 - 1.0);
    let low = rank.floor() as usize;
    let high = (low + 1).min(samples.len() - 1);
    samples[low] + (samples[high] - samples[low]) * (rank - low as f64)
  };
  let mean = samples.iter().sum::<f64>() / samples.len() as f64;
  println!(
    "n={} P50={:.2}ms P95={:.2}ms mean={:.2}ms min={:.2} max={:.2}",
    samples.len(),
    pct(0.5),
    pct(0.95),
    mean,
    samples[0],
    samples[samples.len() - 1]
  );
}

#[cfg(not(target_os = "windows"))]
fn main() {}
