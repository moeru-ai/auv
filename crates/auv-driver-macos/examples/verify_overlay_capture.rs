//! Opt-in macOS probe for retained overlays, clean screenshots and removal.
//! Run with `--features overlay --example verify_overlay_capture -- <display-id>`.
//! Pixels are counted in memory; no screenshots are saved and no input is sent.
#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
  use auv_driver_common::{CaptureOptions, DisplayCapture, Driver, Rect, ScreenPoint};
  use auv_driver_macos::MacosDriver;
  use auv_driver_overlay::{
    Easing, LifecycleOptions, Overlay, OverlayTheme, ShowOptions,
    layers::{Outline, Status},
    style::{Color, OutlineStyle, StatusStyle, Stroke},
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
  if bounds.size.width < 340. || bounds.size.height < 180. {
    return Err("probe requires a display of at least 340x180 logical points".into());
  }
  let options = || CaptureOptions {
    display: Some(display_id.clone()),
    ..Default::default()
  };
  let show_options =
    || ShowOptions::new().with_motion_ease(Duration::ZERO, Easing::EaseInOutExpo).with_lifecycle_options(LifecycleOptions::manual());
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
  let outline_region = Rect::new(276., 32., 48., 48.);
  let baseline = session.display().capture(options())?;
  let baseline_card = count(&baseline, card_region);
  let baseline_outline = count(&baseline, outline_region);
  let _cleanup = Cleanup;
  let marker = Color::rgb(243. / 255., 8. / 255., 162. / 255.);
  let card_only = Overlay::new().with_layer(
    Status::new(ScreenPoint::new(bounds.origin.x + 16., bounds.origin.y + 36.), "overlay capture probe\npersistent status")
      .with_style(StatusStyle::new().with_background(marker)),
  );
  let with_extra = card_only.clone().with_layer(
    Outline::new(Rect::new(bounds.origin.x + 280., bounds.origin.y + 36., 40., 40.))
      .with_style(OutlineStyle::new().with_corner_radius(0.).with_stroke(Stroke::new(marker, 4.))),
  );
  auv_driver_overlay::show_with_theme(&with_extra, show_options(), &OverlayTheme::default())?;
  auv_driver_overlay::pump_events(Duration::from_millis(100))?;
  let visible = session.display().capture(options())?;
  let excluded = session.display().capture_excluding_self(options())?;
  auv_driver_overlay::show_with_theme(&card_only, show_options(), &OverlayTheme::default())?;
  auv_driver_overlay::remove_obsolete(&card_only)?;
  auv_driver_overlay::pump_events(Duration::from_millis(50))?;
  let after = session.display().capture(options())?;
  // No sleep or event pumping between removal and capture: hide must be flushed.
  auv_driver_overlay::remove()?;
  let hidden = session.display().capture(options())?;
  let before_count = count(&visible, card_region);
  let excluded_count = count(&excluded, card_region);
  let after_count = count(&after, card_region);
  let hidden_count = count(&hidden, card_region);
  let outline_before = count(&visible, outline_region);
  let outline_after = count(&after, outline_region);
  println!(
    "baseline={baseline_card} visible={before_count} excluded={excluded_count} retained={after_count} hidden={hidden_count} outline_before={outline_before} outline_after={outline_after} image={}x{}",
    excluded.capture.image.width(),
    excluded.capture.image.height()
  );
  if before_count < baseline_card + 500
    || excluded_count > baseline_card + 100
    || after_count < before_count * 9 / 10
    || hidden_count > baseline_card + 100
    || outline_before < baseline_outline + 100
    || outline_after > baseline_outline + 100
  {
    return Err("overlay retention, capture exclusion, pruning or hide flush failed".into());
  }
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("macOS-only capture exclusion probe");
}
