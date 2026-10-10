use std::thread;

use auv_driver_common::capture::{Activation, Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver_common::display::ObservedDisplays;
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::{Point, Rect, RelativeRect, ScreenPoint, Size, WindowPoint};
use auv_driver_common::input::{
  Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions, Scroll,
  ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions, TypeTextOptions, WaitOptions, WindowInput,
};
use auv_driver_common::selector::WindowSelector;
use auv_driver_common::vision::{TextRecognition, TextRecognitionOptions};
use auv_driver_common::window::{Window, WindowMutationKind, WindowMutationOptions, WindowMutationResult};
use auv_driver_common::{InputTarget, KeyboardHold, KeyboardHoldId, MouseButton};

use crate::accessibility::{AxTreeSnapshot, focus_node, select_node, snapshot_window};
use crate::background_input;
use crate::capture::{capture_display, capture_region, capture_window, list_displays};
use crate::clipboard::{
  ClipboardSnapshot, restore as restore_clipboard, restore_rich as restore_clipboard_rich, set_text as set_clipboard_text, snapshot,
  snapshot_rich as snapshot_clipboard_rich,
};
use crate::driver::WindowsDriverSession;
use crate::error::{invalid_input, not_found};
use crate::input::{click_at, copy, current_position, paste, press_key, scroll_at, type_text};
use crate::mutation::mutate_window;
use crate::permission::{WindowsPermissionProbe, probe as probe_permissions};
use crate::vision::{OcrMatches, find_text_in_capture, recognize_text_in_capture};
use crate::window::{activate_window, list_windows, resolve_window};

#[cfg(feature = "overlay")]
use auv_driver_overlay::{Overlay, ShowOptions};

/// Display-targeted capture capabilities.
///
/// Mirrors the macOS driver's `DisplayApi` shape so capture consumers share one
/// session surface across platforms.
#[derive(Clone, Copy, Debug)]
pub struct DisplayApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Window-targeted enumeration and resolution capabilities.
#[derive(Clone, Copy, Debug)]
pub struct WindowApi<'a> {
  session: &'a WindowsDriverSession,
}

/// Capture-driven text recognition capabilities.
///
/// Mirrors the macOS driver's `VisionApi`, projecting OCR results back into the
/// supplied capture's coordinate space.
#[derive(Clone, Copy, Debug)]
pub struct VisionApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Foreground pointer and keyboard input capabilities.
///
/// Mirrors the macOS driver's `InputApi`. Every primitive is delivered as a
/// foreground synthetic event via `SendInput`, since Windows has no
/// accessibility-targeted input path.
#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Text and rich clipboard snapshot/restore/set capabilities.
///
/// Mirrors the macOS driver's `ClipboardApi` for the text-only `snapshot`/
/// `restore`/`set_text` methods. `snapshot_rich`/`restore_rich` are a
/// Windows-only addition that preserve every memory-backed clipboard format,
/// not just text.
#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Process-level automation readiness capabilities.
///
/// Mirrors the macOS driver's `PermissionApi`, but probes the Windows process
/// token and session (UAC elevation, UIAccess/UIPI, interactive session)
/// instead of macOS TCC permissions.
#[derive(Clone, Copy, Debug)]
pub struct PermissionApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Window accessibility tree inspection capabilities.
///
/// Mirrors the macOS driver's AX tree capture, but reads the Microsoft UI
/// Automation tree for a window instead of the macOS `AXUIElement` tree.
#[derive(Clone, Copy, Debug)]
pub struct AccessibilityApi<'a> {
  _session: &'a WindowsDriverSession,
}

/// Overlay show/remove capabilities.
///
/// Mirrors the macOS driver's `OverlayApi`, dispatching through the shared
/// `auv-driver-overlay` facade with its `windows` backend enabled.
#[cfg(feature = "overlay")]
#[derive(Clone, Copy, Debug)]
pub struct OverlayApi<'a> {
  _session: &'a WindowsDriverSession,
}

impl WindowsDriverSession {
  pub fn display(&self) -> DisplayApi<'_> {
    DisplayApi { _session: self }
  }

  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
  }

  pub fn vision(&self) -> VisionApi<'_> {
    VisionApi { _session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { _session: self }
  }

  pub fn clipboard(&self) -> ClipboardApi<'_> {
    ClipboardApi { _session: self }
  }

  pub fn permission(&self) -> PermissionApi<'_> {
    PermissionApi { _session: self }
  }

  pub fn accessibility(&self) -> AccessibilityApi<'_> {
    AccessibilityApi { _session: self }
  }

  #[cfg(feature = "overlay")]
  pub fn overlay(&self) -> OverlayApi<'_> {
    OverlayApi { _session: self }
  }
}

#[cfg(feature = "overlay")]
impl OverlayApi<'_> {
  pub fn show(&self, overlay: &Overlay, options: ShowOptions) -> DriverResult<()> {
    auv_driver_overlay::show(overlay, options).map_err(|error| DriverError::Backend {
      message: error.to_string(),
    })
  }

  pub fn remove(&self) -> DriverResult<()> {
    auv_driver_overlay::remove().map_err(|error| DriverError::Backend {
      message: error.to_string(),
    })
  }

  /// Starts a live overlay that mirrors the input this process delivers from now on: the
  /// cursor glides between the points input acted on, clicks ripple and targeted windows
  /// are marked. The overlay only draws what was delivered and never sends input itself.
  /// One follower runs at a time; stop it with [`OperationFollower::stop`] or by dropping it.
  pub fn follow_operations(&self, options: ShowOptions) -> DriverResult<crate::OperationFollower> {
    crate::overlay_follow::follow(options)
  }
}

impl WindowApi<'_> {
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    list_windows()
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    resolve_window(&selector)
  }

  /// Restores and foregrounds a window before foreground-only input delivery.
  pub fn activate(&self, window: &Window) -> DriverResult<()> {
    activate_window(window)
  }

  /// Captures a single window's pixels via Win32 GDI `PrintWindow`.
  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    capture_window(window)
  }

  /// [`Self::capture`] with options; only `resolution` applies on Windows.
  pub fn capture_with(&self, window: &Window, options: CaptureOptions) -> DriverResult<Capture> {
    if options.display.is_some() || options.region.is_some() || options.window.is_some() {
      return Err(invalid_input("window.capture_with does not accept display, region, or nested window capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("window.capture_with cannot activate Windows windows in this slice"));
    }
    capture_window(window).map(|capture| capture.at_resolution(options.resolution))
  }

  /// Captures a single window's pixels via Windows.Graphics.Capture (WGC).
  pub fn capture_wgc(&self, window: &Window) -> DriverResult<Capture> {
    crate::wgc::capture_window_wgc(window)
  }

  /// Maps a window-relative point to its absolute screen position by offsetting
  /// against the window's screen-space frame origin.
  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    Ok(screen_point_for_window_point(window, point))
  }

  /// Maps an absolute screen point into window-relative coordinates.
  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    Ok(window_point_for_screen_point(window, point))
  }

  /// Polls `window`'s capture for `query` text until it appears or `wait`'s
  /// timeout elapses, returning whatever matches (possibly none) were last
  /// observed.
  pub fn find_text(&self, window: &Window, query: &str, region: RelativeRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    let started = std::time::Instant::now();
    loop {
      let capture = self.capture(window)?;
      let matches = self.session.vision().find_text_in_capture(&capture, query, region)?;
      if !matches.matches.is_empty() || started.elapsed() >= wait.timeout {
        return Ok(matches);
      }
      thread::sleep(wait.poll_interval);
    }
  }

  /// Like [`Self::find_text`], but fails with `NotFound` when the timeout
  /// elapses without a match instead of returning an empty result.
  pub fn wait_text(&self, window: &Window, query: &str, region: RelativeRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    let matches = self.find_text(window, query, region, wait)?;
    if matches.matches.is_empty() {
      Err(not_found(format!("text {query:?} before timeout")))
    } else {
      Ok(matches)
    }
  }

  pub fn move_to(&self, window: &Window, point: Point, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::MoveTo { point }, options)
  }

  pub fn resize(&self, window: &Window, size: Size, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::Resize { size }, options)
  }

  pub fn set_frame(&self, window: &Window, frame: Rect, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::SetFrame { frame }, options)
  }

  pub fn minimize(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::Minimize, options)
  }

  pub fn restore(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::Restore, options)
  }

  pub fn zoom(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    mutate_window(window, WindowMutationKind::Zoom, options)
  }

  /// Delivers a window-targeted click.
  ///
  /// `ForegroundPreferred` foregrounds the window and uses the same
  /// `SendInput` route as global clicks. `BackgroundOnly`/`BackgroundPreferred`
  /// instead post `WM_LBUTTONDOWN`/`WM_LBUTTONUP` directly to the control
  /// hit-tested under `point` (`background_input::click_at_window`), which
  /// does not raise or focus the window. `window_strategy` is a macOS
  /// background-routing selector; Windows has only one posted-message route
  /// today, so both variants resolve to it.
  fn click_impl(&self, window: &Window, point: WindowPoint, options: ClickOptions) -> DriverResult<InputActionResult> {
    if !matches!(options.policy, InputPolicy::ForegroundPreferred) {
      background_input::validate_modifiers(options.modifiers)?;
    }
    let screen_point = self.to_screen_point(window, point)?.point();
    if matches!(options.policy, InputPolicy::ForegroundPreferred) {
      let activation_attempt = foreground_window_attempt(window, "pointer delivery");
      let mut result = self.session.input().click_at(screen_point, options.button, options.click, options.modifiers)?;
      result.attempts.insert(0, activation_attempt);
      #[cfg(feature = "overlay")]
      crate::overlay_follow::report([crate::overlay_follow::window_targeted(window)]);
      return Ok(result);
    }
    let _ = options.window_strategy;
    #[cfg(feature = "overlay")]
    let click = options.click.clone();
    background_input::click_at_window(window, screen_point, options.button, options.click, options.modifiers)?;
    #[cfg(feature = "overlay")]
    {
      let mut events = vec![crate::overlay_follow::window_targeted(window)];
      events.extend(crate::overlay_follow::clicked(screen_point, options.button, &click));
      crate::overlay_follow::report(events);
    }
    Ok(InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse))
  }

  /// Delivers a window-targeted wheel scroll by trying each candidate in
  /// `options.delivery_strategy` in order (mirroring the macOS driver's
  /// candidate loop). `WindowTargetedWheel` posts `WM_MOUSEWHEEL`/
  /// `WM_MOUSEHWHEEL` to the control under `point`
  /// (`background_input::scroll_at_window`); `AxScroll` and
  /// `WindowTargetedKeyboardScroll` are not implemented on Windows and are
  /// recorded as failed attempts; `ForegroundHid` foregrounds the window and
  /// falls back to `SendInput`.
  fn scroll_impl(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    let mut attempts = Vec::new();
    for candidate in scroll_attempt_candidates(options.policy, &options.delivery_strategy) {
      match candidate {
        ScrollDeliveryCandidate::AxScroll => {
          attempts.push(InputAttempt::failure(InputDeliveryPath::AxScroll, "AX scroll is not supported by the windows desktop driver"));
        }
        ScrollDeliveryCandidate::WindowTargetedKeyboardScroll => {
          attempts.push(InputAttempt::failure(
            InputDeliveryPath::WindowTargetedKeyboardScroll,
            "window-targeted keyboard scroll is not supported by the windows desktop driver",
          ));
        }
        ScrollDeliveryCandidate::WindowTargetedWheel => {
          let screen_point = self.to_screen_point(window, point)?.point();
          match background_input::scroll_at_window(window, screen_point, scroll) {
            Ok(()) => {
              attempts.push(InputAttempt::success(InputDeliveryPath::WindowTargetedWheel));
              return Ok(InputActionResult {
                selected_path: InputDeliveryPath::WindowTargetedWheel,
                attempts,
                verified: false,
                mouse_disturbance: DisturbanceLevel::None,
                focus_disturbance: DisturbanceLevel::None,
                clipboard_disturbance: DisturbanceLevel::None,
              });
            }
            Err(error) => attempts.push(InputAttempt::failure(InputDeliveryPath::WindowTargetedWheel, error.to_string())),
          }
        }
        ScrollDeliveryCandidate::ForegroundHid => {
          if options.policy == InputPolicy::BackgroundOnly {
            continue;
          }
          let activation_attempt = foreground_window_attempt(window, "wheel delivery");
          let screen_point = self.to_screen_point(window, point)?.point();
          let mut result = self.session.input().scroll_at(screen_point, scroll, options.settle)?;
          attempts.push(activation_attempt);
          attempts.append(&mut result.attempts);
          result.attempts = attempts;
          return Ok(result);
        }
      }
    }
    Err(DriverError::unsupported("background_scroll"))
  }
}

impl WindowInput for WindowApi<'_> {
  fn click(&self, window: &Window, point: WindowPoint, options: ClickOptions) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    self.click_impl(window, point, options)
  }

  fn scroll(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    self.scroll_impl(window, point, scroll, options)
  }

  /// One Win32 wheel unit (1/120 notch) in logical pixels.
  fn scroll_quantum(&self) -> f64 {
    crate::input::SCROLL_PIXELS_PER_WHEEL_UNIT
  }

  /// `ForegroundPreferred` foregrounds the window like a foreground click and
  /// uses the `SendInput` desktop drag. Background policies post the held
  /// gesture to the window's child receiver without raising it.
  fn drag(
    &self,
    window: &Window,
    mut movement: auv_driver_common::MoveMouseRequest,
    button: auv_driver_common::MouseButton,
    policy: InputPolicy,
  ) -> DriverResult<(Point, InputActionResult)> {
    if !matches!(policy, InputPolicy::ForegroundPreferred) {
      movement.target = Some(auv_driver_common::InputTarget::Window(window.clone()));
      return self.session.input().drag_mouse(movement, button);
    }
    let activation_attempt = foreground_window_attempt(window, "pointer drag");
    movement.target = Some(auv_driver_common::InputTarget::Foreground);
    let (point, mut result) = self.session.input().drag_mouse(movement, button)?;
    result.attempts.insert(0, activation_attempt);
    Ok((point, result))
  }
}

/// Foregrounds `window` before a foreground-only input delivery, reporting the
/// outcome as an attempt instead of failing the whole delivery on activation
/// trouble (the subsequent `SendInput` call still targets the window's frame).
fn foreground_window_attempt(window: &Window, purpose: &str) -> InputAttempt {
  match activate_window(window) {
    Ok(()) => InputAttempt::success(InputDeliveryPath::ForegroundSystemEvents),
    Err(error) => InputAttempt::failure(
      InputDeliveryPath::ForegroundSystemEvents,
      format!("failed to foreground target window before {purpose}: {error}"),
    ),
  }
}

/// Orders scroll delivery candidates for a policy, mirroring the macOS
/// driver's candidate selection: `ForegroundPreferred` uses only `SendInput`;
/// `BackgroundOnly` drops `ForegroundHid` from the caller's strategy;
/// `BackgroundPreferred` tries the caller's strategy as given.
fn scroll_attempt_candidates(policy: InputPolicy, delivery_strategy: &ScrollDeliveryStrategy) -> Vec<ScrollDeliveryCandidate> {
  match policy {
    InputPolicy::ForegroundPreferred => vec![ScrollDeliveryCandidate::ForegroundHid],
    InputPolicy::BackgroundOnly => {
      delivery_strategy.candidates.iter().copied().filter(|candidate| *candidate != ScrollDeliveryCandidate::ForegroundHid).collect()
    }
    InputPolicy::BackgroundPreferred => delivery_strategy.candidates.clone(),
  }
}

impl VisionApi<'_> {
  pub fn recognize_text_in_capture(&self, capture: &Capture, region: RelativeRect) -> DriverResult<TextRecognition> {
    self.recognize_text_in_capture_with_options(capture, region, TextRecognitionOptions::default())
  }

  pub fn recognize_text_in_capture_with_options(
    &self,
    capture: &Capture,
    region: RelativeRect,
    options: TextRecognitionOptions,
  ) -> DriverResult<TextRecognition> {
    recognize_text_in_capture(capture, region, &options)
  }

  pub fn find_text_in_capture(&self, capture: &Capture, query: &str, region: RelativeRect) -> DriverResult<OcrMatches> {
    self.find_text_in_capture_with_options(capture, query, region, TextRecognitionOptions::default())
  }

  pub fn find_text_in_capture_with_options(
    &self,
    capture: &Capture,
    query: &str,
    region: RelativeRect,
    options: TextRecognitionOptions,
  ) -> DriverResult<OcrMatches> {
    find_text_in_capture(capture, query, region, &options)
  }
}

impl InputApi<'_> {
  /// A complete drag composes press, sampled movement, and release under one admission.
  pub fn drag_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    button: auv_driver_common::MouseButton,
  ) -> DriverResult<(Point, InputActionResult)> {
    let backend = self.pointer_backend(request.target.as_ref())?;
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, Some(button), backend, |_| true)
  }

  fn pointer_backend(&self, target: Option<&InputTarget>) -> DriverResult<std::sync::Arc<dyn auv_driver_common::mouse_input::MouseBackend>> {
    match target {
      None | Some(InputTarget::Foreground) => Ok(std::sync::Arc::new(crate::input::MouseBackend)),
      Some(InputTarget::Window(window)) => crate::background_input::mouse_backend(window.clone()),
      Some(InputTarget::Application { .. }) => Err(DriverError::unsupported("mouse input requires a window or foreground target")),
    }
  }

  pub fn move_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    notify: impl FnMut(auv_driver_common::mouse_input::MotionEvent) -> bool,
  ) -> DriverResult<(Point, InputActionResult)> {
    let backend = self.pointer_backend(request.target.as_ref())?;
    // TODO(overlay-follow-multi-mouse): the overlay draws one cursor, so only the shared
    // default mouse (zero) reports. Other logical mice need their own cursors first.
    #[cfg(feature = "overlay")]
    let (mut notify, mut reporter) =
      (notify, (request.mouse == 0).then(|| crate::overlay_follow::MotionReporter::new(request.target.as_ref())));
    #[cfg(feature = "overlay")]
    let notify = move |event: auv_driver_common::mouse_input::MotionEvent| {
      if let Some(reporter) = reporter.as_mut() {
        crate::overlay_follow::report(reporter.event(&event));
      }
      notify(event)
    };
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, None, backend, notify)
  }

  /// Local convenience: press, wait, and release under one admission.
  /// This composes the mouse lifecycle; it is not a separate Runner capability.
  pub fn hold_mouse(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    duration: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().hold(mouse, point, button, duration, self.pointer_backend(Some(target))?)
  }

  /// Creates logical state; this does not create an independent OS cursor.
  pub fn create_mouse(&self) -> DriverResult<u64> {
    auv_driver_common::mouse_input::mouse_coordinator().create_mouse()
  }

  pub fn remove_mouse(&self, mouse: u64) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().remove_mouse(mouse)
  }

  /// Holds one button until mouse_up or the mandatory bounded timeout.
  pub fn mouse_down(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    timeout: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().down(mouse, point, button, timeout, self.pointer_backend(Some(target))?)
  }

  pub fn mouse_up(&self, mouse: u64) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().up(mouse)
  }

  pub fn move_mouse_to(&self, mouse: u64, point: Point) -> DriverResult<InputActionResult> {
    let result =
      auv_driver_common::mouse_input::mouse_coordinator().move_to(mouse, point, std::sync::Arc::new(crate::input::MouseBackend))?;
    // The overlay draws one cursor, so only the shared default mouse reports (see `move_mouse`).
    #[cfg(feature = "overlay")]
    if mouse == 0 {
      crate::overlay_follow::report([crate::overlay_follow::moved_to(point)]);
    }
    Ok(result)
  }

  pub fn current_position(&self) -> DriverResult<Point> {
    current_position()
  }

  /// Moves the pointer to `point` without activating the target beneath it.
  pub fn move_to(&self, point: Point) -> DriverResult<InputActionResult> {
    self.move_mouse_to(0, point)
  }

  /// Moves the pointer to `point` (screen coordinates) and issues a click.
  pub fn click_at(
    &self,
    point: Point,
    button: auv_driver_common::MouseButton,
    click: Click,
    modifiers: auv_driver_common::ClickModifiers,
  ) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    click_at(point, button, click, modifiers)
  }

  /// Moves the pointer to `point` and emits a mouse-wheel scroll.
  pub fn scroll_at(&self, point: Point, scroll: Scroll, settle: std::time::Duration) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    scroll_at(point, scroll, settle)
  }

  /// Types `text` into the current foreground target as Unicode key events.
  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    type_text(text, options)
  }

  /// Presses a single key, special key, or shortcut (e.g. `ctrl+f`).
  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    press_key(options)
  }

  pub fn key_down(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    timeout: std::time::Duration,
  ) -> DriverResult<KeyboardHold> {
    crate::input::key_down(target, keys, policy, timeout)
  }

  pub fn key_up(&self, hold: KeyboardHoldId) -> DriverResult<InputActionResult> {
    crate::input::key_up(hold)
  }

  pub fn hold_keys(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    duration: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    crate::input::hold_keys(target, keys, policy, duration)
  }

  /// Issues the system copy shortcut (Ctrl+C) against the foreground target.
  pub fn copy(&self) -> DriverResult<()> {
    copy()
  }

  /// Issues the system paste shortcut (Ctrl+V) against the foreground target.
  pub fn paste(&self) -> DriverResult<()> {
    paste()
  }
}

impl ClipboardApi<'_> {
  /// Reads the current clipboard text, or an empty string when no Unicode text
  /// is present.
  pub fn snapshot(&self) -> DriverResult<String> {
    snapshot()
  }

  /// Writes a previously captured snapshot back to the clipboard.
  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    restore_clipboard(snapshot)
  }

  /// Installs `text` as the clipboard's Unicode text payload.
  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    set_clipboard_text(text)
  }

  /// Captures every present clipboard format for exact, format-preserving
  /// restore, unlike `snapshot`, which is text-only.
  pub fn snapshot_rich(&self) -> DriverResult<ClipboardSnapshot> {
    snapshot_clipboard_rich()
  }

  /// Restores a snapshot captured by `snapshot_rich`.
  pub fn restore_rich(&self, snapshot: &ClipboardSnapshot) -> DriverResult<()> {
    restore_clipboard_rich(snapshot)
  }
}

impl PermissionApi<'_> {
  /// Probes the current process's automation readiness (UAC elevation,
  /// UIAccess/UIPI, interactive session). Never fails: undeterminable signals
  /// are reported as `PermissionStatus::Unknown`.
  pub fn probe(&self) -> WindowsPermissionProbe {
    probe_permissions()
  }
}

impl AccessibilityApi<'_> {
  /// Captures the window's accessibility tree as a flattened, depth-first node
  /// list via UI Automation.
  pub fn snapshot_window(&self, window: &Window) -> DriverResult<AxTreeSnapshot> {
    snapshot_window(window)
  }

  /// Moves keyboard focus to a node path from a recent UIA snapshot.
  pub fn focus_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    focus_node(window, node_path)
  }

  /// Selects or invokes an actionable node path from a recent UIA snapshot.
  pub fn select_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    select_node(window, node_path)
  }
}

impl DisplayApi<'_> {
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    list_displays()
  }

  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    if options.window.is_some() || options.region.is_some() {
      return Err(invalid_input("display.capture does not accept window or region capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("display.capture cannot activate an application without an application target"));
    }
    let resolution = options.resolution;
    capture_display(options.display.as_deref()).map(|captured| DisplayCapture {
      capture: captured.capture.at_resolution(resolution),
      ..captured
    })
  }

  /// Captures a target display via Windows.Graphics.Capture (WGC).
  pub fn capture_wgc(&self, selector: Option<&str>) -> DriverResult<DisplayCapture> {
    crate::wgc::capture_display_wgc(selector)
  }

  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    if options.window.is_some() {
      return Err(invalid_input("display.capture_region does not accept nested window capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("display.capture_region cannot activate an application without an application target"));
    }
    let region = options.region.ok_or_else(|| invalid_input("display.capture_region requires CaptureOptions.region"))?;
    let resolution = options.resolution;
    capture_region(options.display.as_deref(), region).map(|captured| RegionCapture {
      capture: captured.capture.at_resolution(resolution),
      ..captured
    })
  }
}

/// Translates a window-relative point into screen space.
///
/// Windows reports window frames in screen (virtual-desktop) coordinates, so
/// the mapping is a pure translation by the frame origin, mirroring the macOS
/// driver. NOTICE: this assumes `window.frame` is current; callers that need a
/// fresh frame should re-resolve the window first.
fn screen_point_for_window_point(window: &Window, point: WindowPoint) -> ScreenPoint {
  let point = point.point();
  ScreenPoint::new(window.frame.origin.x + point.x, window.frame.origin.y + point.y)
}

/// Translates a screen-space point into window-relative coordinates.
fn window_point_for_screen_point(window: &Window, point: ScreenPoint) -> WindowPoint {
  let point = point.point();
  WindowPoint::new(point.x - window.frame.origin.x, point.y - window.frame.origin.y)
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;
