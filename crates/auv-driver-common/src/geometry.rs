use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
  #[default]
  Screen,
  Display(String),
  Window(String),
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
pub struct Point3 {
  pub x: f64,
  pub y: f64,
  pub z: f64,
}

impl Point3 {
  pub const fn new(x: f64, y: f64, z: f64) -> Self {
    Self { x, y, z }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldPoint(pub Point3);

impl WorldPoint {
  pub const fn new(x: f64, y: f64, z: f64) -> Self {
    Self(Point3::new(x, y, z))
  }

  pub const fn point(self) -> Point3 {
    self.0
  }
}

impl From<Point3> for WorldPoint {
  fn from(point: Point3) -> Self {
    Self(point)
  }
}

impl From<WorldPoint> for Point3 {
  fn from(point: WorldPoint) -> Self {
    point.0
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraPoint(pub Point3);

impl CameraPoint {
  pub const fn new(x: f64, y: f64, z: f64) -> Self {
    Self(Point3::new(x, y, z))
  }

  pub const fn point(self) -> Point3 {
    self.0
  }
}

impl From<Point3> for CameraPoint {
  fn from(point: Point3) -> Self {
    Self(point)
  }
}

impl From<CameraPoint> for Point3 {
  fn from(point: CameraPoint) -> Self {
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
    Point::new(self.origin.x + self.size.width / 2.0, self.origin.y + self.size.height / 2.0)
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

/// An integer-pixel rectangle observed directly from a platform report (for
/// example, a parsed macOS AX/window/OCR report line). Distinct from `Rect`
/// (logical f64 coordinates): this type preserves the raw pixel units a
/// driver observed before any coordinate-space normalization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedRect {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProjectionSourceSpace {
  World,
  Camera,
  SourceImagePixels,
  Local2d { name: String },
  Other { name: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDerivationFamily {
  LayoutRule,
  CameraMatrix,
  EmpiricalCalibration,
  ExternalTelemetry,
  Unknown,
}

/// Generic provenance for a source-to-screen/window projection.
///
/// This type records why a projected coordinate is action-grade evidence. It
/// intentionally carries no app-specific target semantics and does not perform
/// projection math.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionBasis {
  pub basis_id: String,
  pub timestamp_millis: u64,
  pub source_space: ProjectionSourceSpace,
  pub projected_coordinate_space: CoordinateSpace,
  pub derivation_family: ProjectionDerivationFamily,
  pub confidence: f64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub match_radius_px: Option<f64>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
}

impl ProjectionBasis {
  pub fn new(
    basis_id: impl Into<String>,
    timestamp_millis: u64,
    source_space: ProjectionSourceSpace,
    projected_coordinate_space: CoordinateSpace,
    derivation_family: ProjectionDerivationFamily,
  ) -> Self {
    Self {
      basis_id: basis_id.into(),
      timestamp_millis,
      source_space,
      projected_coordinate_space,
      derivation_family,
      confidence: 1.0,
      match_radius_px: None,
      known_limits: Vec::new(),
    }
  }

  pub fn with_confidence(mut self, confidence: f64) -> Self {
    self.confidence = confidence;
    self
  }

  pub fn with_match_radius_px(mut self, match_radius_px: f64) -> Self {
    self.match_radius_px = Some(match_radius_px);
    self
  }

  pub fn with_known_limit(mut self, known_limit: impl Into<String>) -> Self {
    self.known_limits.push(known_limit.into());
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn projection_basis_serializes_generic_provenance() {
    let basis = ProjectionBasis::new(
      "basis-frame-1",
      1_000,
      ProjectionSourceSpace::World,
      CoordinateSpace::Window("window-1".to_string()),
      ProjectionDerivationFamily::CameraMatrix,
    )
    .with_confidence(0.75)
    .with_match_radius_px(12.0)
    .with_known_limit("viewport-relative until capture binding is attached");

    let value = serde_json::to_value(&basis).expect("serialize projection basis");

    assert_eq!(value["basis_id"], serde_json::json!("basis-frame-1"));
    assert_eq!(value["source_space"]["kind"], serde_json::json!("world"));
    assert_eq!(value["derivation_family"], serde_json::json!("camera_matrix"));
    assert_eq!(value["match_radius_px"], serde_json::json!(12.0));
  }
}
