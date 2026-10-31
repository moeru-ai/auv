//! Foreground pointer and keyboard input via Win32 `SendInput`.
//!
//! Mirrors the macOS driver's `InputApi`, but Windows has no accessibility
//! input path, so every primitive is delivered as a foreground synthetic event
//! (`InputDeliveryPath::ForegroundSystemEvents`). Pointer moves use absolute
//! virtual-desktop coordinates; text is injected as Unicode key events and key
//! presses/shortcuts as virtual-key events. The coordinate math and key parsing
//! live in host-independent helpers so they stay unit-testable without sending
//! real input.
// Explicit global primitives use the current foreground. Window input batches install
// recipient protection; their native submissions share this same delivery boundary.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Point;
use auv_driver_common::input::{
  Click, ClickModifiers, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions,
  PasteTextOptions, Scroll, TextSubmit, TypeTextOptions,
};
use auv_driver_common::{
  DriverError, InputTarget, KeyboardBackend, KeyboardHold, KeyboardHoldId, KeyboardInput, KeyboardInputError, KeyboardInputProgress,
  PressKeysOptions,
};

use crate::clipboard::{ClipboardRead, ClipboardReader};
use crate::error::{backend, invalid_input};
use crate::session::InputApi;

#[path = "input_recipient.rs"]
mod recipient;

// Validate all waits before a batch can submit its first action. The native
// waiter checks again because the clock can advance after this preflight.
fn validate_keyboard_waits(waits: &[Duration]) -> DriverResult<()> {
  let now = std::time::Instant::now();
  if waits.iter().any(|duration| now.checked_add(*duration).is_none()) {
    return Err(invalid_input("keyboard wait exceeds the native clock range"));
  }
  Ok(())
}

/// Virtual-key code for a modifier, kept as a bare `u16` so shortcut parsing
/// stays independent of the `windows` crate types.
mod vk {
  pub const BACK: u16 = 0x08;
  pub const TAB: u16 = 0x09;
  pub const RETURN: u16 = 0x0D;
  pub const SHIFT: u16 = 0x10;
  pub const CONTROL: u16 = 0x11;
  pub const MENU: u16 = 0x12; // Alt
  pub const ESCAPE: u16 = 0x1B;
  pub const PAUSE: u16 = 0x13;
  pub const CAPITAL: u16 = 0x14; // Caps Lock
  pub const SPACE: u16 = 0x20;
  pub const PRIOR: u16 = 0x21; // Page Up
  pub const NEXT: u16 = 0x22; // Page Down
  pub const END: u16 = 0x23;
  pub const HOME: u16 = 0x24;
  pub const LEFT: u16 = 0x25;
  pub const UP: u16 = 0x26;
  pub const RIGHT: u16 = 0x27;
  pub const DOWN: u16 = 0x28;
  pub const SNAPSHOT: u16 = 0x2C; // Print Screen
  pub const INSERT: u16 = 0x2D;
  pub const DELETE: u16 = 0x2E;
  pub const LWIN: u16 = 0x5B;
  pub const APPS: u16 = 0x5D; // Menu
  pub const F1: u16 = 0x70;
  pub const NUMLOCK: u16 = 0x90;
  pub const SCROLL: u16 = 0x91;
  // Media keys (global — no app focus required).
  pub const MEDIA_NEXT_TRACK: u16 = 0xB0;
  pub const MEDIA_PREV_TRACK: u16 = 0xB1;
  pub const MEDIA_STOP: u16 = 0xB2;
  pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
}

/// Keys that Win32 sends with `KEYEVENTF_EXTENDEDKEY`: the navigation block
/// and the right-hand keys that share scan codes with the keypad.
fn is_extended_key(code: u16) -> bool {
  matches!(
    code,
    vk::PRIOR
      | vk::NEXT
      | vk::END
      | vk::HOME
      | vk::LEFT
      | vk::UP
      | vk::RIGHT
      | vk::DOWN
      | vk::INSERT
      | vk::DELETE
      | vk::APPS
      | vk::SNAPSHOT
      | vk::NUMLOCK
  )
}

/// A parsed key request: zero or more modifier virtual-keys plus one target key.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyChord {
  modifiers: Vec<u16>,
  key: u16,
}

pub fn click_at(
  point: Point,
  button: auv_driver_common::MouseButton,
  click: Click,
  modifiers: ClickModifiers,
) -> DriverResult<InputActionResult> {
  #[cfg(feature = "overlay")]
  let reported = click.clone();
  let result = deliver_click(point, button, click, modifiers)?;
  #[cfg(feature = "overlay")]
  crate::overlay_follow::report(crate::overlay_follow::clicked(point, button, &reported, None));
  Ok(result)
}

/// Delivers a click without reporting it to the live overlay, for a caller that reports
/// it together with the window it was aimed at.
pub(crate) fn deliver_click(
  point: Point,
  button: auv_driver_common::MouseButton,
  click: Click,
  modifiers: ClickModifiers,
) -> DriverResult<InputActionResult> {
  let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
  let (count, interval) = click_parts(&click)?;
  let start_time = std::time::Instant::now();
  native::click(point, button, count, interval, &click_modifier_keys(modifiers))?;
  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  let button_desc = format!("{button:?}");
  crate::latency::record_latency_event("click_at", elapsed_ms, None, Some("SendInput"), Some(&button_desc));
  Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

/// Keeps a foreground click batch bound to the admitted window recipient.
pub(crate) fn deliver_window_click(
  window: &auv_driver_common::window::Window,
  point: Point,
  button: auv_driver_common::MouseButton,
  click: Click,
  modifiers: ClickModifiers,
) -> DriverResult<InputActionResult> {
  let result = (|| {
    let (count, interval) = click_parts(&click)?;
    if !point.x.is_finite() || !point.y.is_finite() {
      return Err(invalid_input("window click point must be finite"));
    }
    if count > 1 && Instant::now().checked_add(interval).is_none() {
      return Err(invalid_input("window click interval exceeds the native clock range"));
    }
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    with_recipient(window, || deliver_click(point, button, click, modifiers)).map(add_admission_attempt)
  })();
  result.map_err(not_delivered)
}

/// Retains the scroll recipient through native delivery and the requested settle.
pub(crate) fn deliver_window_scroll(
  window: &auv_driver_common::window::Window,
  point: Point,
  scroll: Scroll,
  settle: Duration,
) -> DriverResult<InputActionResult> {
  let result = (|| {
    wheel_units(scroll)?;
    if !point.x.is_finite() || !point.y.is_finite() {
      return Err(invalid_input("window scroll point must be finite"));
    }
    if Instant::now().checked_add(settle).is_none() {
      return Err(invalid_input("window scroll settle exceeds the native clock range"));
    }
    let deliver = || {
      let action = scroll_at(point, scroll, Duration::ZERO)?;
      recipient::wait(settle)?;
      Ok(action)
    };
    if recipient::reuse_scroll_recipient(window)? {
      deliver().map_err(recipient::stamp_current)
    } else {
      let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
      with_recipient(window, deliver).map(add_admission_attempt)
    }
  })();
  result.map_err(not_delivered)
}

/// Owns native recipient lifetime and prefix stamping. The caller must already
/// own desktop admission; a complete motion owns it in the common coordinator.
fn with_recipient<T>(window: &auv_driver_common::window::Window, deliver: impl FnOnce() -> DriverResult<T>) -> DriverResult<T> {
  let guard = recipient::Guard::open(window)?;
  guard.begin_action(DisturbanceLevel::Temporary, DisturbanceLevel::None);
  // TODO(windows-mouse-coordinates): frame/point freshness and physical-pointer
  // ownership need a coordinate producer/consumer contract; see the mouse recipient note.
  let result = deliver().map_err(|cause| guard.stamp(cause))?;
  guard.check().map_err(|cause| guard.stamp(cause))?;
  Ok(result)
}

fn add_admission_attempt(mut action: InputActionResult) -> InputActionResult {
  action.attempts.insert(
    0,
    InputAttempt {
      path: InputDeliveryPath::ForegroundSystemEvents,
      succeeded: true,
      message: Some("window recipient identity and foreground admitted".into()),
    },
  );
  action
}

/// Complete foreground drags select the native window recipient explicitly;
/// InputApi's Window target remains its separate posted-message route.
pub(crate) fn deliver_window_drag(
  window: &auv_driver_common::Window,
  mut movement: auv_driver_common::MoveMouseRequest,
  button: auv_driver_common::MouseButton,
) -> DriverResult<(Point, InputActionResult)> {
  movement.target = Some(InputTarget::Window(window.clone()));
  let backend = std::sync::Arc::new(MouseBackend {
    target: InputTarget::Window(window.clone()),
  });
  auv_driver_common::mouse_input::mouse_coordinator()
    .motion(movement, Some(button), backend, |_| true)
    .map(|(point, action)| (point, add_admission_attempt(action)))
    .map_err(not_delivered)
}

/// Runs the shared schedule under one retained Windows scroll recipient.
pub(crate) fn deliver_window_scroll_motion(
  input: &impl auv_driver_common::WindowInput,
  window: &auv_driver_common::Window,
  point: auv_driver_common::WindowPoint,
  motion: &auv_driver_common::ScrollMotion,
  options: auv_driver_common::ScrollOptions,
  notify: &mut dyn FnMut(auv_driver_common::ScrollMotionProgress),
) -> DriverResult<auv_driver_common::ScrollMotionResult> {
  let result = (|| {
    motion.schedule()?;
    if !point.point().x.is_finite() || !point.point().y.is_finite() {
      return Err(invalid_input("window scroll point must be finite"));
    }
    let auv_driver_common::MotionTiming::FixedDuration { duration, .. } = motion.timing;
    if Instant::now().checked_add(duration).and_then(|deadline| deadline.checked_add(options.settle)).is_none() {
      return Err(invalid_input("window scroll motion exceeds the native clock range"));
    }
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    let mut completed = with_recipient(window, || {
      recipient::with_scroll_steps(|| {
        auv_driver_common::scroll_motion::run_window_scroll_motion(input, window, point, motion, options, notify)
      })
    })?;
    completed.action = add_admission_attempt(completed.action);
    Ok(completed)
  })();
  result.map_err(not_delivered)
}

/// Runs live velocity control under the same recipient while idle or moving.
pub(crate) fn deliver_window_scroll_stream(
  input: &impl auv_driver_common::WindowInput,
  window: &auv_driver_common::Window,
  point: auv_driver_common::WindowPoint,
  control: &auv_driver_common::ScrollStreamControl,
  stream: auv_driver_common::ScrollStreamOptions,
  options: auv_driver_common::ScrollOptions,
  notify: &mut dyn FnMut(auv_driver_common::ScrollStreamProgress),
) -> DriverResult<auv_driver_common::ScrollStreamResult> {
  let result = (|| {
    stream.validate()?;
    if !point.point().x.is_finite() || !point.point().y.is_finite() {
      return Err(invalid_input("window scroll point must be finite"));
    }
    if Instant::now().checked_add(options.settle).is_none() {
      return Err(invalid_input("window scroll settle exceeds the native clock range"));
    }
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    let mut completed = with_recipient(window, || {
      recipient::with_scroll_steps(|| {
        auv_driver_common::scroll_stream::run_window_scroll_stream(input, window, point, control, stream, options, notify)
      })
    })?;
    completed.action = completed.action.map(add_admission_attempt);
    Ok(completed)
  })();
  result.map_err(not_delivered)
}

/// The shared executor uses the platform wait only inside an admitted batch.
pub(crate) fn wait_scroll(deadline: Instant) -> DriverResult<()> {
  if recipient::scroll_batch_active() {
    recipient::wait(deadline.saturating_duration_since(Instant::now()))
  } else {
    auv_driver_common::input_cancellation::wait_until(deadline, "scroll motion")
  }
}

/// Refusal before submission must not overwrite an existing partial receipt.
pub(crate) fn not_delivered(cause: DriverError) -> DriverError {
  match cause {
    cause @ DriverError::InputAction { .. } => cause,
    cause => DriverError::InputAction {
      cause: Box::new(cause),
      progress: auv_driver_common::InputActionProgress::NotDelivered,
    },
  }
}

fn click_modifier_keys(modifiers: ClickModifiers) -> Vec<u16> {
  [
    (modifiers.shift, vk::SHIFT),
    (modifiers.control, vk::CONTROL),
    (modifiers.alt, vk::MENU),
    (modifiers.meta, vk::LWIN),
  ]
  .into_iter()
  .filter_map(|(enabled, key)| enabled.then_some(key))
  .collect()
}

pub fn move_to(point: Point) -> DriverResult<InputActionResult> {
  auv_driver_common::mouse_input::mouse_coordinator().move_to(0, point, std::sync::Arc::new(MouseBackend::default()))
}

pub fn current_position() -> DriverResult<Point> {
  native::current_position()
}

pub(crate) fn click_parts(click: &Click) -> DriverResult<(u32, Duration)> {
  let count = click.count();
  if count == 0 {
    return Err(invalid_input("repeated click count must be greater than zero"));
  }
  Ok((u32::from(count), click.interval().unwrap_or(Duration::ZERO)))
}

pub fn scroll_at(point: Point, scroll: Scroll, settle: Duration) -> DriverResult<InputActionResult> {
  let units = wheel_units(scroll)?;
  let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
  let start_time = std::time::Instant::now();
  native::scroll(point, units)?;
  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  crate::latency::record_latency_event("scroll_at", elapsed_ms, None, Some("SendInput"), None);
  sleep_if_nonzero(settle);
  Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

pub fn type_text(text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
  validate_keyboard_waits(&[options.inter_char_delay, options.settle])?;
  if matches!(options.policy, InputPolicy::BackgroundOnly) {
    return Err(invalid_input("windows type_text cannot use background_only input policy"));
  }
  let submit_key = text_submit_virtual_key(options.submit)?;
  native::type_text(text, &options, submit_key)?;
  recipient::wait(options.settle)?;
  Ok(foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None))
}

pub fn press_key(options: KeyPressOptions) -> DriverResult<InputActionResult> {
  let chord = parse_key_chord(&options.key)?;
  let start_time = std::time::Instant::now();
  native::press_chord(&chord)?;
  let elapsed_ms = start_time.elapsed().as_secs_f64() * 1000.0;
  crate::latency::record_latency_event("press_key", elapsed_ms, None, Some("SendInput"), Some(&options.key));
  sleep_if_nonzero(options.settle);
  Ok(foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None))
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

fn combination(options: &PressKeysOptions) -> DriverResult<Vec<u16>> {
  if options.keys.is_empty() || !(1..=255).contains(&options.count) || (options.count > 1 && options.interval.is_zero()) {
    return Err(invalid_input("keys must not be empty; count must be 1..255 and repeated presses require a positive interval"));
  }

  let mut codes = Vec::with_capacity(options.keys.len());
  let mut ordinary = false;

  for key in &options.keys {
    let modifier = modifier_virtual_key(key);

    if ordinary && modifier.is_some() {
      return Err(invalid_input("modifiers must precede ordinary keys"));
    }

    ordinary |= modifier.is_none();
    let (code, needs_shift) = match modifier {
      Some(modifier) => (modifier, false),
      None => key_virtual_key(key).ok_or_else(|| invalid_input(format!("invalid key {key:?}")))?,
    };

    if codes.contains(&code) {
      return Err(invalid_input("a combination cannot contain duplicate keys"));
    }
    // A shifted character such as `+` on a US layout is Shift plus its key.
    if needs_shift && !codes.contains(&vk::SHIFT) {
      codes.insert(0, vk::SHIFT);
    }

    codes.push(code);
  }

  Ok(codes)
}

fn press_combination(codes: Vec<u16>) -> DriverResult<InputActionResult> {
  let _reservation = recipient::reserve_submission()?;
  // The existing hold controller releases an injected prefix if a key-down
  // fails, and releases the complete chord in reverse order on success.
  let mut hold = auv_driver_common::keyboard_hold_controller()
    .clone()
    .down(std::sync::Arc::new(HeldKeyboardBackend::new(codes)), Duration::from_secs(2))?;
  hold.wait_and_release(Duration::ZERO)
}

impl InputApi<'_> {
  /// Validate a batch and preserve native progress; Window targets retain a recipient guard.
  // TODO(windows-targeted-keyboard): Application ids need a Windows identity producer. A posted
  // route also needs child-control resolution (a top-level window drops posted
  // `WM_CHAR`) and a decision on modifiers, which posted messages cannot carry
  // (`GetKeyState` in the target thread stays unchanged). Probe, not command
  // evidence: docs/ai/references/driver/2026-10-12-windows-targeted-keyboard-probe.md.
  pub fn input_keyboard(
    &self,
    target: &InputTarget,
    inputs: Vec<KeyboardInput>,
    dry_run: bool,
  ) -> Result<Option<Vec<InputActionResult>>, KeyboardInputError> {
    if inputs.is_empty() {
      return Err(keyboard_failure(invalid_input("keyboard input requires at least one action"), 0, vec![], 0));
    }

    if matches!(target, InputTarget::Application { .. }) {
      return Err(keyboard_failure(DriverError::unsupported("Windows targeted keyboard input"), 0, vec![], 0));
    }
    if let InputTarget::Window(window) = target {
      if window.reference.id.parse::<isize>().ok().filter(|id| *id != 0).is_none() || window.process_id.is_none_or(|pid| pid == 0) {
        return Err(keyboard_failure(invalid_input("window keyboard input requires an observed native id and process id"), 0, vec![], 0));
      }
    }

    let plans = inputs
      .iter()
      .enumerate()
      .map(|(index, input)| {
        if input.policy() != InputPolicy::ForegroundPreferred {
          return Err(keyboard_failure(invalid_input("Windows keyboard input requires foreground-preferred policy"), index, vec![], 0));
        }

        let (interval, settle) = match input {
          KeyboardInput::PressKeys { options, .. } => (options.interval, options.settle),
          KeyboardInput::TypeText { options, .. } => (options.inter_char_delay, options.settle),
          KeyboardInput::PasteText { options, .. } => (Duration::ZERO, options.settle),
        };
        validate_keyboard_waits(&[interval, settle]).map_err(|cause| keyboard_failure(cause, index, vec![], 0))?;

        let plan = match input {
          KeyboardInput::PressKeys { options, .. } => combination(options).map(Some),
          KeyboardInput::TypeText { options, .. } => text_submit_virtual_key(options.submit).map(|_| None),
          KeyboardInput::PasteText { options, .. } => text_submit_virtual_key(options.submit).map(|_| None),
        };

        plan.map_err(|cause| keyboard_failure(cause, index, vec![], 0))
      })
      .collect::<Result<Vec<_>, _>>()?;

    if dry_run {
      return Ok(None);
    }

    let guard = match target {
      InputTarget::Window(window) => Some(recipient::Guard::open(window).map_err(|cause| {
        // Admission queries identity and installs hooks; it never submits input.
        keyboard_failure(
          DriverError::InputAction {
            cause: Box::new(cause),
            progress: auv_driver_common::InputActionProgress::NotDelivered,
          },
          0,
          vec![],
          0,
        )
      })?),
      _ => None,
    };

    let mut completed = Vec::with_capacity(inputs.len());

    for (index, (input, plan)) in inputs.into_iter().zip(plans).enumerate() {
      if let Some(guard) = &guard {
        guard.begin_action(
          DisturbanceLevel::None,
          if matches!(&input, KeyboardInput::PasteText { .. }) {
            DisturbanceLevel::Temporary
          } else {
            DisturbanceLevel::None
          },
        );
      }
      let failure = |cause, completed: &Vec<InputActionResult>, presses| {
        let cause = if let Some(guard) = &guard {
          guard.stamp(cause)
        } else {
          cause
        };
        keyboard_failure(cause, index, completed.clone(), presses)
      };
      let completed_presses = match &input {
        KeyboardInput::PressKeys { options, .. } => options.count,
        _ => 0,
      };
      let action = match input {
        KeyboardInput::PressKeys { options, .. } => {
          let mut combined: Option<InputActionResult> = None;

          for repetition in 0..options.count {
            if repetition > 0 {
              recipient::wait(options.interval).map_err(|cause| failure(cause, &completed, repetition))?;
            }

            let action = press_combination(plan.as_ref().expect("validated press combination").clone())
              .map_err(|cause| failure(cause, &completed, repetition))?;

            if let Some(combined) = &mut combined {
              combined.attempts.extend(action.attempts);
            } else {
              combined = Some(action);
            }
          }

          if !options.settle.is_zero() {
            recipient::wait(options.settle).map_err(|cause| failure(cause, &completed, options.count))?;
          }

          combined.expect("validated positive press count")
        }
        KeyboardInput::TypeText { text, options } => type_text(&text, options).map_err(|cause| failure(cause, &completed, 0))?,
        KeyboardInput::PasteText { options, .. } => paste_text(options).map_err(|cause| failure(cause, &completed, 0))?,
      };

      recipient::wait(Duration::ZERO).map_err(|cause| failure(cause, &completed, completed_presses))?;
      completed.push(action);
    }

    Ok(Some(completed))
  }
}

struct HeldKeyboardBackend {
  keys: Vec<u16>,
  owned: Vec<std::sync::atomic::AtomicBool>,
}

impl HeldKeyboardBackend {
  fn new(keys: Vec<u16>) -> Self {
    let owned = keys.iter().map(|_| std::sync::atomic::AtomicBool::new(false)).collect();
    Self { keys, owned }
  }
}

impl KeyboardBackend for HeldKeyboardBackend {
  fn key_count(&self) -> usize {
    self.keys.len()
  }
  fn key(&self, index: usize, down: bool) -> DriverResult<()> {
    use std::sync::atomic::Ordering;
    if !down && !self.owned[index].load(Ordering::Acquire) {
      return Ok(());
    }
    native::key_transition(self.keys[index], down)?;
    // Each transition submits exactly one event: success proves its insertion; refusal before
    // the native call owns no key. The controller may ask to clean the full planned combination.
    self.owned[index].store(down, Ordering::Release);
    Ok(())
  }
  fn result(&self) -> InputActionResult {
    foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None)
  }
}

pub fn key_down(target: &InputTarget, keys: Vec<String>, policy: InputPolicy, timeout: Duration) -> DriverResult<KeyboardHold> {
  if !matches!(target, InputTarget::Foreground) || policy != InputPolicy::ForegroundPreferred {
    return Err(DriverError::unsupported("Windows targeted keyboard input"));
  }
  let codes = combination(&PressKeysOptions {
    keys,
    ..Default::default()
  })?;
  let controller = auv_driver_common::keyboard_hold_controller().clone();
  controller.down(std::sync::Arc::new(HeldKeyboardBackend::new(codes)), timeout)
}

pub fn key_up(hold: KeyboardHoldId) -> DriverResult<InputActionResult> {
  auv_driver_common::keyboard_hold_controller().up(hold)
}

pub fn hold_keys(target: &InputTarget, keys: Vec<String>, policy: InputPolicy, duration: Duration) -> DriverResult<InputActionResult> {
  // Give this call time to release before the controller's fallback deadline.
  let mut hold = key_down(target, keys, policy, duration.saturating_add(Duration::from_secs(1)))?;
  hold.wait_and_release(duration)
}

/// How long the foreground application has to ask for pasted text before the paste fails and the
/// user's clipboard comes back.
// NOTICE: classic edit controls ask within milliseconds of Ctrl+V; the bound only has to cover a
// busy UI thread. An application that asks later than this gets the user's restored clipboard
// instead. That is the trade against holding the user's clipboard indefinitely.
const PASTE_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// How long pasted text stays on the clipboard after a process other than the foreground one
/// asked for it first. After that first request Windows serves the text without asking again, so
/// the foreground application's own read is no longer seen and a fixed hold stands in for it.
const PASTE_UNATTRIBUTED_HOLD: Duration = Duration::from_millis(500);

/// One paste at a time: each takes the clipboard from the user and gives it back.
static PASTE: Mutex<()> = Mutex::new(());

/// Who asked for pasted text first, which decides when the user's clipboard may come back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PasteRead {
  /// The foreground application asked: the paste was taken, and the clipboard can come back now.
  Foreground { after: Duration },
  /// Another process, or one Windows does not name, asked first. The foreground application may
  /// still be about to read, so the text is held for [`PASTE_UNATTRIBUTED_HOLD`].
  Other {
    reader: ClipboardReader,
    after: Duration,
  },
  /// Nobody asked within [`PASTE_READ_TIMEOUT`]: nothing was pasted.
  Unread,
}

fn paste_read(read: Option<ClipboardRead>, sent: Instant, foreground: Option<u32>) -> PasteRead {
  let Some(read) = read else {
    return PasteRead::Unread;
  };
  let after = read.at.saturating_duration_since(sent);
  match (read.reader, foreground) {
    (ClipboardReader::Process(reader), Some(foreground)) if reader == foreground => PasteRead::Foreground { after },
    (reader, _) => PasteRead::Other { reader, after },
  }
}

/// Pastes `options.text` into the foreground control through the clipboard and gives the user's
/// clipboard back, every format of it, once the foreground application has asked for the text.
///
/// The text is lent by delayed rendering ([`crate::clipboard::lend_text`]), so the application's
/// request for it is observed. That request is consumption of the clipboard, not proof that the
/// text landed, so the result stays unverified.
pub fn paste_text(options: PasteTextOptions) -> DriverResult<InputActionResult> {
  validate_keyboard_waits(&[options.settle])?;
  let submit_key = text_submit_virtual_key(options.submit)?;
  let _paste = PASTE.lock().unwrap_or_else(PoisonError::into_inner);
  let snapshot = crate::clipboard::snapshot_rich()?;
  let foreground = native::foreground_process();
  let pasted = (|| {
    let loan = crate::clipboard::lend_text(&options.text)?;
    if options.replace_existing {
      // Ctrl+A then Backspace clears the focused field, as `type_text` does.
      native::press_chord(&KeyChord {
        modifiers: vec![vk::CONTROL],
        key: u16::from(b'A'),
      })?;
      native::press_chord(&KeyChord {
        modifiers: Vec::new(),
        key: vk::BACK,
      })?;
    }
    let sent = Instant::now();
    paste()?;
    let read = paste_read(loan.wait_for_read(PASTE_READ_TIMEOUT), sent, foreground);
    if matches!(read, PasteRead::Other { .. }) {
      recipient::wait(PASTE_UNATTRIBUTED_HOLD)?;
    }
    Ok((loan, read))
  })();
  // Restore before the loan closes its window, so the window no longer owns the clipboard and
  // is not asked to leave the text behind.
  let restored = crate::clipboard::restore_rich(&snapshot);
  let read = match (pasted, restored) {
    (Ok((_loan, read)), Ok(())) => read,
    (Err(error), Ok(())) => return Err(error),
    (Ok(_), Err(restore)) => return Err(backend(format!("pasted text but failed to restore the clipboard: {restore}"))),
    (Err(error), Err(restore)) => return Err(backend(format!("{error}; additionally failed to restore the clipboard: {restore}"))),
  };
  let message = match read {
    PasteRead::Foreground { after } => format!("the foreground application read the pasted text {} ms after Ctrl+V", after.as_millis()),
    PasteRead::Other { reader, after } => {
      let reader = match reader {
        ClipboardReader::Process(process) => format!("process {process}, not the foreground application,"),
        ClipboardReader::Unknown => "a reader that opened the clipboard without a window".to_string(),
      };
      format!(
        "{reader} read the pasted text first, {} ms after Ctrl+V; the clipboard was restored {} ms later",
        after.as_millis(),
        PASTE_UNATTRIBUTED_HOLD.as_millis()
      )
    }
    PasteRead::Unread => {
      return Err(backend(format!(
        "the foreground application did not read the pasted text within {} ms; the clipboard was restored",
        PASTE_READ_TIMEOUT.as_millis()
      )));
    }
  };
  if let Some(key) = submit_key {
    native::press_chord(&KeyChord {
      modifiers: Vec::new(),
      key,
    })?;
  }
  recipient::wait(options.settle)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::ClipboardPaste,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::ClipboardPaste,
      succeeded: true,
      message: Some(message),
    }],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Unknown,
    clipboard_disturbance: DisturbanceLevel::Temporary,
  })
}

/// Issues the system copy shortcut (Ctrl+C) against the foreground target.
///
/// Mirrors the macOS driver's keystroke-based copy: it activates the active
/// control's own copy handler rather than reading selection text directly, so
/// the resulting clipboard contents depend on the focused application.
pub fn copy() -> DriverResult<()> {
  native::press_chord(&KeyChord {
    modifiers: vec![vk::CONTROL],
    key: u16::from(b'C'),
  })
}

/// Issues the system paste shortcut (Ctrl+V) against the foreground target.
pub fn paste() -> DriverResult<()> {
  native::press_chord(&KeyChord {
    modifiers: vec![vk::CONTROL],
    key: u16::from(b'V'),
  })
}

fn foreground_result(
  mouse_disturbance: DisturbanceLevel,
  focus_disturbance: DisturbanceLevel,
  clipboard_disturbance: DisturbanceLevel,
) -> InputActionResult {
  InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![InputAttempt::success(
      InputDeliveryPath::ForegroundSystemEvents,
    )],
    verified: false,
    mouse_disturbance,
    focus_disturbance,
    clipboard_disturbance,
  }
}

/// Signed Win32 wheel deltas for one [`Scroll`], shared by `SendInput` and
/// posted `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` delivery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WheelUnits {
  /// Positive toward earlier content (Win32 wheel-up convention).
  pub(crate) vertical: i32,
  /// Positive toward the right (Win32 horizontal wheel convention).
  pub(crate) horizontal: i32,
}

// NOTICE(scroll-pixels-per-notch): Win32 wheel input has no pixel unit. One
// notch (`WHEEL_DELTA` = 120) scrolls 100 DIPs in Chromium and about three
// lines in classic controls, so AUV maps 100 logical pixels to one notch.
// Revisit with live evidence if a target class needs its own factor; see
// `docs/ai/references/driver/2026-10-06-scroll-delta-contract.md`.
const SCROLL_PIXELS_PER_WHEEL_NOTCH: f64 = 100.0;
const WHEEL_DELTA_PER_NOTCH: f64 = 120.0;
/// Logical pixels per Win32 wheel unit, the smallest timed-scroll step.
pub(crate) const SCROLL_PIXELS_PER_WHEEL_UNIT: f64 = SCROLL_PIXELS_PER_WHEEL_NOTCH / WHEEL_DELTA_PER_NOTCH;

/// Converts AUV logical-pixel scroll deltas (positive toward later content)
/// into Win32 wheel units. Rejects values that are non-finite or exceed the
/// signed 16-bit word posted wheel messages carry.
pub(crate) fn wheel_units(scroll: Scroll) -> DriverResult<WheelUnits> {
  let units = |pixels: f64, axis: &str| -> DriverResult<i32> {
    if !pixels.is_finite() {
      return Err(invalid_input(format!("scroll {axis} must be finite")));
    }
    let units = (pixels * WHEEL_DELTA_PER_NOTCH / SCROLL_PIXELS_PER_WHEEL_NOTCH).round();
    if units.abs() > f64::from(i16::MAX) {
      return Err(invalid_input(format!("scroll {axis} {pixels} exceeds the Windows wheel delta range")));
    }
    Ok(units as i32)
  };
  Ok(WheelUnits {
    vertical: units(-scroll.delta_y, "delta_y")?,
    horizontal: units(scroll.delta_x, "delta_x")?,
  })
}

fn sleep_if_nonzero(duration: Duration) {
  if !duration.is_zero() {
    std::thread::sleep(duration);
  }
}

/// Maps a screen coordinate onto the `0..=65535` absolute axis that
/// `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` expects, where `origin` and
/// `extent` are the virtual desktop's start and size on that axis.
fn normalize_absolute(coord: f64, origin: i32, extent: i32) -> i32 {
  if extent <= 1 {
    return 0;
  }
  let relative = coord - f64::from(origin);
  let scaled = relative * 65535.0 / f64::from(extent - 1);
  scaled.round().clamp(0.0, 65535.0) as i32
}

fn text_submit_virtual_key(submit: TextSubmit) -> DriverResult<Option<u16>> {
  match submit {
    TextSubmit::No => Ok(None),
    TextSubmit::Return => Ok(Some(vk::RETURN)),
    TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => {
      Err(invalid_input(format!("text submit {submit:?} is not supported by the windows desktop driver yet")))
    }
  }
}

fn parse_key_chord(input: &str) -> DriverResult<KeyChord> {
  let trimmed = input.trim();
  if trimmed.is_empty() {
    return Err(invalid_input("key must not be empty"));
  }
  if trimmed.contains('+') {
    return parse_shortcut(trimmed);
  }
  if let Some((key, needs_shift)) = key_virtual_key(trimmed) {
    return Ok(KeyChord {
      modifiers: if needs_shift {
        vec![vk::SHIFT]
      } else {
        Vec::new()
      },
      key,
    });
  }
  Err(invalid_input(format!(
    "invalid key {trimmed}; use a special key like Return, a shortcut like ctrl+f, or type_text for multi-character text"
  )))
}

fn parse_shortcut(shortcut: &str) -> DriverResult<KeyChord> {
  let parts = shortcut.split('+').map(str::trim).filter(|part| !part.is_empty()).collect::<Vec<_>>();
  if parts.len() < 2 {
    return Err(invalid_input(format!("invalid shortcut {shortcut}; expected a form like ctrl+f or ctrl+shift+p")));
  }
  let (key_part, modifier_parts) = parts.split_last().expect("len checked >= 2");
  let (key, needs_shift) =
    key_virtual_key(key_part).ok_or_else(|| invalid_input(format!("invalid shortcut {shortcut}; unsupported key {key_part}")))?;
  let mut modifiers = if needs_shift {
    vec![vk::SHIFT]
  } else {
    Vec::new()
  };
  for raw in modifier_parts {
    let modifier =
      modifier_virtual_key(raw).ok_or_else(|| invalid_input(format!("invalid shortcut {shortcut}; unsupported modifier {raw}")))?;
    if !modifiers.contains(&modifier) {
      modifiers.push(modifier);
    }
  }
  Ok(KeyChord { modifiers, key })
}

fn modifier_virtual_key(raw: &str) -> Option<u16> {
  use auv_driver_common::{Key, Modifier};
  // Keep the existing Windows shortcut aliases; super is Linux-only here.
  if raw.eq_ignore_ascii_case("super") {
    return None;
  }
  match raw.parse::<Key>().ok()? {
    Key::Modifier(Modifier::Control) => Some(vk::CONTROL),
    Key::Modifier(Modifier::Shift) => Some(vk::SHIFT),
    Key::Modifier(Modifier::Alt) => Some(vk::MENU),
    Key::Modifier(Modifier::Meta) => Some(vk::LWIN),
    Key::Symbol(_) => None,
  }
}

fn special_virtual_key(raw: &str) -> Option<u16> {
  use auv_driver_common::{Key, Keysym};
  // forward_delete is a macOS alias, not an existing Windows input spelling.
  // Compare without separators, as the shared parser does (`forward-delete`).
  if raw.chars().filter(char::is_ascii_alphanumeric).collect::<String>().eq_ignore_ascii_case("forwarddelete") {
    return None;
  }
  match raw.parse::<Key>().ok()? {
    Key::Symbol(Keysym::Return | Keysym::KP_Enter) => Some(vk::RETURN),
    Key::Symbol(Keysym::Tab) => Some(vk::TAB),
    Key::Symbol(Keysym::Escape) => Some(vk::ESCAPE),
    Key::Symbol(Keysym::space) => Some(vk::SPACE),
    Key::Symbol(Keysym::Delete) => Some(vk::DELETE),
    Key::Symbol(Keysym::BackSpace) => Some(vk::BACK),
    Key::Symbol(Keysym::Left) => Some(vk::LEFT),
    Key::Symbol(Keysym::Up) => Some(vk::UP),
    Key::Symbol(Keysym::Right) => Some(vk::RIGHT),
    Key::Symbol(Keysym::Down) => Some(vk::DOWN),
    Key::Symbol(Keysym::Home) => Some(vk::HOME),
    Key::Symbol(Keysym::End) => Some(vk::END),
    Key::Symbol(Keysym::Page_Up) => Some(vk::PRIOR),
    Key::Symbol(Keysym::Page_Down) => Some(vk::NEXT),
    Key::Symbol(Keysym::Insert) => Some(vk::INSERT),
    Key::Symbol(Keysym::Menu) => Some(vk::APPS),
    Key::Symbol(Keysym::Print) => Some(vk::SNAPSHOT),
    Key::Symbol(Keysym::Pause) => Some(vk::PAUSE),
    Key::Symbol(Keysym::Caps_Lock) => Some(vk::CAPITAL),
    Key::Symbol(Keysym::Num_Lock) => Some(vk::NUMLOCK),
    Key::Symbol(Keysym::Scroll_Lock) => Some(vk::SCROLL),
    // VK_F1..VK_F24 are contiguous, like the F1..F24 keysyms.
    Key::Symbol(symbol) if (Keysym::F1.raw()..=Keysym::F24.raw()).contains(&symbol.raw()) => {
      Some(vk::F1 + (symbol.raw() - Keysym::F1.raw()) as u16)
    }
    Key::Symbol(Keysym::XF86_AudioPlay) => Some(vk::MEDIA_PLAY_PAUSE),
    Key::Symbol(Keysym::XF86_AudioNext) => Some(vk::MEDIA_NEXT_TRACK),
    Key::Symbol(Keysym::XF86_AudioPrev) => Some(vk::MEDIA_PREV_TRACK),
    Key::Symbol(Keysym::XF86_AudioStop) => Some(vk::MEDIA_STOP),
    _ => None,
  }
}

/// Resolves one non-modifier key name to its virtual-key code and whether
/// the active layout needs Shift for it. Names outside the named-key table
/// fall back to the character they spell: a single character (`,`) or a
/// shared logical name (`comma`, `bracket_left`).
fn key_virtual_key(raw: &str) -> Option<(u16, bool)> {
  if let Some(code) = special_virtual_key(raw) {
    return Some((code, false));
  }
  let mut chars = raw.chars();
  let character = match (chars.next(), chars.next()) {
    (Some(character), None) => character,
    _ => match raw.parse::<auv_driver_common::Key>().ok()? {
      auv_driver_common::Key::Symbol(symbol) => symbol.key_char().filter(char::is_ascii_graphic)?,
      auv_driver_common::Key::Modifier(_) => return None,
    },
  };
  char_virtual_key(character)
}

/// Maps a printable ASCII character to a virtual-key code. Letters and digits
/// use the Win32 VK convention for `0-9`/`A-Z`; punctuation depends on the
/// keyboard layout, so it asks the active layout.
fn char_virtual_key(character: char) -> Option<(u16, bool)> {
  if character.is_ascii_alphanumeric() {
    return Some((u16::from(character.to_ascii_uppercase() as u8), false));
  }
  if !character.is_ascii_graphic() {
    return None;
  }
  native::layout_virtual_key(character)
}

#[cfg(target_os = "windows")]
mod native {
  use std::mem::size_of;
  use std::time::Duration;

  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::TypeTextOptions;
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VIRTUAL_KEY, VkKeyScanExW,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
  };

  use super::{KeyChord, normalize_absolute};
  use crate::error::backend;

  /// Sends one batch of synthetic input events, failing if the OS rejected any
  /// (a blocked input session or UIPI denial returns a short count).
  fn send_inputs(inputs: &[INPUT]) -> DriverResult<()> {
    if inputs.is_empty() {
      return Ok(());
    }
    let sent = insert_inputs(inputs, false)?;
    if sent as usize != inputs.len() {
      return Err(backend(format!("SendInput injected {sent} of {} events (input may be blocked)", inputs.len())));
    }
    Ok(())
  }

  /// Central native boundary. Cleanup releases bypass recipient loss and do not replace the
  /// original submission receipt; a cancelled action must still release AUV-owned held keys.
  fn insert_inputs(inputs: &[INPUT], release: bool) -> DriverResult<u32> {
    let _reservation = if release {
      None
    } else {
      super::recipient::reserve_submission()?
    };
    if !release {
      super::recipient::check()?;
      super::recipient::before_submission();
    }
    // SAFETY: every caller initializes each INPUT union to match its type tag; the slice is live.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if !release {
      super::recipient::after_submission(sent as usize == inputs.len());
    }
    Ok(sent)
  }

  /// A Unicode or ordinary-key pair owns its matching release even after recipient loss.
  fn send_key_pair(inputs: [INPUT; 2]) -> DriverResult<()> {
    let sent = insert_inputs(&inputs, false)?;
    if sent == 2 {
      return Ok(());
    }
    let release = if sent == 1 {
      insert_inputs(&inputs[1..], true).map(|count| count == 1)
    } else {
      Ok(true)
    };
    Err(backend(format!("SendInput injected {sent} of 2 key events; owned release accepted={}", release.unwrap_or(false))))
  }

  fn mouse_input(dx: i32, dy: i32, mouse_data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
      r#type: INPUT_MOUSE,
      Anonymous: INPUT_0 {
        mi: MOUSEINPUT {
          dx,
          dy,
          mouseData: mouse_data as u32,
          dwFlags: flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn unicode_input(unit: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VIRTUAL_KEY(0),
          wScan: unit,
          dwFlags: KEYEVENTF_UNICODE | flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn virtual_key_input(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    // NOTICE: navigation keys share scan codes with the numeric keypad. Without
    // KEYEVENTF_EXTENDEDKEY, applications that read scan codes can see a
    // keypad key (for example `4` instead of Left when Num Lock is on).
    let flags = if super::is_extended_key(vk) {
      flags | KEYEVENTF_EXTENDEDKEY
    } else {
      flags
    };
    INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VIRTUAL_KEY(vk),
          wScan: 0,
          dwFlags: flags,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  /// The foreground application's keyboard-layout virtual key for
  /// `character`, and whether it needs Shift.
  ///
  /// NOTICE: Windows keyboard layouts are per thread. `VkKeyScanW` would read
  /// the calling (daemon or Runner worker) thread's layout, which can differ
  /// from the focused application's on a multi-layout system, so the layout of
  /// the foreground window's thread is used. Characters that need Ctrl, Alt
  /// (AltGr) or other shift-state bits cannot be expressed as one key name.
  pub(super) fn layout_virtual_key(character: char) -> Option<(u16, bool)> {
    let foreground = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    let thread = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(foreground, None) };
    let layout = unsafe { GetKeyboardLayout(thread) };
    let scan = unsafe { VkKeyScanExW(character as u16, layout) };
    if scan == -1 {
      return None;
    }
    let [code, shift_state] = (scan as u16).to_le_bytes();
    // Bit 0 is Shift; any other bit (Ctrl, Alt, Hankaku, reserved) is rejected.
    if shift_state & !1 != 0 {
      return None;
    }
    Some((u16::from(code), shift_state & 1 != 0))
  }

  pub(super) fn current_position() -> DriverResult<Point> {
    let mut point = windows::Win32::Foundation::POINT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }
      .map_err(|error| crate::error::backend(format!("GetCursorPos failed: {error}")))?;
    Ok(Point::new(f64::from(point.x), f64::from(point.y)))
  }

  pub(super) fn button(button: auv_driver_common::MouseButton, down: bool) -> DriverResult<()> {
    send_button(button, down, insert_inputs)
  }

  fn send_button(
    button: auv_driver_common::MouseButton,
    down: bool,
    mut insert: impl FnMut(&[INPUT], bool) -> DriverResult<u32>,
  ) -> DriverResult<()> {
    let flags = button_flags(button);
    // The coordinator records button ownership before submitting its down.
    // Its matching up must survive cancellation and recipient loss.
    let sent = insert(&[mouse_input(0, 0, 0, if down { flags.0 } else { flags.1 })], !down)?;
    if sent != 1 {
      return Err(backend(format!("SendInput injected {sent} of 1 mouse button events (input may be blocked)")));
    }
    Ok(())
  }

  #[cfg(test)]
  mod button_tests {
    use super::*;
    use auv_driver_common::input_cancellation::{InputCancellation, with_input_cancellation};

    // ROOT CAUSE:
    //
    // A complete drag owns its down, but the old button-up used ordinary
    // admission. Cancellation therefore rejected its matching cleanup event.
    #[test]
    fn owned_button_up_survives_cancellation_and_checks_native_count() {
      let cancellation = std::sync::Arc::new(InputCancellation::default());
      cancellation.cancel();
      let mut inserted = Vec::new();
      let result = with_input_cancellation(cancellation, || {
        send_button(auv_driver_common::MouseButton::Left, false, |inputs, release| {
          if !release {
            super::super::recipient::check()?;
          }
          assert_eq!(inputs.len(), 1);
          // SAFETY: send_button initialized this INPUT as a mouse event.
          assert_eq!(unsafe { inputs[0].Anonymous.mi.dwFlags }, MOUSEEVENTF_LEFTUP);
          inserted.push(release);
          Ok(1)
        })
      });
      assert!(result.is_ok(), "owned up must bypass cancelled delivery admission: {result:?}");
      assert_eq!(inserted, [true]);
      let short = send_button(auv_driver_common::MouseButton::Left, false, |_, release| {
        assert!(release);
        Ok(0)
      });
      assert!(short.is_err(), "a refused up must never confirm release");
    }
  }

  pub(super) fn move_to(point: Point) -> DriverResult<()> {
    let origin_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let origin_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    let dx = normalize_absolute(point.x, origin_x, width);
    let dy = normalize_absolute(point.y, origin_y, height);
    send_inputs(&[mouse_input(
      dx,
      dy,
      0,
      MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
    )])
  }

  pub(super) fn click(
    point: Point,
    button: auv_driver_common::MouseButton,
    count: u32,
    interval: Duration,
    modifiers: &[u16],
  ) -> DriverResult<()> {
    let _reservation = super::recipient::reserve_submission()?;
    move_to(point)?;
    for index in 0..count {
      // Preserve already-held user keys: this operation only releases keys it
      // injects. SendInput does not reset existing keyboard state.
      let keys: Vec<_> = modifiers
        .iter()
        .copied()
        .filter(|key| {
          // SAFETY: GetAsyncKeyState accepts any virtual-key integer; no pointers.
          unsafe { GetAsyncKeyState(i32::from(*key)) >= 0 }
        })
        .collect();
      send_click(button, &keys, insert_inputs)?;
      if index + 1 < count && !interval.is_zero() {
        super::recipient::wait(interval)?;
      }
    }
    Ok(())
  }

  /// Owns partial click submission and releases only its inserted downs.
  /// A release bypasses admission and preserves the original submission receipt.
  fn send_click(
    button: auv_driver_common::MouseButton,
    keys: &[u16],
    mut insert: impl FnMut(&[INPUT], bool) -> DriverResult<u32>,
  ) -> DriverResult<()> {
    let inputs = click_inputs(button, keys);
    // SAFETY: inputs contains initialized INPUT unions matching their tags,
    // and its slice remains live throughout SendInput at the native boundary.
    let sent = insert(&inputs, false)? as usize;
    if sent == inputs.len() {
      return Ok(());
    }
    // SendInput inserts in order. Release only the injected prefix that
    // remains down, including the mouse button if its up was not inserted.
    let cleanup = click_cleanup(button, keys, sent);
    let release = if cleanup.is_empty() {
      Ok(())
    } else {
      insert(&cleanup, true).and_then(|released| {
        if released as usize == cleanup.len() {
          Ok(())
        } else {
          Err(backend(format!("SendInput injected {released} of {} events (input may be blocked)", cleanup.len())))
        }
      })
    };
    let detail = release.err().map(|error| format!("; input release also failed: {error}")).unwrap_or_default();
    Err(backend(format!("SendInput injected {sent} of {} click events{detail}", inputs.len())))
  }

  fn button_flags(button: auv_driver_common::MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
      MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    };
    match button {
      auv_driver_common::MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
      auv_driver_common::MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
      auv_driver_common::MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
  }

  pub(super) fn click_inputs(button: auv_driver_common::MouseButton, keys: &[u16]) -> Vec<INPUT> {
    let mut inputs: Vec<_> = keys.iter().map(|key| virtual_key_input(*key, KEYBD_EVENT_FLAGS(0))).collect();
    inputs.push(mouse_input(0, 0, 0, button_flags(button).0));
    inputs.push(mouse_input(0, 0, 0, button_flags(button).1));
    inputs.extend(keys.iter().rev().map(|key| virtual_key_input(*key, KEYEVENTF_KEYUP)));
    inputs
  }

  pub(super) fn click_cleanup(button: auv_driver_common::MouseButton, keys: &[u16], sent: usize) -> Vec<INPUT> {
    let mut inputs = Vec::new();
    if sent == keys.len() + 1 {
      inputs.push(mouse_input(0, 0, 0, button_flags(button).1));
    }
    let pressed = sent.min(keys.len());
    let released = sent.saturating_sub(keys.len() + 2).min(keys.len());
    inputs.extend(keys[..pressed.saturating_sub(released)].iter().rev().map(|key| virtual_key_input(*key, KEYEVENTF_KEYUP)));
    inputs
  }

  pub(super) fn scroll(point: Point, units: super::WheelUnits) -> DriverResult<()> {
    move_to(point)?;
    let mut inputs = Vec::new();
    if units.vertical != 0 {
      inputs.push(mouse_input(0, 0, units.vertical, MOUSEEVENTF_WHEEL));
    }
    if units.horizontal != 0 {
      inputs.push(mouse_input(0, 0, units.horizontal, MOUSEEVENTF_HWHEEL));
    }
    send_inputs(&inputs)
  }

  pub(super) fn type_text(text: &str, options: &TypeTextOptions, submit_key: Option<u16>) -> DriverResult<()> {
    if options.replace_existing {
      // Ctrl+A then Backspace clears the focused field before typing.
      press_chord(&KeyChord {
        modifiers: vec![super::vk::CONTROL],
        key: u16::from(b'A'),
      })?;
      send_key_pair([
        virtual_key_input(super::vk::BACK, KEYBD_EVENT_FLAGS(0)),
        virtual_key_input(super::vk::BACK, KEYEVENTF_KEYUP),
      ])?;
    }
    let inter_char_delay = options.inter_char_delay;
    for character in text.chars() {
      let mut buffer = [0u16; 2];
      for unit in character.encode_utf16(&mut buffer) {
        send_key_pair([
          unicode_input(*unit, KEYBD_EVENT_FLAGS(0)),
          unicode_input(*unit, KEYEVENTF_KEYUP),
        ])?;
      }
      if !inter_char_delay.is_zero() {
        super::recipient::wait(inter_char_delay)?;
      }
    }
    if let Some(vk) = submit_key {
      send_key_pair([
        virtual_key_input(vk, KEYBD_EVENT_FLAGS(0)),
        virtual_key_input(vk, KEYEVENTF_KEYUP),
      ])?;
    }
    Ok(())
  }

  pub(super) fn press_chord(chord: &KeyChord) -> DriverResult<()> {
    let _reservation = super::recipient::reserve_submission()?;
    let mut inputs = Vec::new();
    for modifier in &chord.modifiers {
      inputs.push(virtual_key_input(*modifier, KEYBD_EVENT_FLAGS(0)));
    }
    inputs.push(virtual_key_input(chord.key, KEYBD_EVENT_FLAGS(0)));
    inputs.push(virtual_key_input(chord.key, KEYEVENTF_KEYUP));
    // Release modifiers in reverse press order.
    for modifier in chord.modifiers.iter().rev() {
      inputs.push(virtual_key_input(*modifier, KEYEVENTF_KEYUP));
    }
    let sent = insert_inputs(&inputs, false)? as usize;
    if sent == inputs.len() {
      return Ok(());
    }
    // Downs precede reverse-order ups. Release only keys whose down was inserted without its up.
    let codes: Vec<_> = chord.modifiers.iter().copied().chain([chord.key]).collect();
    let cleanup: Vec<_> = codes
      .iter()
      .enumerate()
      .rev()
      .filter(|(index, _)| sent > *index && sent <= 2 * codes.len() - 1 - *index)
      .map(|(_, key)| virtual_key_input(*key, KEYEVENTF_KEYUP))
      .collect();
    let released = if cleanup.is_empty() {
      true
    } else {
      insert_inputs(&cleanup, true).is_ok_and(|count| count as usize == cleanup.len())
    };
    Err(backend(format!("SendInput injected {sent} of {} chord events; owned releases accepted={released}", inputs.len())))
  }

  pub(super) fn key_transition(key: u16, down: bool) -> DriverResult<()> {
    let inputs = [virtual_key_input(
      key,
      if down {
        KEYBD_EVENT_FLAGS(0)
      } else {
        KEYEVENTF_KEYUP
      },
    )];
    let sent = insert_inputs(&inputs, !down)?;
    if sent != 1 {
      return Err(backend(format!("SendInput injected {sent} of 1 key events")));
    }
    Ok(())
  }

  /// The process of the foreground window, the one keyboard input reaches.
  pub(super) fn foreground_process() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
      return None;
    }
    let mut process = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    (process != 0).then_some(process)
  }
  #[cfg(test)]
  mod click_tests {
    use super::*;

    // ROOT CAUSE:
    //
    // If a click was partially inserted, recipient loss or cancellation could
    // refuse its owned releases because cleanup reused ordinary admission.
    // Before the fix, a modifier and button could remain down.
    // The fix releases the inserted downs without admitting another action.
    #[test]
    fn partial_click_releases_owned_keys_and_button_after_admission_loss() {
      let mut calls = Vec::new();
      let result = send_click(auv_driver_common::MouseButton::Left, &[super::super::vk::SHIFT], |inputs, release| {
        calls.push((release, inputs.to_vec()));
        if calls.len() == 1 {
          return Ok(2); // Shift and button downs were inserted; their ups were not.
        }
        if !release {
          return Err(crate::error::invalid_input("input cancelled before native submission"));
        }
        Ok(inputs.len() as u32)
      });
      assert!(result.unwrap_err().to_string().contains("SendInput injected 2 of 4 click events"));
      assert_eq!(calls.len(), 2);
      assert!(!calls[0].0);
      assert!(calls[1].0, "owned releases must bypass lost/cancelled admission");
      assert_eq!(calls[1].1.len(), 2);
      // SAFETY: these events come from click_cleanup, which initializes each
      // INPUT union consistently with its type tag.
      unsafe {
        assert_eq!(calls[1].1[0].r#type, INPUT_MOUSE);
        assert_eq!(calls[1].1[0].Anonymous.mi.dwFlags, MOUSEEVENTF_LEFTUP);
        assert_eq!(calls[1].1[1].r#type, INPUT_KEYBOARD);
        assert_eq!(calls[1].1[1].Anonymous.ki.wVk.0, super::super::vk::SHIFT);
        assert_eq!(calls[1].1[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
      }
    }
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use std::time::Duration;

  use auv_driver_common::error::{DriverError, DriverResult};
  use auv_driver_common::geometry::Point;
  use auv_driver_common::input::{MouseButton, TypeTextOptions};

  use super::KeyChord;

  pub(super) fn current_position() -> DriverResult<Point> {
    Err(DriverError::unsupported("input.current_position"))
  }

  pub(super) fn button(_button: MouseButton, _down: bool) -> DriverResult<()> {
    Err(DriverError::unsupported("windows mouse button"))
  }

  pub(super) fn move_to(_point: Point) -> DriverResult<()> {
    Err(DriverError::unsupported("input.move_to"))
  }

  pub(super) fn click(_point: Point, _button: MouseButton, _count: u32, _interval: Duration, _modifiers: &[u16]) -> DriverResult<()> {
    Err(DriverError::unsupported("input.click"))
  }

  pub(super) fn scroll(_point: Point, _units: super::WheelUnits) -> DriverResult<()> {
    Err(DriverError::unsupported("input.scroll"))
  }

  pub(super) fn type_text(_text: &str, _options: &TypeTextOptions, _submit_key: Option<u16>) -> DriverResult<()> {
    Err(DriverError::unsupported("input.type_text"))
  }

  pub(super) fn press_chord(_chord: &KeyChord) -> DriverResult<()> {
    Err(DriverError::unsupported("input.press_key"))
  }

  pub(super) fn key_transition(_key: u16, _down: bool) -> DriverResult<()> {
    Err(DriverError::unsupported("input.key_down"))
  }

  pub(super) fn foreground_process() -> Option<u32> {
    None
  }

  pub(super) fn layout_virtual_key(_character: char) -> Option<(u16, bool)> {
    None
  }
}

#[cfg(test)]
#[path = "input_test.rs"]
mod tests;

#[derive(Clone)]
pub(crate) struct MouseBackend {
  pub(crate) target: InputTarget,
}
impl Default for MouseBackend {
  fn default() -> Self {
    Self {
      target: InputTarget::Foreground,
    }
  }
}
impl auv_driver_common::mouse_input::MouseBackend for MouseBackend {
  fn target(&self) -> InputTarget {
    self.target.clone()
  }
  fn current_position(&self) -> DriverResult<Point> {
    native::current_position()
  }
  fn move_to(&self, point: Point, _held: Option<auv_driver_common::MouseButton>) -> DriverResult<InputActionResult> {
    native::move_to(point).map_err(recipient::stamp_current)?;
    Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
  }
  fn button(&self, _point: Point, button: auv_driver_common::MouseButton, down: bool) -> DriverResult<InputActionResult> {
    native::button(button, down).map_err(recipient::stamp_current)?;
    Ok(foreground_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
  }
  fn with_motion(
    &self,
    motion: Box<dyn FnOnce() -> DriverResult<(Point, InputActionResult)> + '_>,
  ) -> DriverResult<(Point, InputActionResult)> {
    match &self.target {
      InputTarget::Window(window) => with_recipient(window, motion),
      _ => motion(),
    }
  }
  fn check_motion(&self) -> DriverResult<()> {
    recipient::check().map_err(recipient::stamp_current)
  }
}
