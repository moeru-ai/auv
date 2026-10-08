use serde::{Deserialize, Serialize};

use crate::DriverResult;
use crate::window::WindowRef;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
  #[default]
  Screen,
  Display(String),
  Window(String),
}

/// A point together with the space needed to interpret it. Window positions
/// retain the exact window identity, not an app's mutable main-window selector.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
  pub point: Point,
  pub coordinate_space: CoordinateSpace,
}

impl Position {
  /// Binds a logical screen point to screen space without querying displays or
  /// validating coordinate freshness, finiteness, visibility, or actionability.
  pub fn in_screen(point: ScreenPoint) -> Self {
    Self {
      point: point.point(),
      coordinate_space: CoordinateSpace::Screen,
    }
  }

  /// Binds an already window-local logical point to this exact `WindowRef` ID.
  /// This copies the ID; it neither resolves a window nor converts screen
  /// coordinates. It does not validate window existence, coordinate freshness,
  /// finiteness, visibility, or actionability.
  pub fn in_window(window: &WindowRef, point: WindowPoint) -> Self {
    Self {
      point: point.point(),
      coordinate_space: CoordinateSpace::Window(window.id.clone()),
    }
  }
}

/// Supplies an observed position; this does not promise visibility, freshness,
/// or support for a particular action. It never re-runs a locator.
/// Unbound data, such as a detached image, returns an error.
pub trait Positional {
  fn position(&self) -> DriverResult<Position>;
}

impl Positional for Position {
  fn position(&self) -> DriverResult<Position> {
    Ok(self.clone())
  }
}

impl Positional for ScreenPoint {
  fn position(&self) -> DriverResult<Position> {
    Ok(Position::in_screen(*self))
  }
}

/// Keeps domain data alongside its chosen action point and coordinate context.
/// The point can differ from the data's bounds center (for example a card cover
/// located using its title). Updating it is an explicit caller decision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Positioned<T> {
  pub value: T,
  pub position: Position,
}

impl<T> Positional for Positioned<T> {
  fn position(&self) -> DriverResult<Position> {
    Ok(self.position.clone())
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

/// A size in physical image pixels, as opposed to a logical [`Size`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PixelSize {
  pub width: u32,
  pub height: u32,
}

impl PixelSize {
  pub const fn new(width: u32, height: u32) -> Self {
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

  // Geometry helpers, the same as `@auv-js/sdk`'s (`below(rect, 40)` there is
  // `rect.below(40.0, 0.0)` here). They never convert coordinate spaces; see
  // docs/ai/references/session-api/2026-10-09-sdk-ergonomics-design.md.

  /// The point at a fraction of the rectangle: `at(0.0, 0.0)` is the
  /// top-left and `at(0.5, 0.5)` the center.
  pub fn at(self, fx: f64, fy: f64) -> Point {
    Point::new(self.origin.x + self.size.width * fx, self.origin.y + self.size.height * fy)
  }

  /// The strip of `height` above this rectangle, `gap` away.
  pub fn above(self, height: f64, gap: f64) -> Self {
    Self::new(self.origin.x, self.origin.y - gap - height, self.size.width, height)
  }

  /// The strip of `height` below this rectangle, `gap` away.
  pub fn below(self, height: f64, gap: f64) -> Self {
    Self::new(self.origin.x, self.origin.y + self.size.height + gap, self.size.width, height)
  }

  /// The strip of `width` left of this rectangle, `gap` away.
  pub fn left_of(self, width: f64, gap: f64) -> Self {
    Self::new(self.origin.x - gap - width, self.origin.y, width, self.size.height)
  }

  /// The strip of `width` right of this rectangle, `gap` away.
  pub fn right_of(self, width: f64, gap: f64) -> Self {
    Self::new(self.origin.x + self.size.width + gap, self.origin.y, width, self.size.height)
  }

  /// Shrinks by the same amount on every side, or per side.
  pub fn inset(self, by: impl Into<Insets>) -> Self {
    let by = by.into();
    Self::new(self.origin.x + by.left, self.origin.y + by.top, self.size.width - by.left - by.right, self.size.height - by.top - by.bottom)
  }

  pub fn offset(self, dx: f64, dy: f64) -> Self {
    Self::new(self.origin.x + dx, self.origin.y + dy, self.size.width, self.size.height)
  }

  /// A box inside this rectangle by distances from its edges. Per axis give
  /// two of start, end and size (`left`/`right`/`width`,
  /// `top`/`bottom`/`height`): a missing start is 0 and a missing size fills
  /// the rest. Percentages are of this rectangle. All three on one axis is an
  /// error.
  pub fn region(self, edges: Edges) -> DriverResult<Self> {
    let (x, width) = region_axis("left, right and width", edges.left, edges.right, edges.width, self.size.width)?;
    let (y, height) = region_axis("top, bottom and height", edges.top, edges.bottom, edges.height, self.size.height)?;
    Ok(Self::new(self.origin.x + x, self.origin.y + y, width, height))
  }

  /// Whether `point` lies inside; edges count as inside.
  pub fn contains_point(self, point: Point) -> bool {
    point.x >= self.origin.x
      && point.y >= self.origin.y
      && point.x <= self.origin.x + self.size.width
      && point.y <= self.origin.y + self.size.height
  }

  /// Whether `other` lies wholly inside; edges count as inside.
  pub fn contains_rect(self, other: Rect) -> bool {
    self.contains_point(other.origin)
      && self.contains_point(Point::new(other.origin.x + other.size.width, other.origin.y + other.size.height))
  }

  /// The overlap of two rectangles, or `None` when they share no area
  /// (touching edges do not).
  pub fn intersect(self, other: Rect) -> Option<Self> {
    let x = self.origin.x.max(other.origin.x);
    let y = self.origin.y.max(other.origin.y);
    let width = (self.origin.x + self.size.width).min(other.origin.x + other.size.width) - x;
    let height = (self.origin.y + self.size.height).min(other.origin.y + other.size.height) - y;
    (width > 0.0 && height > 0.0).then(|| Self::new(x, y, width, height))
  }
}

/// A length along one axis of a rectangle: logical points, or a percentage
/// of that axis. A plain `f64` converts to points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Length {
  Points(f64),
  Percent(f64),
}

impl Length {
  fn resolve(self, total: f64) -> f64 {
    match self {
      Self::Points(points) => points,
      Self::Percent(percent) => total * percent / 100.0,
    }
  }
}

impl From<f64> for Length {
  fn from(points: f64) -> Self {
    Self::Points(points)
  }
}

/// Where a `Rect::region` box sits: distances from the parent's edges, and
/// sizes. Unset fields follow the rules of `Rect::region`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Edges {
  pub top: Option<Length>,
  pub right: Option<Length>,
  pub bottom: Option<Length>,
  pub left: Option<Length>,
  pub width: Option<Length>,
  pub height: Option<Length>,
}

/// Per-side amounts for `Rect::inset`. A plain `f64` insets every side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Insets {
  pub top: f64,
  pub right: f64,
  pub bottom: f64,
  pub left: f64,
}

impl From<f64> for Insets {
  fn from(by: f64) -> Self {
    Self {
      top: by,
      right: by,
      bottom: by,
      left: by,
    }
  }
}

/// One axis of `Rect::region` as `(offset, size)` inside `total`.
fn region_axis(names: &str, start: Option<Length>, end: Option<Length>, size: Option<Length>, total: f64) -> DriverResult<(f64, f64)> {
  let [start, end, size] = [start, end, size].map(|length| length.map(|length| length.resolve(total)));
  match (start, end, size) {
    (Some(_), Some(_), Some(_)) => Err(crate::DriverError::InvalidInput {
      message: format!("region: {names} over-constrain the box; give two of them"),
    }),
    (start, end, Some(size)) => Ok((start.unwrap_or_else(|| end.map_or(0.0, |end| total - end - size)), size)),
    (start, end, None) => {
      let start = start.unwrap_or(0.0);
      Ok((start, total - start - end.unwrap_or(0.0)))
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RelativeRect {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl RelativeRect {
  pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }

  /// Whether this is a finite, non-empty rectangle inside the unit square.
  pub fn is_normalized(&self) -> bool {
    [self.x, self.y, self.width, self.height].iter().all(|value| value.is_finite())
      && self.x >= 0.0
      && self.y >= 0.0
      && self.width > 0.0
      && self.height > 0.0
      && self.x + self.width <= 1.0
      && self.y + self.height <= 1.0
  }
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
}

#[cfg(test)]
#[path = "geometry_test.rs"]
mod tests;
