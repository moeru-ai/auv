//! Foreground batch validation and delivery, shared by invoke and Runner.
use crate::{
  InputApi,
  error::invalid_input,
  input::{KeyboardPlan, keysym, with_input_session},
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
    let plans = inputs
      .iter()
      .enumerate()
      .map(|(index, input)| {
        if input.policy() != InputPolicy::ForegroundPreferred {
          return Err(failure(invalid_input("background keyboard input requires supported targeted delivery"), index, vec![], 0));
        }
        match input {
          KeyboardInput::PressKeys { options, .. } => combination(options).map(KeyboardPlan::chord),
          KeyboardInput::TypeText { text, options } => KeyboardPlan::type_text(text, *options),
          KeyboardInput::PasteText { options, .. } => Ok(KeyboardPlan::paste_text(options)),
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
        KeyboardInput::TypeText { options, .. } => (1, Duration::ZERO, options.settle),
        KeyboardInput::PasteText { .. } => (1, Duration::ZERO, Duration::ZERO),
      };
      let mut combined: Option<InputActionResult> = None;
      for repetition in 0..count {
        if repetition > 0 {
          std::thread::sleep(interval);
        }
        let outcome = match &input {
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
      completed.push(combined.expect("positive count"));
    }
    Ok(Some(completed))
  }
}
