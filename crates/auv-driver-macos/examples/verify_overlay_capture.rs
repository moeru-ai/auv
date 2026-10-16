//! Opt-in macOS probe for clean screenshots while an overlay is visible.
//! Run with `--features overlay --example verify_overlay_capture -- <display-id>`.
//! Pixels are counted in memory; no screenshots are saved and no input is sent.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  use auv_driver_common::{CaptureOptions, DisplayCapture, Driver, DriverError, Rect, ScreenPoint};
  use auv_driver_macos::MacosDriver;
  use auv_driver_overlay::{
    Easing, Overlay, OverlayTheme, ShowOptions,
    layers::Status,
    style::{Color, StatusStyle},
  };
  use std::time::Duration;

  struct Cleanup;
  impl Drop for Cleanup {
    fn drop(&mut self) {
      let _ = auv_driver_overlay::remove();
    }
  }

  let display_id = std::env::args().nth(1).ok_or("usage: verify_overlay_capture <display-id>")?;
  let session = MacosDriver::new().open_local()?;
  let displays = session.display().list()?.displays;
  let display = displays.iter().find(|display| display.id == display_id).ok_or("display not found")?;
  let bounds = display.frame;
  if bounds.size.width < 280. || bounds.size.height < 180. {
    return Err("probe requires a display of at least 280x180 logical points".into());
  }
  let options = || CaptureOptions {
    display: Some(display_id.clone()),
    ..Default::default()
  };
  let count = |capture: &DisplayCapture, region: Rect| {
    // Regions are relative to this display, independent of desktop origin.
    let image = &capture.capture.image;
    let sx = f64::from(image.width()) / bounds.size.width;
    let sy = f64::from(image.height()) / bounds.size.height;
    image::imageops::crop_imm(
      image,
      (region.origin.x * sx) as u32,
      (region.origin.y * sy) as u32,
      (region.size.width * sx) as u32,
      (region.size.height * sy) as u32,
    )
    .to_image()
    .pixels()
    // Match hue rather than exact RGB: display profiles can shift the channels.
    .filter(|p| p[0] > 180 && p[1] < 90 && p[2] > 110 && p[2] < 230)
    .count()
  };
  let card_region = Rect::new(10., 30., 260., 100.);
  let baseline = session.display().capture(options())?;
  let baseline_card = count(&baseline, card_region);
  let _cleanup = Cleanup;
  let marker = Color::rgb(243. / 255., 8. / 255., 162. / 255.);
  let overlay = Overlay::new().with_layer(
    Status::new(ScreenPoint::new(bounds.origin.x + 16., bounds.origin.y + 36.), "overlay capture probe\npersistent status")
      .with_style(StatusStyle::new().with_background(marker)),
  );
  let (visible, excluded, after) = std::thread::scope(|scope| -> Result<_, Box<dyn std::error::Error>> {
    let worker = scope.spawn(|| -> Result<_, DriverError> {
      // Let the existing timed-show interface service the first AppKit turn.
      std::thread::sleep(Duration::from_millis(100));
      let visible = session.display().capture(options())?;
      let excluded = session.display().capture_excluding_self(options())?;
      let after = session.display().capture(options())?;
      Ok((visible, excluded, after))
    });
    let shown = auv_driver_overlay::show_with_theme(
      &overlay,
      ShowOptions::new().with_motion_ease(Duration::ZERO, Easing::EaseInOutExpo).with_auto_removal_after(Duration::from_secs(3)),
      &OverlayTheme::default(),
    );
    let captured = worker.join().map_err(|_| "capture worker panicked")?;
    shown?;
    Ok(captured?)
  })?;
  // Timed show removes the overlay before returning; capture immediately.
  let hidden = session.display().capture(options())?;
  let before_count = count(&visible, card_region);
  let excluded_count = count(&excluded, card_region);
  let after_count = count(&after, card_region);
  let hidden_count = count(&hidden, card_region);
  println!(
    "baseline={baseline_card} visible={before_count} excluded={excluded_count} after_capture={after_count} hidden={hidden_count} image={}x{}",
    excluded.capture.image.width(),
    excluded.capture.image.height()
  );
  if before_count < baseline_card + 500
    || excluded_count > baseline_card + 100
    || after_count < baseline_card + 500
    || hidden_count > baseline_card + 100
  {
    return Err("capture exclusion, overlay visibility or hide flush failed".into());
  }
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("macOS-only capture exclusion probe");
}
