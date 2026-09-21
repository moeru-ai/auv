//! Position encoding and validation shared by Runner clients and servers.

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_driver::{CoordinateSpace, Point, Position};

/// A wire position that cannot identify a finite point in an explicit space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
  /// At least one coordinate is NaN or infinite.
  #[error("position coordinates must be finite")]
  NonFiniteCoordinates,
  /// The space is missing, the screen flag is false, or a resource ID is empty.
  #[error("position requires an explicit coordinate space and nonempty resource id")]
  InvalidCoordinateSpace,
}

/// Encodes the point and exact coordinate-space identity without validation.
/// Receivers validate wire values with [`decode`]; this performs no resource IO.
pub fn encode(position: Position) -> proto::Position {
  use proto::position::CoordinateSpace as Space;
  proto::Position {
    x: position.point.x,
    y: position.point.y,
    coordinate_space: Some(match position.coordinate_space {
      CoordinateSpace::Screen => Space::Screen(true),
      CoordinateSpace::Display(id) => Space::DisplayId(id),
      CoordinateSpace::Window(id) => Space::WindowId(id),
    }),
  }
}

/// Decodes a finite logical point with an explicit space and nonempty resource
/// ID where applicable. Does not resolve resources or check freshness or
/// actionability. Missing optional positions are handled by the owning message.
pub fn decode(position: proto::Position) -> Result<Position, DecodeError> {
  use proto::position::CoordinateSpace as Space;
  if !position.x.is_finite() || !position.y.is_finite() {
    return Err(DecodeError::NonFiniteCoordinates);
  }
  let coordinate_space = match position.coordinate_space {
    Some(Space::Screen(true)) => CoordinateSpace::Screen,
    Some(Space::DisplayId(id)) if !id.is_empty() => CoordinateSpace::Display(id),
    Some(Space::WindowId(id)) if !id.is_empty() => CoordinateSpace::Window(id),
    _ => return Err(DecodeError::InvalidCoordinateSpace),
  };
  Ok(Position {
    point: Point::new(position.x, position.y),
    coordinate_space,
  })
}

#[cfg(test)]
#[path = "position_test.rs"]
mod tests;
