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
pub struct ScreenPoint(pub Point);

impl ScreenPoint {
  pub const fn new(x: f64, y: f64) -> Self {
    Self(Point::new(x, y))
  }

  pub const fn point(self) -> Point {
    self.0
  }
}

impl From<Point> for ScreenPoint {
  fn from(point: Point) -> Self {
    Self(point)
  }
}

impl From<ScreenPoint> for Point {
  fn from(point: ScreenPoint) -> Self {
    point.0
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowPoint(pub Point);

impl WindowPoint {
  pub const fn new(x: f64, y: f64) -> Self {
    Self(Point::new(x, y))
  }

  pub const fn point(self) -> Point {
    self.0
  }
}

impl From<Point> for WindowPoint {
  fn from(point: Point) -> Self {
    Self(point)
  }
}

impl From<WindowPoint> for Point {
  fn from(point: WindowPoint) -> Self {
    point.0
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
