use super::*;

fn response(width: i64, height: i64, bytes: Vec<u8>) -> NativeImageCaptureResponse {
  NativeImageCaptureResponse {
    image_width: width,
    image_height: height,
    window_x: 10.,
    window_y: 20.,
    window_width: 30.,
    window_height: 40.,
    rgba_bytes: bytes,
    error_message: None,
    recovery_hint: None,
  }
}

#[test]
fn shared_image_envelope_preserves_exact_pixels_and_rejects_invalid_dimensions() {
  let image = decode_capture_response(response(1, 1, vec![10, 20, 30, 255])).unwrap();
  assert_eq!(image.rgba_bytes, [10, 20, 30, 255]);
  assert_eq!(image.window_frame, auv_driver_common::Rect::new(10., 20., 30., 40.));
  for (width, height) in [(0, 1), (-1, -1), (i64::MAX, 5)] {
    assert!(decode_capture_response(response(width, height, vec![])).is_err());
  }
  assert!(decode_capture_response(response(1, 1, vec![1, 2, 3])).is_err());
}

#[test]
fn native_capture_errors_preserve_recovery_and_do_not_become_empty_images() {
  let mut value = response(0, 0, vec![]);
  value.error_message = Some("capture exclusion failed".into());
  value.recovery_hint = Some("grant Screen Recording".into());
  let error = decode_capture_response(value).unwrap_err();
  assert!(error.contains("capture exclusion failed"));
  assert!(error.contains("grant Screen Recording"));
}
