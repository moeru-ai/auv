use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
  Screen,
  Display(String),
  Window(String),
}

impl Default for CoordinateSpace {
  fn default() -> Self {
    Self::Screen
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
  pub x: f64,
  pub y: f64,
}

impl Point {
  pub const fn new(x: f64, y: f64) -> Self {
    Self { x, y }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
  pub width: f64,
  pub height: f64,
}

impl Size {
  pub const fn new(width: f64, height: f64) -> Self {
    Self { width, height }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
  pub origin: Point,
  pub size: Size,
}

impl Rect {
  pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      origin: Point::new(x, y),
      size: Size::new(width, height),
    }
  }

  pub fn center(self) -> Point {
    Point::new(
      self.origin.x + self.size.width / 2.0,
      self.origin.y + self.size.height / 2.0,
    )
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RatioRect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl RatioRect {
  pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }
}
