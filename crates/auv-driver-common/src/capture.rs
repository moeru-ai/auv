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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaptureOptions {
  pub activation: Activation,
  pub display: Option<String>,
  pub window: Option<WindowRef>,
  pub region: Option<Rect>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayCapture {
  pub display: Display,
  pub capture: Capture,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegionCapture {
  pub display: Display,
  pub capture: Capture,
}
