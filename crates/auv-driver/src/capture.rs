use std::time::Duration;

use image::{RgbaImage, SubImage};

use crate::geometry::Rect;
use crate::window::WindowRef;

pub type ImageView<'a> = SubImage<&'a RgbaImage>;

#[derive(Clone, Debug, PartialEq)]
pub enum Activation {
  KeepCurrent,
  ActivateFirst { settle: Duration },
}

impl Default for Activation {
  fn default() -> Self {
    Self::KeepCurrent
  }
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
}
