#[cfg(target_os = "macos")]
use super::action_result;
#[cfg(target_os = "macos")]
use crate::native::binding::ffi::NativeActionResponse;

#[cfg(target_os = "macos")]
#[test]
fn action_result_includes_operation_name() {
  let error = action_result(
    "type_text_in_window",
    NativeActionResponse {
      ok: false,
      error_message: Some("failed to create keyboard event".to_string()),
      recovery_hint: Some("grant Accessibility permission".to_string()),
    },
  )
  .unwrap_err();

  assert!(error.contains("type_text_in_window"));
  assert!(error.contains("failed to create keyboard event"));
}

// Test replacement at the native event-delivery boundary. Thread-local state
// prevents parallel tests from capturing another test's real input.
#[derive(Default)]
pub(crate) struct KeyCombinationRecorder {
  pub calls: Vec<(Option<(i64, i64)>, Vec<i32>)>,
  pub fail_on: Option<usize>,
}
thread_local! {
  pub(crate) static KEY_COMBINATIONS: std::cell::RefCell<Option<KeyCombinationRecorder>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn record_combination(target: Option<(i64, i64)>, keys: &[i32]) -> Option<super::AuvResult<()>> {
  KEY_COMBINATIONS.with_borrow_mut(|recorder| {
    let recorder = recorder.as_mut()?;
    let index = recorder.calls.len();
    recorder.calls.push((target, keys.to_vec()));
    Some(if recorder.fail_on == Some(index) {
      Err("injected native event creation failure".into())
    } else {
      Ok(())
    })
  })
}

pub(crate) fn with_combination_recorder<T>(fail_on: Option<usize>, run: impl FnOnce() -> T) -> (T, KeyCombinationRecorder) {
  struct Reset;
  impl Drop for Reset {
    fn drop(&mut self) {
      KEY_COMBINATIONS.with_borrow_mut(|state| {
        *state = None;
      });
    }
  }
  KEY_COMBINATIONS.with_borrow_mut(|state| {
    assert!(state.is_none());
    *state = Some(KeyCombinationRecorder {
      fail_on,
      ..Default::default()
    });
  });
  let _reset = Reset;
  let result = run();
  let recorder = KEY_COMBINATIONS.with_borrow_mut(|state| state.take().unwrap());
  (result, recorder)
}
