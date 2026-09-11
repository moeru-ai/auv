use std::thread;

use auv_driver_common::capture::{Activation, Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver_common::display::ObservedDisplays;
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::{Point, RatioRect, Rect, ScreenPoint, Size, WindowPoint};
use auv_driver_common::input::{
  Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, KeyPressOptions, Scroll,
  ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions, TypeTextOptions, WaitOptions, WindowInput,
};
use auv_driver_common::selector::WindowSelector;
use auv_driver_common::vision::{TextRecognition, TextRecognitionOptions};
use auv_driver_common::window::{Window, WindowMutationKind, WindowMutationOptions, WindowMutationResult};

use crate::accessibility::{AxTreeSnapshot, focus_node, select_node, snapshot_window};
use crate::background_input;
use crate::capture::{capture_display, capture_region, capture_window, list_displays};
use crate::clipboard::{
  ClipboardSnapshot, restore as restore_clipboard, restore_rich as restore_clipboard_rich, set_text as set_clipboard_text, snapshot,
  snapshot_rich as snapshot_clipboard_rich,
};
use crate::driver::WindowsDriverSession;
use crate::error::{invalid_input, not_found};
use crate::input::{click_at, copy, current_position, move_to, paste, press_key, scroll_at, type_text};
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
  session: &'a WindowsDriverSession,
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
  session: &'a WindowsDriverSession,
}

/// Foreground pointer and keyboard input capabilities.
///
/// Mirrors the macOS driver's `InputApi`. Every primitive is delivered as a
/// foreground synthetic event via `SendInput`, since Windows has no
/// accessibility-targeted input path.
#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  session: &'a WindowsDriverSession,
}

/// Text and rich clipboard snapshot/restore/set capabilities.
///
/// Mirrors the macOS driver's `ClipboardApi` for the text-only `snapshot`/
/// `restore`/`set_text` methods. `snapshot_rich`/`restore_rich` are a
/// Windows-only addition that preserve every memory-backed clipboard format,
/// not just text.
#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  session: &'a WindowsDriverSession,
}

/// Process-level automation readiness capabilities.
///
/// Mirrors the macOS driver's `PermissionApi`, but probes the Windows process
/// token and session (UAC elevation, UIAccess/UIPI, interactive session)
/// instead of macOS TCC permissions.
#[derive(Clone, Copy, Debug)]
pub struct PermissionApi<'a> {
  session: &'a WindowsDriverSession,
}

/// Window accessibility tree inspection capabilities.
///
/// Mirrors the macOS driver's AX tree capture, but reads the Microsoft UI
/// Automation tree for a window instead of the macOS `AXUIElement` tree.
#[derive(Clone, Copy, Debug)]
pub struct AccessibilityApi<'a> {
  session: &'a WindowsDriverSession,
}

/// Overlay show/remove capabilities.
///
/// Mirrors the macOS driver's `OverlayApi`, dispatching through the shared
/// `auv-driver-overlay` facade with its `windows` backend enabled.
#[cfg(feature = "overlay")]
#[derive(Clone, Copy, Debug)]
pub struct OverlayApi<'a> {
  session: &'a WindowsDriverSession,
}

impl WindowsDriverSession {
  pub fn display(&self) -> DisplayApi<'_> {
    DisplayApi { session: self }
  }

  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
  }

  pub fn vision(&self) -> VisionApi<'_> {
    VisionApi { session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { session: self }
  }

  pub fn clipboard(&self) -> ClipboardApi<'_> {
    ClipboardApi { session: self }
  }

  pub fn permission(&self) -> PermissionApi<'_> {
    PermissionApi { session: self }
  }

  pub fn accessibility(&self) -> AccessibilityApi<'_> {
    AccessibilityApi { session: self }
  }

  #[cfg(feature = "overlay")]
  pub fn overlay(&self) -> OverlayApi<'_> {
    OverlayApi { session: self }
  }
}

#[cfg(feature = "overlay")]
impl OverlayApi<'_> {
  pub fn show(&self, overlay: &Overlay, options: ShowOptions) -> DriverResult<()> {
    let _ = self.session;
    auv_driver_overlay::show(overlay, options).map_err(|error| auv_driver_common::error::DriverError::Backend {
      message: error.to_string(),
    })
  }

  pub fn remove(&self) -> DriverResult<()> {
    let _ = self.session;
    auv_driver_overlay::remove().map_err(|error| auv_driver_common::error::DriverError::Backend {
      message: error.to_string(),
    })
  }
}

impl WindowApi<'_> {
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    let _ = self.session;
    list_windows()
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    let _ = self.session;
    resolve_window(&selector)
  }

  /// Restores and foregrounds a window before foreground-only input delivery.
  pub fn activate(&self, window: &Window) -> DriverResult<()> {
    let _ = self.session;
    activate_window(window)
  }

  /// Captures a single window's pixels via Win32 GDI `PrintWindow`.
  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    let _ = self.session;
    capture_window(window)
  }

  /// Maps a window-relative point to its absolute screen position by offsetting
  /// against the window's screen-space frame origin.
  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    let _ = self.session;
    Ok(screen_point_for_window_point(window, point))
  }

  /// Maps an absolute screen point into window-relative coordinates.
  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    let _ = self.session;
    Ok(window_point_for_screen_point(window, point))
  }

  /// Polls `window`'s capture for `query` text until it appears or `wait`'s
  /// timeout elapses, returning whatever matches (possibly none) were last
  /// observed.
  pub fn find_text(&self, window: &Window, query: &str, region: RatioRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
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
  pub fn wait_text(&self, window: &Window, query: &str, region: RatioRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    let matches = self.find_text(window, query, region, wait)?;
    if matches.matches.is_empty() {
      Err(not_found(format!("text {query:?} before timeout")))
    } else {
      Ok(matches)
    }
  }

  pub fn move_to(&self, window: &Window, point: Point, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    mutate_window(window, WindowMutationKind::MoveTo { point }, options)
  }

  pub fn resize(&self, window: &Window, size: Size, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    mutate_window(window, WindowMutationKind::Resize { size }, options)
  }

  pub fn set_frame(&self, window: &Window, frame: Rect, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    mutate_window(window, WindowMutationKind::SetFrame { frame }, options)
  }

  pub fn minimize(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    mutate_window(window, WindowMutationKind::Minimize, options)
  }

  pub fn restore(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    mutate_window(window, WindowMutationKind::Restore, options)
  }

  pub fn zoom(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
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
    let screen_point = self.to_screen_point(window, point)?.point();
    if matches!(options.policy, InputPolicy::ForegroundPreferred) {
      let activation_attempt = foreground_window_attempt(window, "pointer delivery");
      let mut result = self.session.input().click_at(screen_point, options.click)?;
      result.attempts.insert(0, activation_attempt);
      return Ok(result);
    }
    let _ = options.window_strategy;
    background_input::click_at_window(window, screen_point, options.click)?;
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
    self.click_impl(window, point, options)
  }

  fn scroll(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    self.scroll_impl(window, point, scroll, options)
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
  pub fn recognize_text_in_capture(&self, capture: &Capture, region: RatioRect) -> DriverResult<TextRecognition> {
    self.recognize_text_in_capture_with_options(capture, region, TextRecognitionOptions::default())
  }

  pub fn recognize_text_in_capture_with_options(
    &self,
    capture: &Capture,
    region: RatioRect,
    options: TextRecognitionOptions,
  ) -> DriverResult<TextRecognition> {
    let _ = self.session;
    recognize_text_in_capture(capture, region, &options)
  }

  pub fn find_text_in_capture(&self, capture: &Capture, query: &str, region: RatioRect) -> DriverResult<OcrMatches> {
    self.find_text_in_capture_with_options(capture, query, region, TextRecognitionOptions::default())
  }

  pub fn find_text_in_capture_with_options(
    &self,
    capture: &Capture,
    query: &str,
    region: RatioRect,
    options: TextRecognitionOptions,
  ) -> DriverResult<OcrMatches> {
    let _ = self.session;
    find_text_in_capture(capture, query, region, &options)
  }
}

impl InputApi<'_> {
  pub fn current_position(&self) -> DriverResult<Point> {
    let _ = self.session;
    current_position()
  }

  /// Moves the pointer to `point` without activating the target beneath it.
  pub fn move_to(&self, point: Point) -> DriverResult<InputActionResult> {
    let _ = self.session;
    move_to(point)
  }

  /// Moves the pointer to `point` (screen coordinates) and issues a click.
  pub fn click_at(&self, point: Point, click: Click) -> DriverResult<InputActionResult> {
    let _ = self.session;
    click_at(point, click)
  }

  /// Moves the pointer to `point` and emits a mouse-wheel scroll.
  pub fn scroll_at(&self, point: Point, scroll: Scroll, settle: std::time::Duration) -> DriverResult<InputActionResult> {
    let _ = self.session;
    scroll_at(point, scroll, settle)
  }

  /// Types `text` into the current foreground target as Unicode key events.
  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    let _ = self.session;
    type_text(text, options)
  }

  /// Presses a single key, special key, or shortcut (e.g. `ctrl+f`).
  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    let _ = self.session;
    press_key(options)
  }

  /// Issues the system copy shortcut (Ctrl+C) against the foreground target.
  pub fn copy(&self) -> DriverResult<()> {
    let _ = self.session;
    copy()
  }

  /// Issues the system paste shortcut (Ctrl+V) against the foreground target.
  pub fn paste(&self) -> DriverResult<()> {
    let _ = self.session;
    paste()
  }
}

impl ClipboardApi<'_> {
  /// Reads the current clipboard text, or an empty string when no Unicode text
  /// is present.
  pub fn snapshot(&self) -> DriverResult<String> {
    let _ = self.session;
    snapshot()
  }

  /// Writes a previously captured snapshot back to the clipboard.
  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    let _ = self.session;
    restore_clipboard(snapshot)
  }

  /// Installs `text` as the clipboard's Unicode text payload.
  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    let _ = self.session;
    set_clipboard_text(text)
  }

  /// Captures every present clipboard format for exact, format-preserving
  /// restore, unlike `snapshot`, which is text-only.
  pub fn snapshot_rich(&self) -> DriverResult<ClipboardSnapshot> {
    let _ = self.session;
    snapshot_clipboard_rich()
  }

  /// Restores a snapshot captured by `snapshot_rich`.
  pub fn restore_rich(&self, snapshot: &ClipboardSnapshot) -> DriverResult<()> {
    let _ = self.session;
    restore_clipboard_rich(snapshot)
  }
}

impl PermissionApi<'_> {
  /// Probes the current process's automation readiness (UAC elevation,
  /// UIAccess/UIPI, interactive session). Never fails: undeterminable signals
  /// are reported as `PermissionStatus::Unknown`.
  pub fn probe(&self) -> WindowsPermissionProbe {
    let _ = self.session;
    probe_permissions()
  }
}

impl AccessibilityApi<'_> {
  /// Captures the window's accessibility tree as a flattened, depth-first node
  /// list via UI Automation.
  pub fn snapshot_window(&self, window: &Window) -> DriverResult<AxTreeSnapshot> {
    let _ = self.session;
    snapshot_window(window)
  }

  /// Moves keyboard focus to a node path from a recent UIA snapshot.
  pub fn focus_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    let _ = self.session;
    focus_node(window, node_path)
  }

  /// Selects or invokes an actionable node path from a recent UIA snapshot.
  pub fn select_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    let _ = self.session;
    select_node(window, node_path)
  }
}

impl DisplayApi<'_> {
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    let _ = self.session;
    list_displays()
  }

  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    let _ = self.session;
    if options.window.is_some() || options.region.is_some() {
      return Err(invalid_input("display.capture does not accept window or region capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("display.capture cannot activate an application without an application target"));
    }
    capture_display(options.display.as_deref())
  }

  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    let _ = self.session;
    if options.window.is_some() {
      return Err(invalid_input("display.capture_region does not accept nested window capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("display.capture_region cannot activate an application without an application target"));
    }
    let region = options.region.ok_or_else(|| invalid_input("display.capture_region requires CaptureOptions.region"))?;
    capture_region(options.display.as_deref(), region)
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
