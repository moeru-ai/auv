use super::*;

// https://github.com/moeru-ai/auv/actions/runs/30574130256/job/90978006386
#[test]
fn ci_90978006386_click_parts_support_repeated_clicks() {
  // ROOT CAUSE:
  //
  // If Click::Repeated was passed to the Linux portal driver, the crate failed
  // to compile because click_at still matched only the older single/double shape.
  //
  // Before the fix, Ubuntu CI stopped at a non-exhaustive match.
  // The fix keeps every shared Click variant mapped to portal count/interval values.
  assert_eq!(
    click_parts(&Click::Repeated {
      count: 3,
      interval: Duration::from_millis(60),
    })
    .expect("repeated click"),
    (3, Duration::from_millis(60))
  );
  assert!(matches!(
    click_parts(&Click::Repeated {
      count: 0,
      interval: Duration::from_millis(60),
    }),
    Err(auv_driver_common::error::DriverError::InvalidInput { message })
      if message == "repeated click count must be greater than zero"
  ));
}

#[test]
fn device_mask_requests_keyboard_and_pointer() {
  assert_eq!(DEVICE_KEYBOARD | DEVICE_POINTER, 3);
}

#[test]
fn evdev_button_codes_match_primary_buttons() {
  assert_eq!(BUTTON_LEFT, 0x110);
  assert_eq!(BUTTON_RIGHT, 0x111);
  assert_eq!(BUTTON_MIDDLE, 0x112);
}

#[test]
fn output_mapping_scales_logical_screen_point_for_remote_desktop_motion() {
  let display = display(Rect::new(0.0, 0.0, 2752.0, 1152.0), 1.25);
  let stream = stream(7, Rect::new(0.0, 0.0, 2752.0, 1152.0));

  let mapping = output_mapping(&display, &[stream]).expect("display maps to stream");

  assert_eq!(mapping.to_motion_target(Point::new(1477.0, 804.0)), MotionTarget::absolute(7, Point::new(1846.25, 1005.0)));
}

#[test]
fn output_mapping_clamps_absolute_motion_and_keeps_remaining_delta() {
  let display = display(Rect::new(0.0, 0.0, 2752.0, 1152.0), 1.25);
  let stream = stream(7, Rect::new(0.0, 0.0, 2752.0, 1152.0));

  let mapping = output_mapping(&display, &[stream]).expect("display maps to stream");

  assert_eq!(
    mapping.to_motion_target(Point::new(1477.0, 1096.0)),
    MotionTarget {
      stream_id: 7,
      absolute_point: Point::new(1846.25, 1151.0),
      relative_delta: Point::new(0.0, 175.20000000000005),
    }
  );
}

#[test]
fn output_mapping_rejects_ambiguous_logical_stream_rects() {
  let display = display(Rect::new(0.0, 0.0, 1000.0, 800.0), 1.0);
  let streams = [
    stream(1, Rect::new(0.0, 0.0, 1000.0, 800.0)),
    stream(2, Rect::new(0.0, 0.0, 1000.0, 800.0)),
  ];

  assert_eq!(output_mapping(&display, &streams), None);
}

#[test]
fn capture_pixel_scale_does_not_change_portal_logical_motion_coordinates() {
  let display = display(Rect::new(0.0, 0.0, 2560.0, 1440.0), 1.0);
  let stream = stream(86, Rect::new(0.0, 0.0, 2560.0, 1440.0));

  let mapping = output_mapping(&display, &[stream]).expect("display maps to stream");

  assert_eq!(mapping.to_motion_target(Point::new(1332.0, 913.0)), MotionTarget::absolute(86, Point::new(1332.0, 913.0)));
}

fn display(frame: Rect, scale_factor: f64) -> Display {
  Display {
    id: "display".to_string(),
    name: None,
    frame,
    coordinate_space: auv_driver_common::geometry::CoordinateSpace::Screen,
    scale_factor,
    is_primary: true,
    is_builtin: None,
  }
}

fn stream(id: u32, rect: Rect) -> ScreenCastStream {
  ScreenCastStream {
    id,
    position: Some((rect.origin.x as i32, rect.origin.y as i32)),
    size: Some((rect.size.width as i32, rect.size.height as i32)),
    source_type: None,
    mapping_id: None,
    pipewire_serial: None,
  }
}
