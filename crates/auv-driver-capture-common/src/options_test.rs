use super::*;

#[test]
fn standalone_display_capture_rejects_region_before_native_capture() {
  let error = CaptureOptions {
    region: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
    ..Default::default()
  }
  .validate_display()
  .unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert_eq!(error.to_string(), "display.capture does not accept window or region capture options");
}

#[test]
fn standalone_region_capture_requires_region_before_native_capture() {
  let error = CaptureOptions::default().validate_region().unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert_eq!(error.to_string(), "display.capture_region requires CaptureOptions.region");
}

#[test]
fn standalone_capture_rejects_activation_without_an_application_target() {
  let error = CaptureOptions {
    activation: Activation::ActivateFirst {
      settle: std::time::Duration::ZERO,
    },
    ..Default::default()
  }
  .validate_display()
  .unwrap_err();

  assert!(matches!(error, auv_driver_common::DriverError::InvalidInput { .. }));
  assert_eq!(error.to_string(), "display.capture cannot activate an application without an application target");
}
