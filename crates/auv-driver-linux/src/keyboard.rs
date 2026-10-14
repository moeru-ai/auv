//! Foreground batch validation and delivery, shared by invoke and Runner.
use crate::{
  InputApi,
  error::invalid_input,
  input::{KeyboardPlan, keysym, with_input_session},
};
use auv_driver_common::{
  DriverError, DriverResult, InputActionResult, InputDeliveryPath, InputPolicy, InputTarget, KeyboardInput, KeyboardInputError,
  KeyboardInputProgress, PressKeysOptions,
};
use std::time::Duration;

fn failure(cause: DriverError, action_index: usize, completed: Vec<InputActionResult>, completed_presses: u32) -> KeyboardInputError {
  KeyboardInputError {
    cause,
    progress: KeyboardInputProgress {
      action_index,
      completed,
      completed_presses,
    },
  }
}

/// Resolve every key before emitting any action; '+' is a literal list item.
pub(crate) fn combination(options: &PressKeysOptions) -> DriverResult<Vec<i32>> {
  if options.keys.is_empty() || !(1..=255).contains(&options.count) || (options.count > 1 && options.interval.is_zero()) {
    return Err(invalid_input("keys must not be empty; count must be 1..255 and repeated presses require a positive interval"));
  }
  let mut keys = Vec::new();
  let mut ordinary = false;
  for raw in &options.keys {
    let raw = raw.trim();
    let modifier = keysym::modifier(raw);
    if ordinary && modifier.is_some() {
      return Err(invalid_input("modifiers must precede ordinary keys"));
    }
    ordinary |= modifier.is_none();
    let key = keysym::named_or_char(raw)?;
    if keys.contains(&key) {
      return Err(invalid_input("a combination cannot contain duplicate keys"));
    }
    keys.push(key);
  }
  Ok(keys)
}

/// Fails unless the target window (or a window of the target application) is
/// the AT-SPI active window.
///
/// NOTICE: Wayland gives clients no way to send keys to a chosen window or to
/// raise one (AT-SPI GrabFocus does not move keyboard focus between top-level
/// windows on GNOME). A target that is not focused is refused instead of
/// typing into whatever is in front.
fn ensure_target_focused(target: &InputTarget) -> DriverResult<()> {
  let active = crate::atspi::active_window()?;
  if target_is_focused(target, active.as_ref()) {
    return Ok(());
  }
  let active = active.and_then(|window| window.title.or(window.app_name)).unwrap_or_else(|| "no window".to_string());
  Err(invalid_input(format!(
    "the keyboard target does not have keyboard focus and Wayland cannot focus it; focus it first (focused: {active})"
  )))
}

/// Whether `active` is the target window or a window of the target
/// application. An application id is the AT-SPI AccessibleId `window.list`
/// reports; the application name is accepted for apps without one.
pub(crate) fn target_is_focused(target: &InputTarget, active: Option<&auv_driver_common::Window>) -> bool {
  match (target, active) {
    (InputTarget::Window(window), Some(active)) => active.reference == window.reference,
    (InputTarget::Application { bundle_id }, Some(active)) => {
      active.app_bundle_id.as_deref() == Some(bundle_id.as_str()) || active.app_name.as_deref() == Some(bundle_id.as_str())
    }
    _ => false,
  }
}

impl InputApi<'_> {
  pub fn key_down(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    timeout: Duration,
  ) -> DriverResult<auv_driver_common::KeyboardHold> {
    // TODO(linux-targeted-held-keys): held keys could use the same focused-target
    // check as `input_keyboard`, but a hold outlives the check while focus can
    // move. Enable it with a focus re-check when a workflow needs targeted holds.
    if !matches!(target, InputTarget::Foreground) || policy != InputPolicy::ForegroundPreferred {
      return Err(DriverError::unsupported("Linux targeted keyboard input"));
    }
    let symbols = combination(&PressKeysOptions {
      keys,
      ..Default::default()
    })?;
    let backend = crate::input::held_keyboard_backend(&self.session.state, &symbols)?;
    let controller = auv_driver_common::keyboard_hold_controller().clone();
    controller.down(backend, timeout)
  }

  pub fn key_up(&self, hold: auv_driver_common::KeyboardHoldId) -> DriverResult<InputActionResult> {
    auv_driver_common::keyboard_hold_controller().up(hold)
  }

  pub fn hold_keys(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    duration: Duration,
  ) -> DriverResult<InputActionResult> {
    // Give this call time to release before the controller's fallback deadline.
    let mut hold = self.key_down(target, keys, policy, duration.saturating_add(Duration::from_secs(1)))?;
    hold.wait_and_release(duration)
  }

  /// Validate an entire foreground batch before delivery. Errors retain completed
  /// actions and repetitions; submitted events never establish semantic success.
  pub fn input_keyboard(
    &self,
    target: &InputTarget,
    inputs: Vec<KeyboardInput>,
    dry_run: bool,
  ) -> Result<Option<Vec<InputActionResult>>, KeyboardInputError> {
    if inputs.is_empty() {
      return Err(failure(invalid_input("keyboard input requires at least one action"), 0, vec![], 0));
    }
    // Wayland cannot address keys to one window or raise another one, so a
    // window or application target is served only while it has keyboard focus
    // (checked before each action, after validation); every result records the
    // foreground fallback.
    let targeted = !matches!(target, InputTarget::Foreground);
    let plans = inputs
      .iter()
      .enumerate()
      .map(|(index, input)| {
        if input.policy() != InputPolicy::ForegroundPreferred {
          return Err(failure(invalid_input("background keyboard input requires supported targeted delivery"), index, vec![], 0));
        }
        match input {
          KeyboardInput::PressKeys { options, .. } => combination(options).map(KeyboardPlan::chord),
          KeyboardInput::TypeText { text, options } if !crate::input::keysym_typeable(text) => {
            Ok(KeyboardPlan::paste_text(&crate::input::paste_options_for_text(text, *options), crate::input::focused_terminal()))
          }
          KeyboardInput::TypeText { text, options } => KeyboardPlan::type_text(text, *options),
          // NOTICE: the paste shortcut follows the window focused while the batch
          // is planned, before earlier steps (such as an app switch) run.
          KeyboardInput::PasteText { options, .. } => Ok(KeyboardPlan::paste_text(options, crate::input::focused_terminal())),
        }
        .map_err(|cause| failure(cause, index, vec![], 0))
      })
      .collect::<Result<Vec<_>, _>>()?;
    if dry_run {
      return Ok(None);
    }
    // Load one layout for the complete batch. Keep this snapshot through
    // clipboard operations too, even though those release the session lock.
    let mut invalid_index = 0;
    let layout = with_input_session(&self.session.state, |session| {
      let layout = session.keyboard_layout()?;
      for (index, plan) in plans.iter().enumerate() {
        invalid_index = index;
        session.validate_keyboard(&layout, plan)?;
      }
      Ok(layout)
    })
    .map_err(|cause| failure(cause, invalid_index, vec![], 0))?;
    let mut completed = Vec::new();
    for (index, (input, plan)) in inputs.into_iter().zip(plans).enumerate() {
      let (count, interval, settle) = match &input {
        KeyboardInput::PressKeys { options, .. } => (options.count, options.interval, options.settle),
        // The paste path already waits `settle` for non-ASCII text.
        KeyboardInput::TypeText { text, .. } if !crate::input::keysym_typeable(text) => (1, Duration::ZERO, Duration::ZERO),
        KeyboardInput::TypeText { options, .. } => (1, Duration::ZERO, options.settle),
        KeyboardInput::PasteText { .. } => (1, Duration::ZERO, Duration::ZERO),
      };
      if targeted {
        ensure_target_focused(target).map_err(|cause| failure(cause, index, completed.clone(), 0))?;
      }
      let mut combined: Option<InputActionResult> = None;
      for repetition in 0..count {
        if repetition > 0 {
          std::thread::sleep(interval);
        }
        let outcome = match &input {
          KeyboardInput::TypeText { text, options } if !crate::input::keysym_typeable(text) => {
            crate::input::paste_prepared(&self.session.state, crate::input::paste_options_for_text(text, *options), &layout, &plan)
              .map(crate::input::typed_through_paste)
          }
          KeyboardInput::PressKeys { .. } | KeyboardInput::TypeText { .. } => {
            with_input_session(&self.session.state, |session| session.deliver_keyboard(&layout, &plan))
              .map(|()| crate::input::keyboard_result())
          }
          KeyboardInput::PasteText { options, .. } => crate::input::paste_prepared(&self.session.state, options.clone(), &layout, &plan),
        };
        let action = outcome.map_err(|cause| failure(cause, index, completed.clone(), repetition))?;
        if let Some(combined) = &mut combined {
          combined.attempts.extend(action.attempts);
        } else {
          combined = Some(action);
        }
      }
      if !settle.is_zero() {
        std::thread::sleep(settle);
      }
      let mut combined = combined.expect("positive count");
      if targeted {
        crate::session::add_foreground_window_fallback_reason(
          &mut combined,
          InputDeliveryPath::WindowTargetedKeyboard,
          "Wayland cannot deliver keys to one window; the target has keyboard focus, so keys went to the foreground",
        );
      }
      completed.push(combined);
    }
    Ok(Some(completed))
  }
}
