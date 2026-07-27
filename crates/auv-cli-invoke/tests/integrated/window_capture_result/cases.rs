use auv_cli_invoke::InvokeCommandOutput;
use auv_cli_invoke::commands::window::{WindowCapture, window_capture_result};
use auv_driver::{Capture, CoordinateSpace, Rect, Window, WindowRef};
use image::RgbaImage;

#[test]
fn pixels_are_excluded_from_the_public_window_capture_result() {
  let capture = WindowCapture {
    window: Window {
      reference: WindowRef {
        id: "window_capture".to_string(),
      },
      title: Some("Fixture".to_string()),
      app_name: Some("Fixture App".to_string()),
      app_bundle_id: Some("com.example.Fixture".to_string()),
      process_id: Some(42),
      frame: Rect::new(10.0, 20.0, 640.0, 480.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    capture: Capture {
      image: RgbaImage::new(1280, 960),
      bounds: Rect::new(10.0, 20.0, 640.0, 480.0),
      scale_factor: 2.0,
      backend: "fixture-window".to_string(),
      fallback_reason: None,
    },
  };

  let output = InvokeCommandOutput::from_result(&window_capture_result(&capture)).expect("window capture result should serialize");
  let result = output.result().expect("capture should have a result");

  assert_eq!(result["window"]["reference"]["id"], "window_capture");
  assert_eq!(result["capture"]["pixel_dimensions"]["width"], 1280);
  assert_eq!(result["capture"]["backend"], "fixture-window");
  assert!(result["capture"].get("image").is_none());
}
