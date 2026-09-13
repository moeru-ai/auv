use crate::LinuxDriver;
use auv_driver_common::{Driver, InputPolicy, InputTarget, KeyboardInput, PressKeysOptions};

#[test]
fn invalid_tail_is_rejected_before_delivering_the_prefix() {
  // ROOT CAUSE: the invoke batch contract was not connected to the Linux driver.
  // Validate a malformed tail before any backend or desktop access.
  let session = LinuxDriver::new().open_local().unwrap();
  let action = |keys: &[&str]| KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: keys.iter().map(|key| (*key).into()).collect(),
      ..Default::default()
    },
    policy: InputPolicy::ForegroundPreferred,
  };
  let error = session.input().input_keyboard(&InputTarget::Foreground, vec![action(&["a"]), action(&["invalid-key"])], false).unwrap_err();
  assert_eq!(error.progress.action_index, 1);
  assert!(error.progress.completed.is_empty());
  assert_eq!(error.progress.completed_presses, 0);
  assert!(matches!(error.cause, auv_driver_common::DriverError::InvalidInput { .. }));
}

#[test]
fn dry_run_accepts_literal_plus_and_repeated_chords_without_a_desktop() {
  let session = LinuxDriver::new().open_local().unwrap();
  let result = session
    .input()
    .input_keyboard(
      &InputTarget::Foreground,
      vec![KeyboardInput::PressKeys {
        options: PressKeysOptions {
          keys: vec!["ctrl".into(), "+".into()],
          count: 2,
          interval: std::time::Duration::from_millis(10),
          ..Default::default()
        },
        policy: InputPolicy::ForegroundPreferred,
      }],
      true,
    )
    .unwrap();
  assert!(result.is_none());
}
