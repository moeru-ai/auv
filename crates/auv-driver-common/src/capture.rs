use std::time::Duration;

use image::{RgbaImage, SubImage};

use crate::display::Display;
use crate::geometry::Rect;
use crate::window::WindowRef;

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
  pub image: RgbaImage,
  pub bounds: Rect,
  pub scale_factor: f64,
  pub backend: String,
  pub fallback_reason: Option<String>,
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
