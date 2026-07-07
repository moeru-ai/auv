use auv_driver::capture::{Activation, Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver::display::ObservedDisplays;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{Point, RatioRect, Rect, ScreenPoint, Size, WindowPoint};
use auv_driver::input::{Click, InputActionResult, KeyPressOptions, Scroll, TypeTextOptions};
use auv_driver::selector::WindowSelector;
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};
use auv_driver::window::{Window, WindowMutationKind, WindowMutationOptions, WindowMutationResult};

use crate::accessibility::{AxTreeSnapshot, focus_node, select_node, snapshot_window};
use crate::capture::{capture_display, capture_region, capture_window, list_displays};
use crate::clipboard::{restore as restore_clipboard, set_text as set_clipboard_text, snapshot};
use crate::driver::WindowsDriverSession;
use crate::error::invalid_input;
use crate::input::{click_at, copy, paste, press_key, scroll_at, type_text};
use crate::mutation::mutate_window;
use crate::permission::{WindowsPermissionProbe, probe as probe_permissions};
use crate::vision::{OcrMatches, find_text_in_capture, recognize_text_in_capture};
use crate::window::{activate_window, list_windows, resolve_window};

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

/// Text clipboard snapshot/restore/set capabilities.
///
/// Mirrors the macOS driver's `ClipboardApi`, modeling the clipboard as a
/// single text payload over the Win32 clipboard.
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
mod tests {
  use auv_driver::Driver;
  use auv_driver::capture::{Activation, CaptureOptions};
  use auv_driver::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
  use auv_driver::window::{Window, WindowRef};

  use super::{screen_point_for_window_point, window_point_for_screen_point};
  use crate::WindowsDriver;

  fn session() -> crate::WindowsDriverSession {
    WindowsDriver::new().open_local().expect("session opens")
  }

  fn sample_window() -> Window {
    Window {
      reference: WindowRef {
        id: "42".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: Some(123),
      frame: Rect::new(100.0, 200.0, 800.0, 600.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    }
  }

  #[test]
  fn window_point_converts_to_screen_point() {
    let window = sample_window();

    let point = screen_point_for_window_point(&window, WindowPoint::new(25.0, 30.0));

    assert_eq!(point, ScreenPoint::new(125.0, 230.0));
  }

  #[test]
  fn screen_point_converts_to_window_point() {
    let window = sample_window();

    let point = window_point_for_screen_point(&window, ScreenPoint::new(125.0, 230.0));

    assert_eq!(point, WindowPoint::new(25.0, 30.0));
  }

  #[test]
  fn capture_rejects_region_option() {
    let options = CaptureOptions {
      region: Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
      ..CaptureOptions::default()
    };

    assert!(session().display().capture(options).is_err());
  }

  #[test]
  fn capture_rejects_activation_without_app_target() {
    let options = CaptureOptions {
      activation: Activation::ActivateFirst {
        settle: std::time::Duration::from_millis(0),
      },
      ..CaptureOptions::default()
    };

    assert!(session().display().capture(options).is_err());
  }

  #[test]
  fn capture_region_requires_region() {
    assert!(session().display().capture_region(CaptureOptions::default()).is_err());
  }
}
