use crate::{
  X11DriverSession,
  session::{backend, invalid},
};
use auv_driver_common::{
  Click, ClickModifiers, DisturbanceLevel, DriverError, DriverResult, InputActionResult, InputDeliveryPath, InputPolicy, InputTarget,
  KeyPressOptions, KeyboardBackend, KeyboardHoldIdentity, KeyboardInput, KeyboardInputError, KeyboardInputProgress, Point, PressKeysOptions,
  Scroll, TextSubmit, TypeTextOptions, input::MouseButton, mouse_input::MouseBackend,
};
use enigo::{Axis, Button, Coordinate, Direction, Key, Keyboard, Mouse};
use std::{sync::Arc, time::Duration};

/// Serialized foreground XTEST input. No operation retries after delivery.
#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  pub(crate) session: &'a X11DriverSession,
}

impl InputApi<'_> {
  pub fn key_down(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    timeout: Duration,
  ) -> DriverResult<auv_driver_common::KeyboardHold> {
    validate_foreground(target, policy, "X11 targeted keyboard input")?;
    let keys = parse_keys(&PressKeysOptions {
      keys,
      ..Default::default()
    })?;
    // Enigo maps Key through this X keysym before choosing an X11 keycode.
    // The session pins DISPLAY/XAUTHORITY process-wide, so all independent
    // holds on this controller have one native route. Alias spellings that
    // resolve to the same keysym conflict before XTEST delivery.
    let identity = KeyboardHoldIdentity::new("x11-pinned-display", keys.iter().map(|key| format!("{}", xkeysym::Keysym::from(*key).raw())));
    let backend = Arc::new(HeldKeyboardBackend {
      session: self.session.clone(),
      keys,
    });
    auv_driver_common::keyboard_hold_controller().clone().down_independent(backend, timeout, identity)
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
    let mut hold = self.key_down(target, keys, policy, duration.saturating_add(Duration::from_secs(1)))?;
    hold.wait_and_release(duration)
  }

  /// Validates a complete foreground batch before delivering its first event.
  pub fn input_keyboard(
    &self,
    target: &InputTarget,
    inputs: Vec<KeyboardInput>,
    dry_run: bool,
  ) -> Result<Option<Vec<InputActionResult>>, KeyboardInputError> {
    let plans = validate_keyboard_batch(target, &inputs)?;
    if dry_run {
      return Ok(None);
    }

    let mut completed = Vec::new();
    for (index, plan) in plans.into_iter().enumerate() {
      let action = match plan {
        KeyboardPlan::Press {
          keys,
          count,
          interval,
          settle,
        } => {
          let mut combined: Option<InputActionResult> = None;
          let mut input = self.session.lock_input().map_err(|cause| keyboard_failure(cause, index, completed.clone(), 0))?;
          for repetition in 0..count {
            if repetition > 0 {
              std::thread::sleep(interval);
            }
            with_keys(&mut *input, &keys, |_| Ok(())).map_err(|cause| keyboard_failure(cause, index, completed.clone(), repetition))?;
            let action = delivered(false);
            if let Some(combined) = &mut combined {
              combined.attempts.extend(action.attempts);
            } else {
              combined = Some(action);
            }
          }
          std::thread::sleep(settle);
          combined.expect("validated positive key repetition count")
        }
        KeyboardPlan::Type { text, options } => {
          self.type_text(&text, options).map_err(|cause| keyboard_failure(cause, index, completed.clone(), 0))?
        }
      };
      completed.push(action);
    }
    Ok(Some(completed))
  }

  /// Returns the pointer's current root-window pixel coordinates.
  pub fn current_position(&self) -> DriverResult<Point> {
    let (x, y) = self.session.lock_input()?.location().map_err(backend)?;
    Ok(Point::new(f64::from(x), f64::from(y)))
  }
  /// Moves the visible pointer to an integral X11 root coordinate.
  pub fn move_to(&self, point: Point) -> DriverResult<InputActionResult> {
    self.move_mouse_to(0, point)
  }

  /// Delivers a sampled movement through the shared logical-mouse lifecycle.
  pub fn move_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    notify: impl FnMut(auv_driver_common::mouse_input::MotionEvent) -> bool,
  ) -> DriverResult<(Point, InputActionResult)> {
    let receiver = self.pointer_backend(request.target.as_ref())?;
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, None, receiver, notify)
  }

  /// Composes button press, sampled motion, and release under one admission.
  pub fn drag_mouse(&self, request: auv_driver_common::MoveMouseRequest, button: MouseButton) -> DriverResult<(Point, InputActionResult)> {
    let receiver = self.pointer_backend(request.target.as_ref())?;
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, Some(button), receiver, |_| true)
  }

  pub fn hold_mouse(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    duration: Duration,
  ) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().hold(mouse, point, button, duration, self.pointer_backend(Some(target))?)
  }

  pub fn create_mouse(&self) -> DriverResult<u64> {
    auv_driver_common::mouse_input::mouse_coordinator().create_mouse()
  }

  pub fn remove_mouse(&self, mouse: u64) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().remove_mouse(mouse)
  }

  pub fn mouse_down(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    timeout: Duration,
  ) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().down(mouse, point, button, timeout, self.pointer_backend(Some(target))?)
  }

  pub fn mouse_up(&self, mouse: u64) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().up(mouse)
  }

  pub fn move_mouse_to(&self, mouse: u64, point: Point) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().move_to(mouse, point, Arc::new(X11MouseBackend::new(self.session.clone())))
  }

  fn pointer_backend(&self, target: Option<&InputTarget>) -> DriverResult<Arc<dyn MouseBackend>> {
    match target {
      None | Some(InputTarget::Foreground) => Ok(Arc::new(X11MouseBackend::new(self.session.clone()))),
      Some(InputTarget::Window(_)) => Err(DriverError::unsupported("X11 window-targeted mouse input")),
      Some(InputTarget::Application { .. }) => Err(DriverError::unsupported("mouse input requires a window or foreground target")),
    }
  }
  /// Left-clicks at a root coordinate with modifiers released after the action.
  pub fn click_at(&self, point: Point, click: Click, modifiers: ClickModifiers) -> DriverResult<InputActionResult> {
    self.click_button_at(point, MouseButton::Left, click, modifiers)
  }
  /// Clicks a selected mouse button; zero repetitions fail before moving.
  pub fn click_button_at(
    &self,
    point: Point,
    button: MouseButton,
    click: Click,
    modifiers: ClickModifiers,
  ) -> DriverResult<InputActionResult> {
    let (x, y) = coordinates(point)?;
    if click.count() == 0 {
      return Err(invalid("click count must be positive"));
    }
    let mut input = self.session.lock_input()?;
    let keys = modifiers_to_keys(modifiers);
    with_keys(&mut *input, &keys, |input| {
      input.move_mouse(x, y, Coordinate::Abs).map_err(backend)?;
      for index in 0..click.count() {
        if index > 0 {
          std::thread::sleep(click.interval().unwrap_or_default());
        }
        with_button(input, mouse_button(button), |_| Ok(()))?;
      }
      Ok(())
    })?;
    Ok(delivered(true))
  }
  /// Drags between root coordinates, releasing the button even if motion fails.
  /// The path is a direct move; callers needing timed curves should not use it.
  pub fn drag(&self, from: Point, to: Point, button: MouseButton) -> DriverResult<InputActionResult> {
    let (x, y) = coordinates(from)?;
    let (end_x, end_y) = coordinates(to)?;
    let mut input = self.session.lock_input()?;
    input.move_mouse(x, y, Coordinate::Abs).map_err(backend)?;
    with_button(&mut *input, mouse_button(button), |input| input.move_mouse(end_x, end_y, Coordinate::Abs).map_err(backend))?;
    Ok(delivered(true))
  }
  /// Converts logical pixels to whole XTEST wheel detents, carrying sub-notch
  /// remainder across calls on clones of this session.
  pub fn scroll_at(&self, point: Point, scroll: Scroll, settle: Duration) -> DriverResult<InputActionResult> {
    let (x, y) = coordinates(point)?;
    let mut input = self.session.lock_input()?;
    let mut remainder = self.session.wheel_remainder.lock().map_err(|_| backend("X11 wheel remainder mutex poisoned"))?;
    let ((dx, dy), next_remainder) = wheel_notches(*remainder, scroll)?;
    // Enigo expands detents into individual XTEST events and calls i32::abs.
    // Bound work before moving, including rejecting i32::MIN overflow.
    if dx.unsigned_abs() > 1024 || dy.unsigned_abs() > 1024 {
      return Err(invalid("scroll is limited to 1024 wheel detents per axis"));
    }
    input.move_mouse(x, y, Coordinate::Abs).map_err(backend)?;
    // NOTICE(x11-partial-wheel): XTEST sends one button click per detent and
    // each axis separately. A later failure can leave earlier wheel events
    // delivered; do not infer rollback or retry this operation automatically.
    if dx != 0 {
      input.scroll(dx, Axis::Horizontal).map_err(backend)?;
    }
    if dy != 0 {
      input.scroll(dy, Axis::Vertical).map_err(backend)?;
    }
    *remainder = next_remainder;
    std::thread::sleep(settle);
    Ok(delivered(true))
  }
  /// Delivers one named key or a `ctrl+shift+p` style shortcut.
  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    self.press_keys(options.into())
  }
  /// Holds keys in order and releases in reverse order for each repetition.
  /// All key spellings and repetition options are checked before any delivery.
  pub fn press_keys(&self, options: PressKeysOptions) -> DriverResult<InputActionResult> {
    let keys = parse_keys(&options)?;
    let mut input = self.session.lock_input()?;
    for index in 0..options.count {
      if index > 0 {
        std::thread::sleep(options.interval);
      }
      with_keys(&mut *input, &keys, |_| Ok(()))?;
    }
    std::thread::sleep(options.settle);
    Ok(delivered(false))
  }
  /// Types literal Unicode using Enigo's keyboard mapping, without clipboard use.
  /// Replacement sends Ctrl+A then Backspace; the focused app must bind Ctrl+A
  /// to select-all. Rejects background-only policy and NUL before delivery.
  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    validate_text(text, options)?;
    let mut input = self.session.lock_input()?;
    if options.replace_existing {
      with_keys(&mut *input, &[Key::Control, Key::Unicode('a')], |_| Ok(()))?;
      with_keys(&mut *input, &[Key::Backspace], |_| Ok(()))?;
    }
    for (index, ch) in text.chars().enumerate() {
      if index > 0 {
        std::thread::sleep(options.inter_char_delay);
      }
      match ch {
        '\n' | '\r' => with_keys(&mut *input, &[Key::Return], |_| Ok(()))?,
        '\t' => with_keys(&mut *input, &[Key::Tab], |_| Ok(()))?,
        _ => input.text(&ch.to_string()).map_err(backend)?,
      }
    }
    if options.submit != TextSubmit::No {
      with_keys(&mut *input, &[Key::Return], |_| Ok(()))?;
    }
    std::thread::sleep(options.settle);
    // TODO(x11-clipboard): clipboard fallback and preservation are deferred;
    // introduce them only with an explicit selection-ownership contract.
    Ok(delivered(false))
  }
}

struct HeldKeyboardBackend {
  session: X11DriverSession,
  keys: Vec<Key>,
}

impl KeyboardBackend for HeldKeyboardBackend {
  fn key_count(&self) -> usize {
    self.keys.len()
  }

  fn key(&self, index: usize, down: bool) -> DriverResult<()> {
    self
      .session
      .lock_input()?
      .key(
        self.keys[index],
        if down {
          Direction::Press
        } else {
          Direction::Release
        },
      )
      .map_err(backend)
  }

  fn result(&self) -> InputActionResult {
    delivered(false)
  }
}

struct X11MouseBackend {
  session: X11DriverSession,
}

impl X11MouseBackend {
  fn new(session: X11DriverSession) -> Self {
    Self { session }
  }
}

impl MouseBackend for X11MouseBackend {
  fn current_position(&self) -> DriverResult<Point> {
    let (x, y) = self.session.lock_input()?.location().map_err(backend)?;
    Ok(Point::new(f64::from(x), f64::from(y)))
  }

  fn move_to(&self, point: Point, _held: Option<MouseButton>) -> DriverResult<InputActionResult> {
    let (x, y) = motion_coordinates(point)?;
    self.session.lock_input()?.move_mouse(x, y, Coordinate::Abs).map_err(backend)?;
    Ok(delivered(true))
  }

  fn button(&self, point: Point, button: MouseButton, down: bool) -> DriverResult<InputActionResult> {
    let (x, y) = coordinates(point)?;
    let mut input = self.session.lock_input()?;
    input.move_mouse(x, y, Coordinate::Abs).map_err(backend)?;
    input
      .button(
        mouse_button(button),
        if down {
          Direction::Press
        } else {
          Direction::Release
        },
      )
      .map_err(backend)?;
    Ok(delivered(true))
  }
}

#[derive(Debug)]
enum KeyboardPlan {
  Press {
    keys: Vec<Key>,
    count: u32,
    interval: Duration,
    settle: Duration,
  },
  Type {
    text: String,
    options: TypeTextOptions,
  },
}

fn validate_keyboard_batch(target: &InputTarget, inputs: &[KeyboardInput]) -> Result<Vec<KeyboardPlan>, KeyboardInputError> {
  if inputs.is_empty() {
    return Err(keyboard_failure(invalid("keyboard input requires at least one action"), 0, vec![], 0));
  }
  if !matches!(target, InputTarget::Foreground) {
    return Err(keyboard_failure(DriverError::unsupported("X11 targeted keyboard input"), 0, vec![], 0));
  }
  inputs
    .iter()
    .enumerate()
    .map(|(index, input)| {
      if input.policy() != InputPolicy::ForegroundPreferred {
        return Err(keyboard_failure(invalid("background keyboard input requires supported targeted delivery"), index, vec![], 0));
      }
      match input {
        KeyboardInput::PressKeys { options, .. } => parse_keys(options)
          .map(|keys| KeyboardPlan::Press {
            keys,
            count: options.count,
            interval: options.interval,
            settle: options.settle,
          })
          .map_err(|cause| keyboard_failure(cause, index, vec![], 0)),
        KeyboardInput::TypeText { text, options } => validate_text(text, *options)
          .map(|()| KeyboardPlan::Type {
            text: text.clone(),
            options: *options,
          })
          .map_err(|cause| keyboard_failure(cause, index, vec![], 0)),
        KeyboardInput::PasteText { .. } => Err(keyboard_failure(DriverError::unsupported("X11 clipboard paste"), index, vec![], 0)),
      }
    })
    .collect()
}

fn keyboard_failure(
  cause: DriverError,
  action_index: usize,
  completed: Vec<InputActionResult>,
  completed_presses: u32,
) -> KeyboardInputError {
  KeyboardInputError {
    cause,
    progress: KeyboardInputProgress {
      action_index,
      completed,
      completed_presses,
    },
  }
}

fn validate_text(text: &str, options: TypeTextOptions) -> DriverResult<()> {
  if options.policy == InputPolicy::BackgroundOnly {
    return Err(invalid("X11 driver only supports foreground input"));
  }
  if text.contains('\0') {
    return Err(invalid("text cannot contain NUL"));
  }
  Ok(())
}

fn validate_foreground(target: &InputTarget, policy: InputPolicy, unsupported: &'static str) -> DriverResult<()> {
  if !matches!(target, InputTarget::Foreground) {
    return Err(DriverError::unsupported(unsupported));
  }
  if policy != InputPolicy::ForegroundPreferred {
    return Err(invalid("background input requires supported targeted delivery"));
  }
  Ok(())
}

fn integral(value: f64) -> DriverResult<i32> {
  if !value.is_finite() || value.fract() != 0.0 || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
    return Err(invalid("X11 input requires finite integral coordinates"));
  }
  Ok(value as i32)
}

// NOTICE(x11-wheel-unit): XTEST exposes discrete wheel buttons, not pixel
// motion. Match the current Linux driver conversion (120 logical px/notch)
// until a receiver test demonstrates a different X11 application scaling.
// TODO(x11-hi-res-wheel): sub-notch deltas persist only in one X11 session;
// add a pixel-precise path if an X11 consumer requires exact small deltas.
fn wheel_notches(remainder: (f64, f64), scroll: Scroll) -> DriverResult<((i32, i32), (f64, f64))> {
  if !scroll.delta_x.is_finite() || !scroll.delta_y.is_finite() || (scroll.delta_x == 0.0 && scroll.delta_y == 0.0) {
    return Err(invalid("X11 scroll requires finite, non-zero logical-pixel deltas"));
  }
  let x = remainder.0 + scroll.delta_x / 120.0;
  let y = remainder.1 + scroll.delta_y / 120.0;
  if x.abs() > f64::from(i32::MAX) || y.abs() > f64::from(i32::MAX) {
    return Err(invalid("X11 scroll exceeds the wheel notch range"));
  }
  Ok(((x.trunc() as i32, y.trunc() as i32), (x.fract(), y.fract())))
}
fn coordinates(point: Point) -> DriverResult<(i32, i32)> {
  let x = integral(point.x)?;
  let y = integral(point.y)?;
  xtest_coordinates(x, y)
}

/// Projects sampled logical motion onto the integral X11 root pixel grid.
///
/// Shared mouse paths interpolate in logical coordinates and can therefore
/// contain fractional intermediate points even when both endpoints are whole
/// pixels. XTEST has no subpixel motion representation, so each motion sample
/// uses the nearest root pixel while direct click and scroll coordinates keep
/// their stricter lossless validation.
fn motion_coordinates(point: Point) -> DriverResult<(i32, i32)> {
  if !point.x.is_finite() || !point.y.is_finite() {
    return Err(invalid("X11 pointer motion requires finite coordinates"));
  }
  let x = point.x.round();
  let y = point.y.round();
  if x < f64::from(i32::MIN) || x > f64::from(i32::MAX) || y < f64::from(i32::MIN) || y > f64::from(i32::MAX) {
    return Err(invalid("X11 pointer motion coordinates exceed signed 32-bit range"));
  }
  xtest_coordinates(x as i32, y as i32)
}

fn xtest_coordinates(x: i32, y: i32) -> DriverResult<(i32, i32)> {
  // XTEST motion fields are signed 16-bit, even though Enigo accepts i32.
  if i16::try_from(x).is_err() || i16::try_from(y).is_err() {
    return Err(invalid("XTEST coordinates exceed signed 16-bit range"));
  }
  Ok((x, y))
}
fn mouse_button(button: MouseButton) -> Button {
  match button {
    MouseButton::Left => Button::Left,
    MouseButton::Middle => Button::Middle,
    MouseButton::Right => Button::Right,
  }
}
fn modifiers_to_keys(modifiers: ClickModifiers) -> Vec<Key> {
  [
    (modifiers.control, Key::Control),
    (modifiers.alt, Key::Alt),
    (modifiers.shift, Key::Shift),
    (modifiers.meta, Key::Meta),
  ]
  .into_iter()
  .filter_map(|(enabled, key)| enabled.then_some(key))
  .collect()
}

fn parse_keys(options: &PressKeysOptions) -> DriverResult<Vec<Key>> {
  if options.keys.is_empty() || options.count == 0 || options.count > 255 || (options.count > 1 && options.interval.is_zero()) {
    return Err(invalid("keys must be nonempty; count must be 1..=255 with a positive interval for repetitions"));
  }
  let mut keys = Vec::new();
  for name in &options.keys {
    let key = parse_key(name)?;
    if !keys.contains(&key) {
      keys.push(key);
    }
  }
  Ok(keys)
}
fn parse_key(name: &str) -> DriverResult<Key> {
  let name = name.trim();
  let key = match name.to_ascii_lowercase().as_str() {
    "ctrl" | "control" => Key::Control,
    "shift" => Key::Shift,
    "alt" | "option" => Key::Alt,
    "meta" | "super" | "win" | "command" | "cmd" => Key::Meta,
    "return" | "enter" => Key::Return,
    "escape" | "esc" => Key::Escape,
    "tab" => Key::Tab,
    "space" => Key::Space,
    "backspace" => Key::Backspace,
    "delete" | "del" => Key::Delete,
    "left" => Key::LeftArrow,
    "right" => Key::RightArrow,
    "up" => Key::UpArrow,
    "down" => Key::DownArrow,
    "home" => Key::Home,
    "end" => Key::End,
    "pageup" | "pgup" => Key::PageUp,
    "pagedown" | "pgdn" => Key::PageDown,
    "prtsc" | "prtscr" | "printscreen" | "print" => Key::PrintScr,
    "capslock" => Key::CapsLock,
    "numlock" => Key::Numlock,
    "scrolllock" => Key::ScrollLock,
    "insert" | "ins" => Key::Insert,
    "pause" => Key::Pause,
    "break" => Key::Break,
    "f1" => Key::F1,
    "f2" => Key::F2,
    "f3" => Key::F3,
    "f4" => Key::F4,
    "f5" => Key::F5,
    "f6" => Key::F6,
    "f7" => Key::F7,
    "f8" => Key::F8,
    "f9" => Key::F9,
    "f10" => Key::F10,
    "f11" => Key::F11,
    "f12" => Key::F12,
    "f13" => Key::F13,
    "f14" => Key::F14,
    "f15" => Key::F15,
    "f16" => Key::F16,
    "f17" => Key::F17,
    "f18" => Key::F18,
    "f19" => Key::F19,
    "f20" => Key::F20,
    "f21" => Key::F21,
    "f22" => Key::F22,
    "f23" => Key::F23,
    "f24" => Key::F24,
    _ => {
      let mut chars = name.chars();
      match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii_graphic() => Key::Unicode(ch),
        _ => return Err(invalid(format!("unknown key {name:?}; use type_text for Unicode text"))),
      }
    }
  };
  Ok(key)
}

// Every attempted press is paired with a release, including a press reporting
// an error: transport errors do not prove the server never received the event.
fn with_keys<B: Keyboard, T>(input: &mut B, keys: &[Key], action: impl FnOnce(&mut B) -> DriverResult<T>) -> DriverResult<T> {
  let mut pressed = Vec::new();
  let mut outcome = Ok(());
  for &key in keys {
    pressed.push(key);
    if let Err(error) = input.key(key, Direction::Press) {
      outcome = Err(backend(error));
      break;
    }
  }
  let outcome = outcome.and_then(|()| action(input));
  let mut cleanup = Ok(());
  for key in pressed.into_iter().rev() {
    if let Err(error) = input.key(key, Direction::Release) {
      cleanup = Err(backend(error));
    }
  }
  finish(outcome, cleanup)
}
fn with_button<B: Mouse, T>(input: &mut B, button: Button, action: impl FnOnce(&mut B) -> DriverResult<T>) -> DriverResult<T> {
  let outcome = input.button(button, Direction::Press).map_err(backend).and_then(|()| action(input));
  let cleanup = input.button(button, Direction::Release).map_err(backend);
  finish(outcome, cleanup)
}
fn finish<T>(outcome: DriverResult<T>, cleanup: DriverResult<()>) -> DriverResult<T> {
  match (outcome, cleanup) {
    (Ok(value), Ok(())) => Ok(value),
    (Err(error), Ok(())) => Err(error),
    (Ok(_), Err(error)) => Err(backend(format!("input release failed: {error}"))),
    (Err(error), Err(cleanup)) => Err(backend(format!("{error}; input release also failed: {cleanup}"))),
  }
}
fn delivered(pointer: bool) -> InputActionResult {
  let mut result = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);
  result.mouse_disturbance = if pointer {
    DisturbanceLevel::Foreground
  } else {
    DisturbanceLevel::None
  };
  result.focus_disturbance = DisturbanceLevel::Unknown;
  result
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;
