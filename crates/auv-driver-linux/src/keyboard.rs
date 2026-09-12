//! Foreground batch validation and delivery, shared by invoke and Runner.
use crate::{
  InputApi,
  error::invalid_input,
  input::{keysym, with_input_session},
};
use auv_driver_common::{
  DriverError, DriverResult, InputActionResult, InputPolicy, InputTarget, KeyboardInput, KeyboardInputError, KeyboardInputProgress,
  PressKeysOptions,
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

impl InputApi<'_> {
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
    // TODO: targeted Linux keyboard input requires recipient preparation and
    // identity validation. Enable it when that driver capability is implemented.
    if !matches!(target, InputTarget::Foreground) {
      return Err(failure(DriverError::unsupported("Linux targeted keyboard input"), 0, vec![], 0));
    }
    let mut combinations = Vec::with_capacity(inputs.len());
    let mut symbols = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
      if input.policy() != InputPolicy::ForegroundPreferred {
        return Err(failure(invalid_input("background keyboard input requires supported targeted delivery"), index, vec![], 0));
      }
      let keys = match input {
        KeyboardInput::PressKeys { options, .. } => combination(options),
        KeyboardInput::TypeText { text, .. } => text.chars().map(keysym::for_char).collect(),
        KeyboardInput::PasteText { .. } => Ok(vec![]),
      }
      .map_err(|cause| failure(cause, index, vec![], 0))?;
      let mut required = keys.clone();
      match input {
        KeyboardInput::TypeText { options, .. } => {
          if options.replace_existing {
            required.extend([keysym::CONTROL_L, 'a' as i32, keysym::BACKSPACE]);
          }
          if options.submit != auv_driver_common::TextSubmit::No {
            required.push(keysym::RETURN);
          }
        }
        KeyboardInput::PasteText { options, .. } => {
          required.extend([keysym::CONTROL_L, 'v' as i32]);
          if options.replace_existing {
            required.push('a' as i32);
          }
          if options.submit != auv_driver_common::TextSubmit::No {
            required.push(keysym::RETURN);
          }
        }
        _ => {}
      }
      symbols.push(required);
      combinations.push(keys);
    }
    if dry_run {
      return Ok(None);
    }
    // Backend-specific layout validation also covers the complete batch before
    // the first event. Creating a session may request permission but sends no input.
    for (index, keys) in symbols.iter().enumerate() {
      with_input_session(&self.session.state, |session| session.validate_keys(keys)).map_err(|cause| failure(cause, index, vec![], 0))?;
    }
    let mut completed = Vec::new();
    for (index, (input, keys)) in inputs.into_iter().zip(combinations).enumerate() {
      let (count, interval, settle) = match &input {
        KeyboardInput::PressKeys { options, .. } => (options.count, options.interval, options.settle),
        _ => (1, Duration::ZERO, Duration::ZERO),
      };
      let mut combined: Option<InputActionResult> = None;
      for repetition in 0..count {
        if repetition > 0 {
          std::thread::sleep(interval);
        }
        let outcome = match &input {
          KeyboardInput::PressKeys { .. } => {
            let (key, held) = keys.split_last().expect("validated nonempty combination");
            with_input_session(&self.session.state, |session| session.key_chord(held, *key)).map(|()| crate::input::keyboard_result())
          }
          KeyboardInput::TypeText { text, options } => self.type_text(text, *options),
          KeyboardInput::PasteText { options, .. } => self.paste_text(options.clone()),
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
      completed.push(combined.expect("positive count"));
    }
    Ok(Some(completed))
  }
}
