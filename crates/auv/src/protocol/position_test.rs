use super::*;
use proto::position::CoordinateSpace as Space;

#[test]
fn roundtrip_preserves_coordinates_and_exact_space_identity() {
  for coordinate_space in [
    CoordinateSpace::Screen,
    CoordinateSpace::Display("display-1".into()),
    CoordinateSpace::Window("window-1".into()),
  ] {
    let position = Position {
      point: Point::new(-200.0, 300.0),
      coordinate_space,
    };
    assert_eq!(decode(encode(position.clone())).unwrap(), position);
  }
}

#[test]
fn rejects_missing_false_or_empty_coordinate_space() {
  for space in [
    None,
    Some(Space::Screen(false)),
    Some(Space::WindowId(String::new())),
    Some(Space::DisplayId(String::new())),
  ] {
    assert_eq!(
      decode(proto::Position {
        coordinate_space: space,
        ..Default::default()
      }),
      Err(DecodeError::InvalidCoordinateSpace)
    );
  }
}

#[test]
fn rejects_nonfinite_coordinates_on_either_axis() {
  for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
    for (x, y) in [(invalid, 0.0), (0.0, invalid)] {
      assert_eq!(
        decode(proto::Position {
          x,
          y,
          coordinate_space: Some(Space::Screen(true))
        }),
        Err(DecodeError::NonFiniteCoordinates)
      );
    }
  }
}
