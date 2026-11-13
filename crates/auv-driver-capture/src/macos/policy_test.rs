use super::*;

// ROOT CAUSE:
//
// If Mission Control showed the target window, ScreenCaptureKit captured it
// at its shrunk frame (955x558 pt for a 1644x960 pt window) and the driver
// derived `scale_factor` from the resolved frame instead, returning a black
// image with scale 1.17.
//
// The fix compares the captured frame with the resolved one, and asks the
// application (AX) whether a mismatch is a real resize.
#[test]
fn captured_frame_is_transformed_while_the_application_keeps_its_size() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1644.0, 960.0),
      minimized: false,
    })
  };

  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 960.0), Size::new(955.0, 558.0), ax),
    CapturedWindowFrame::Transformed { minimized: false }
  );
}

#[test]
fn captured_frame_is_a_resize_when_the_application_agrees_with_it() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1056.0, 752.0),
      minimized: false,
    })
  };

  assert_eq!(classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1056.0, 752.0), ax), CapturedWindowFrame::Resized);
}

#[test]
fn captured_frame_of_a_minimized_window_is_transformed() {
  let ax = || {
    Some(WindowAxSize {
      size: Size::new(1644.0, 965.0),
      minimized: true,
    })
  };

  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 965.0), Size::new(101.0, 45.0), ax),
    CapturedWindowFrame::Transformed { minimized: true }
  );
}

#[test]
fn captured_frame_matches_up_to_rounding_without_asking_the_application() {
  let ax = || -> Option<WindowAxSize> { panic!("AX is only consulted on a mismatch") };

  assert_eq!(classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1644.5, 959.5), ax), CapturedWindowFrame::Resolved);
  assert_eq!(classify_captured_frame(Size::new(0.0, 0.0), Size::new(1644.0, 960.0), ax), CapturedWindowFrame::Resolved);
}

#[test]
fn captured_frame_without_application_size_is_treated_as_transformed() {
  assert_eq!(
    classify_captured_frame(Size::new(1644.0, 960.0), Size::new(1056.0, 752.0), || None),
    CapturedWindowFrame::Transformed { minimized: false }
  );
}
