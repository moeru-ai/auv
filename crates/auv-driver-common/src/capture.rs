use std::time::Duration;

use image::{RgbaImage, SubImage};

use crate::display::Display;
use crate::geometry::{Point, Position, Positional, Rect};
use crate::window::WindowRef;
use crate::{DriverError, DriverResult};

pub type ImageView<'a> = SubImage<&'a RgbaImage>;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Activation {
  #[default]
  KeepCurrent,
  ActivateFirst {
    settle: Duration,
  },
}

/// The pixel resolution a capture is taken at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaptureResolution {
  /// The display's backing pixels (2× on Retina). For OCR and anything that
  /// reads small detail.
  #[default]
  Native,
  /// One pixel per logical point. For display and motion checks: a quarter of
  /// a Retina capture's pixels, and the image lines up with logical bounds.
  Logical,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureOptions {
  pub activation: Activation,
  pub display: Option<String>,
  pub window: Option<WindowRef>,
  pub region: Option<Rect>,
  pub resolution: CaptureResolution,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
  /// Logical location of the image's top-left pixel in its owning space.
  /// Full-window captures use (0, 0) in that exact window. Detached images may
  /// be unbound; they must not pretend to be screen or window input targets.
  pub origin: Option<Position>,
  pub image: RgbaImage,
  pub bounds: Rect,
  pub scale_factor: f64,
  pub backend: String,
  pub fallback_reason: Option<String>,
}

impl Positional for Capture {
  fn position(&self) -> DriverResult<Position> {
    self.origin.clone().ok_or_else(|| DriverError::InvalidInput {
      message: "capture has no bound coordinate origin".into(),
    })
  }
}

impl Capture {
  /// This capture at `resolution`. `Logical` averages pixel areas down to one
  /// pixel per point (what a 1× display shows) and sets `scale_factor` to 1;
  /// captures already at or below 1× are returned unchanged.
  ///
  /// NOTICE(capture-logical-downscale): backends that cannot capture at 1×
  /// directly use this after a native capture. Area averaging took ~9 ms for
  /// a Retina window, versus 21–37 ms for Triangle, CatmullRom or Lanczos3
  /// (2026-10-07).
  pub fn at_resolution(self, resolution: CaptureResolution) -> Self {
    if resolution == CaptureResolution::Native || !self.scale_factor.is_finite() || self.scale_factor <= 1.0 {
      return self;
    }
    let width = (self.bounds.size.width.round() as u32).max(1);
    let height = (self.bounds.size.height.round() as u32).max(1);
    Self {
      image: image::imageops::thumbnail(&self.image, width, height),
      scale_factor: 1.0,
      ..self
    }
  }

  /// Capture OCR retains the existing `bounds`-based numeric coordinates.
  /// Record the offset that interprets those numbers in the owning space.
  /// This changes metadata only; pixel-to-logical scaling stays in the producer.
  pub fn recognition_origin(&self) -> Option<Position> {
    self.origin.as_ref().map(|origin| Position {
      point: Point::new(origin.point.x - self.bounds.origin.x, origin.point.y - self.bounds.origin.y),
      coordinate_space: origin.coordinate_space.clone(),
    })
  }
}

/// A display capture. `C` is how the capture is held: in-process pixels
/// (`Capture`), or a Runner-held reference in the `auv-core` client.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayCapture<C = Capture> {
  pub display: Display,
  pub capture: C,
}

/// A screen-region capture; `C` as for [`DisplayCapture`].
#[derive(Clone, Debug, PartialEq)]
pub struct RegionCapture<C = Capture> {
  pub display: Display,
  pub capture: C,
}

#[cfg(test)]
#[path = "capture_test.rs"]
mod tests;
