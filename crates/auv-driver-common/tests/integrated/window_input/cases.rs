use auv_driver_common::{
  ClickOptions, CoordinateSpace, DriverError, DriverResult, InputActionResult, InputPolicy, Rect, Scroll, ScrollOptions, Window,
  WindowInput, WindowPoint, WindowRef,
};

struct UnsupportedWindowInput;

impl WindowInput for UnsupportedWindowInput {
  fn click(&self, _window: &Window, _point: WindowPoint, _options: ClickOptions) -> DriverResult<InputActionResult> {
    Err(DriverError::unsupported("window.click"))
  }

  fn scroll(&self, _window: &Window, _point: WindowPoint, _scroll: Scroll, _options: ScrollOptions) -> DriverResult<InputActionResult> {
    Err(DriverError::unsupported("window.scroll"))
  }
}

fn fixture_window() -> Window {
  Window {
    reference: WindowRef {
      id: "fixture-window".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(100.0, 200.0, 800.0, 600.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  }
}

#[test]
fn unavailable_window_input_capabilities_are_explicit() {
  let adapter = UnsupportedWindowInput;
  let window = fixture_window();

  let click_error = adapter
    .click(
      &window,
      WindowPoint::new(20.0, 30.0),
      ClickOptions {
        policy: InputPolicy::BackgroundOnly,
        ..ClickOptions::default()
      },
    )
    .expect_err("an adapter without window click support should reject the operation");
  let scroll_error = adapter
    .scroll(&window, WindowPoint::new(20.0, 30.0), Scroll::new(0.0, -120.0), ScrollOptions::default())
    .expect_err("an adapter without window scroll support should reject the operation");

  assert_eq!(click_error.to_string(), "window.click is not supported by this driver");
  assert_eq!(scroll_error.to_string(), "window.scroll is not supported by this driver");
}
