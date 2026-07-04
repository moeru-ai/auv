use auv_driver::capture::{Activation, Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver::display::ObservedDisplays;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{Point, RatioRect, ScreenPoint, WindowPoint};
use auv_driver::input::{Click, InputActionResult, KeyPressOptions, Scroll, TypeTextOptions};
use auv_driver::permission::PermissionProbe;
use auv_driver::selector::WindowSelector;
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};
use auv_driver::window::Window;

use crate::accessibility::{AxTreeSnapshot, focus_node, select_node, snapshot_window};
use crate::capture::{capture_display, capture_region, list_displays};
use crate::clipboard::{restore as restore_clipboard, set_text as set_clipboard_text, snapshot};
use crate::driver::LinuxDriverSession;
use crate::error::invalid_input;
use crate::input::{click_at, press_key, scroll_at, type_text};
use crate::permission::{LinuxPortalProbe, probe_portals};
use crate::vision::{OcrMatches, find_text_in_capture, recognize_text_in_capture};
use crate::window::{capture_window, list_windows, resolve_window};

#[derive(Clone, Copy, Debug)]
pub struct DisplayApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct WindowApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct VisionApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct PermissionApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct AccessibilityApi<'a> {
  session: &'a LinuxDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  session: &'a LinuxDriverSession,
}

impl LinuxDriverSession {
  pub fn display(&self) -> DisplayApi<'_> {
    DisplayApi { session: self }
  }

  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { session: self }
  }

  pub fn vision(&self) -> VisionApi<'_> {
    VisionApi { session: self }
  }

  pub fn permission(&self) -> PermissionApi<'_> {
    PermissionApi { session: self }
  }

  pub fn accessibility(&self) -> AccessibilityApi<'_> {
    AccessibilityApi { session: self }
  }

  pub fn clipboard(&self) -> ClipboardApi<'_> {
    ClipboardApi { session: self }
  }
}

impl PermissionApi<'_> {
  pub fn probe_linux(&self) -> LinuxPortalProbe {
    let _ = self.session;
    probe_portals()
  }

  pub fn probe(&self) -> PermissionProbe {
    self.probe_linux().as_permission_probe()
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
      return Err(invalid_input(
        "display.capture does not accept window or region capture options",
      ));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input(
        "display.capture cannot activate an application without an application target",
      ));
    }
    capture_display(&self.session.state, options.display.as_deref())
  }

  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    let _ = self.session;
    if options.window.is_some() {
      return Err(invalid_input(
        "display.capture_region does not accept nested window capture options",
      ));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input(
        "display.capture_region cannot activate an application without an application target",
      ));
    }
    let region = options
      .region
      .ok_or_else(|| invalid_input("display.capture_region requires CaptureOptions.region"))?;
    capture_region(&self.session.state, options.display.as_deref(), region)
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

  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    let _ = self.session;
    capture_window(&self.session.state, window)
  }

  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    let _ = self.session;
    let point = point.point();
    Ok(ScreenPoint::new(
      window.frame.origin.x + point.x,
      window.frame.origin.y + point.y,
    ))
  }

  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    let _ = self.session;
    let point = point.point();
    Ok(WindowPoint::new(
      point.x - window.frame.origin.x,
      point.y - window.frame.origin.y,
    ))
  }
}

impl VisionApi<'_> {
  pub fn recognize_text_in_capture(
    &self,
    capture: &Capture,
    region: RatioRect,
  ) -> DriverResult<TextRecognition> {
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

  pub fn find_text_in_capture(
    &self,
    capture: &Capture,
    query: &str,
    region: RatioRect,
  ) -> DriverResult<OcrMatches> {
    self.find_text_in_capture_with_options(
      capture,
      query,
      region,
      TextRecognitionOptions::default(),
    )
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

impl AccessibilityApi<'_> {
  pub fn snapshot_window(&self, window: &Window) -> DriverResult<AxTreeSnapshot> {
    let _ = self.session;
    snapshot_window(window)
  }

  pub fn focus_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    let _ = self.session;
    focus_node(window, node_path)
  }

  pub fn select_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    let _ = self.session;
    select_node(window, node_path)
  }
}

impl InputApi<'_> {
  pub fn click_at(&self, point: Point, click: Click) -> DriverResult<InputActionResult> {
    click_at(&self.session.state, point, click)
  }

  pub fn scroll_at(
    &self,
    point: Point,
    scroll: Scroll,
    settle: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    scroll_at(&self.session.state, point, scroll, settle)
  }

  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    type_text(&self.session.state, text, options)
  }

  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    press_key(&self.session.state, options)
  }
}

impl ClipboardApi<'_> {
  /// Reads the current Wayland clipboard text, or an empty string when no text
  /// payload is present.
  pub fn snapshot(&self) -> DriverResult<String> {
    snapshot(&self.session.state)
  }

  /// Writes a previously captured text snapshot back to the Wayland clipboard.
  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    restore_clipboard(&self.session.state, snapshot)
  }

  /// Installs `text` as the Wayland clipboard's UTF-8 text payload.
  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    set_clipboard_text(&self.session.state, text)
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::Driver;
  use auv_driver::geometry::{CoordinateSpace, Rect, ScreenPoint, WindowPoint};
  use auv_driver::window::{Window, WindowRef};

  use super::*;
  use crate::LinuxDriver;

  fn session() -> LinuxDriverSession {
    LinuxDriver::new().open_local().expect("session opens")
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

    let point = session()
      .window()
      .to_screen_point(&window, WindowPoint::new(25.0, 30.0))
      .expect("point maps");

    assert_eq!(point, ScreenPoint::new(125.0, 230.0));
  }

  #[test]
  fn screen_point_converts_to_window_point() {
    let window = sample_window();

    let point = session()
      .window()
      .to_window_point(&window, ScreenPoint::new(125.0, 230.0))
      .expect("point maps");

    assert_eq!(point, WindowPoint::new(25.0, 30.0));
  }
}
