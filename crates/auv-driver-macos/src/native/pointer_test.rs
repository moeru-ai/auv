#[cfg(target_os = "macos")]
use super::action_result;
#[cfg(target_os = "macos")]
use crate::native::binding::ffi::NativeActionResponse;

#[cfg(target_os = "macos")]
#[test]
fn click_modifiers_map_to_quartz_mouse_event_flags() {
  use auv_driver_common::ClickModifiers;
  // CoreGraphics/CGEventTypes.h: Shift 17, Control 18, Alternate 19, Command 20.
  assert_eq!(
    super::click_flags(ClickModifiers {
      shift: true,
      ..Default::default()
    }),
    1 << 17
  );
  assert_eq!(
    super::click_flags(ClickModifiers {
      control: true,
      ..Default::default()
    }),
    1 << 18
  );
  assert_eq!(
    super::click_flags(ClickModifiers {
      alt: true,
      ..Default::default()
    }),
    1 << 19
  );
  assert_eq!(
    super::click_flags(ClickModifiers {
      meta: true,
      ..Default::default()
    }),
    1 << 20
  );
  assert_eq!(super::click_flags(ClickModifiers::default()), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn action_result_includes_operation_name() {
  let error = action_result(
    "click_point",
    NativeActionResponse {
      ok: false,
      error_message: Some("event creation failed".to_string()),
      recovery_hint: Some("grant Accessibility permission".to_string()),
    },
  )
  .unwrap_err();

  assert!(error.contains("click_point"));
  assert!(error.contains("event creation failed"));
}
