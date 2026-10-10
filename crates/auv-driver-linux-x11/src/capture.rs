use crate::{
  X11DriverSession,
  session::{backend, invalid},
};
use auv_driver_common::{
  Activation, Capture, CaptureOptions, CoordinateSpace, Display, DisplayCapture, DriverResult, ObservedDisplays, Rect, RegionCapture,
};

/// X11 monitor observation in physical root-window pixel coordinates.
#[derive(Clone, Copy, Debug)]
pub struct DisplayApi<'a> {
  pub(crate) session: &'a X11DriverSession,
}

impl DisplayApi<'_> {
  /// Lists connected monitors; IDs are xcap's XRandR output identifiers.
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    self.session.check_environment()?;
    Ok(ObservedDisplays {
      displays: monitors()?.iter().map(|(_, display)| display.clone()).collect(),
    })
  }

  /// Captures a selected monitor (ID/name), defaulting to primary then first.
  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    validate_options(&options)?;
    if options.region.is_some() {
      return Err(invalid("display.capture does not accept a region"));
    }
    self.session.check_environment()?;
    let targets = monitors()?;
    let (monitor, display) = select(&targets, options.display.as_deref(), None)?;
    let image = monitor.capture_image().map_err(backend)?;
    if (image.width(), image.height()) != (display.frame.size.width as u32, display.frame.size.height as u32) {
      return Err(backend("monitor geometry changed during capture; observe again"));
    }
    Ok(DisplayCapture {
      display: display.clone(),
      capture: capture(image, display.frame),
    })
  }

  /// Captures an integral, nonempty region wholly inside one monitor.
  /// Regions use root-window pixels, including the selected monitor's offset.
  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    validate_options(&options)?;
    let region = options.region.ok_or_else(|| invalid("capture_region requires a region"))?;
    validate_region(region)?;
    self.session.check_environment()?;
    let targets = monitors()?;
    let (monitor, display) = select(&targets, options.display.as_deref(), Some(region))?;
    let image = monitor
      .capture_region(
        (region.origin.x - display.frame.origin.x) as u32,
        (region.origin.y - display.frame.origin.y) as u32,
        region.size.width as u32,
        region.size.height as u32,
      )
      .map_err(backend)?;
    Ok(RegionCapture {
      display: display.clone(),
      capture: capture(image, region),
    })
  }
}

fn validate_options(options: &CaptureOptions) -> DriverResult<()> {
  if options.window.is_some() || !matches!(options.activation, Activation::KeepCurrent) {
    return Err(invalid("X11 display capture does not activate or target windows"));
  }
  Ok(())
}

fn capture(image: image::RgbaImage, bounds: Rect) -> Capture {
  Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(bounds.origin))),
    image,
    bounds,
    scale_factor: 1.0,
    backend: "xcap.x11".into(),
    fallback_reason: None,
  }
}

fn monitors() -> DriverResult<Vec<(xcap::Monitor, Display)>> {
  xcap::Monitor::all()
    .map_err(backend)?
    .into_iter()
    .map(|monitor| {
      let display = Display {
        id: monitor.id().map_err(backend)?.to_string(),
        name: Some(monitor.name().map_err(backend)?),
        frame: Rect::new(
          f64::from(monitor.x().map_err(backend)?),
          f64::from(monitor.y().map_err(backend)?),
          f64::from(monitor.width().map_err(backend)?),
          f64::from(monitor.height().map_err(backend)?),
        ),
        coordinate_space: CoordinateSpace::Screen,
        // XTEST and XRandR both use root pixels. Xft DPI is font scaling, not a
        // conversion between the captured pixels and the input coordinate space.
        scale_factor: 1.0,
        is_primary: monitor.is_primary().map_err(backend)?,
        is_builtin: None,
      };
      Ok((monitor, display))
    })
    .collect()
}

fn select<'a>(
  targets: &'a [(xcap::Monitor, Display)],
  selector: Option<&str>,
  region: Option<Rect>,
) -> DriverResult<&'a (xcap::Monitor, Display)> {
  let target = if let Some(selector) = selector {
    targets.iter().find(|(_, display)| display.id == selector || display.name.as_deref() == Some(selector))
  } else if let Some(region) = region {
    targets.iter().find(|(_, display)| contains(display.frame, region))
  } else {
    targets.iter().find(|(_, display)| display.is_primary).or_else(|| targets.first())
  }
  .ok_or_else(|| auv_driver_common::DriverError::NotFound {
    target: format!("X11 monitor {selector:?} containing {region:?}"),
  })?;
  if region.is_some_and(|region| !contains(target.1.frame, region)) {
    return Err(invalid("region must be contained within the selected monitor"));
  }
  Ok(target)
}

fn validate_region(region: Rect) -> DriverResult<()> {
  for value in [
    region.origin.x,
    region.origin.y,
    region.size.width,
    region.size.height,
  ] {
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > f64::from(i32::MAX) {
      return Err(invalid("capture region must use finite integral X11 pixel coordinates"));
    }
  }
  if region.size.width <= 0.0 || region.size.height <= 0.0 {
    return Err(invalid("capture region must be nonempty"));
  }
  Ok(())
}

fn contains(frame: Rect, region: Rect) -> bool {
  region.origin.x >= frame.origin.x
    && region.origin.y >= frame.origin.y
    && region.origin.x + region.size.width <= frame.origin.x + frame.size.width
    && region.origin.y + region.size.height <= frame.origin.y + frame.size.height
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn capture_binds_pixels_to_the_x11_root_screen() {
    let bounds = Rect::new(-1920.0, 10.0, 4.0, 3.0);
    let capture = capture(image::RgbaImage::new(4, 3), bounds);
    assert_eq!(capture.origin, Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(bounds.origin))));
  }

  #[test]
  fn region_validation_rejects_lossy_or_empty_crops() {
    for region in [
      Rect::new(f64::NAN, 0.0, 2.0, 2.0),
      Rect::new(0.5, 0.0, 2.0, 2.0),
      Rect::new(0.0, 0.0, 0.0, 2.0),
    ] {
      assert!(validate_region(region).is_err());
    }
    assert!(validate_region(Rect::new(-1920.0, 0.0, 1920.0, 1080.0)).is_ok());
  }
  #[test]
  fn offset_monitor_contains_only_its_own_pixels() {
    let monitor = Rect::new(-1920.0, 0.0, 1920.0, 1080.0);
    assert!(contains(monitor, Rect::new(-20.0, 5.0, 20.0, 10.0)));
    assert!(!contains(monitor, Rect::new(-20.0, 5.0, 21.0, 10.0)));
  }
}
