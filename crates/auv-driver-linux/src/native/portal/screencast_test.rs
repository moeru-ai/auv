use super::*;

#[test]
fn stream_maps_global_point_to_local_point() {
  let stream = ScreenCastStream {
    id: 7,
    position: Some((100, 50)),
    size: Some((800, 600)),
    source_type: Some(SOURCE_MONITOR),
    mapping_id: None,
    pipewire_serial: None,
  };

  let point = stream.local_point(Point::new(120.0, 80.0)).expect("point maps into stream");

  assert_eq!(point, Point::new(20.0, 30.0));
}

#[test]
fn stream_rejects_outside_point() {
  let stream = ScreenCastStream {
    id: 7,
    position: Some((100, 50)),
    size: Some((800, 600)),
    source_type: Some(SOURCE_MONITOR),
    mapping_id: None,
    pipewire_serial: None,
  };

  assert!(stream.local_point(Point::new(50.0, 80.0)).is_err());
}

#[test]
fn bgrx_pixel_converts_to_rgba() {
  let mut dest = [0, 0, 0, 0];

  write_rgba_pixel(spa::param::video::VideoFormat::BGRx, &[3, 2, 1, 0], &mut dest).expect("BGRx converts");

  assert_eq!(dest, [1, 2, 3, 255]);
}

#[test]
fn xrgb_pixel_converts_to_rgba() {
  let mut dest = [0, 0, 0, 0];

  write_rgba_pixel(spa::param::video::VideoFormat::xRGB, &[0, 1, 2, 3], &mut dest).expect("xRGB converts");

  assert_eq!(dest, [1, 2, 3, 255]);
}

#[test]
fn window_stream_prefers_window_source_type() {
  let monitor = ScreenCastStream {
    id: 1,
    position: None,
    size: None,
    source_type: Some(SOURCE_MONITOR),
    mapping_id: None,
    pipewire_serial: None,
  };
  let window = ScreenCastStream {
    id: 2,
    position: None,
    size: None,
    source_type: Some(SOURCE_WINDOW),
    mapping_id: None,
    pipewire_serial: None,
  };

  let streams = [monitor, window];
  let selected = select_window_stream(&streams).expect("stream selected");

  assert_eq!(selected.id, 2);
}
