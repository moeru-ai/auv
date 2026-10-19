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
fn click_modifiers_release_after_success_and_delivery_failure() {
  for fail_click in [false, true] {
    let events = std::cell::RefCell::new(Vec::new());
    let result = with_held_keys(
      &[0xffe1, 0xffe3],
      |key, pressed| {
        events.borrow_mut().push((key, pressed));
        Ok(())
      },
      || {
        events.borrow_mut().push((0x110, true));
        if fail_click {
          Err(backend("click failed"))
        } else {
          Ok(())
        }
      },
    );
    assert_eq!(result.is_err(), fail_click);
    assert_eq!(
      *events.borrow(),
      [
        (0xffe1, true),
        (0xffe3, true),
        (0x110, true),
        (0xffe3, false),
        (0xffe1, false)
      ]
    );
  }
}

#[test]
fn click_modifiers_release_uncertain_press_and_continue_after_release_failure() {
  let mut events = Vec::new();
  let result = with_held_keys(
    &[0xffe1, 0xffe3, 0xffe9],
    |key, pressed| {
      events.push((key, pressed));
      if key == 0xffe3 {
        Err(backend(if pressed {
          "press reply lost"
        } else {
          "release reply lost"
        }))
      } else {
        Ok(())
      }
    },
    || panic!("click must not run after a failed modifier press"),
  );
  assert_eq!(
    events,
    [
      (0xffe1, true),
      (0xffe3, true),
      (0xffe3, false),
      (0xffe1, false)
    ]
  );
  let error = result.unwrap_err().to_string();
  assert!(error.contains("press reply lost"));
  assert!(error.contains("release reply lost"));
}

#[test]
fn click_modifiers_report_release_failure_after_successful_click() {
  let result = with_held_keys(
    &[0xffe1],
    |_, pressed| {
      if pressed {
        Ok(())
      } else {
        Err(backend("release failed"))
      }
    },
    || Ok(()),
  );
  assert!(result.unwrap_err().to_string().contains("release failed"));
}
