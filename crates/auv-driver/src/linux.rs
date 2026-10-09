use std::ffi::OsStr;

use auv_driver_common::{
  Capture, CaptureOptions, Click, ClickModifiers, ClickOptions, DisplayCapture, DriverError, DriverResult, InputActionResult, InputPolicy,
  InputTarget, KeyPressOptions, KeyboardInput, KeyboardInputError, ObservedDisplays, PasteTextOptions, PermissionProbe, Point,
  PressKeysOptions, RegionCapture, RelativeRect, ScreenPoint, Scroll, ScrollOptions, TextRecognition, TextRecognitionOptions,
  TypeTextOptions, WaitOptions, Window, WindowInput, WindowPoint, WindowSelector, input::MouseButton, vision::OcrMatches,
};
use auv_driver_linux::{AxTreeSnapshot, LinuxDriverDescriptor, LinuxPortalProbe};

use crate::LocalDriverSession;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
  Wayland,
  X11,
}

pub(crate) fn selected_backend_from_process() -> Backend {
  selected_backend(std::env::var_os("WAYLAND_DISPLAY").as_deref(), std::env::var_os("DISPLAY").as_deref())
}

fn selected_backend(wayland_display: Option<&OsStr>, display: Option<&OsStr>) -> Backend {
  let has_wayland = wayland_display.is_some_and(|value| !value.is_empty());
  let has_x11 = display.is_some_and(|value| !value.is_empty());
  // A nonempty Wayland socket remains authoritative when XWayland also exports
  // DISPLAY. With no desktop variables, preserve the prior Wayland diagnostics.
  if !has_wayland && has_x11 {
    Backend::X11
  } else {
    Backend::Wayland
  }
}

#[derive(Clone, Copy, Debug)]
pub struct DisplayApi<'a> {
  session: &'a LocalDriverSession,
}

impl DisplayApi<'_> {
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    match self.session {
      LocalDriverSession::Linux(session) => session.display().list(),
      LocalDriverSession::LinuxX11(session) => session.display().list(),
    }
  }

  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    match self.session {
      LocalDriverSession::Linux(session) => session.display().capture(options),
      LocalDriverSession::LinuxX11(session) => session.display().capture(options),
    }
  }

  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
    match self.session {
      LocalDriverSession::Linux(session) => session.display().capture_region(options),
      LocalDriverSession::LinuxX11(session) => session.display().capture_region(options),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  session: &'a LocalDriverSession,
}

impl InputApi<'_> {
  pub fn key_down(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    timeout: std::time::Duration,
  ) -> DriverResult<auv_driver_common::KeyboardHold> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().key_down(target, keys, policy, timeout),
      LocalDriverSession::LinuxX11(session) => session.input().key_down(target, keys, policy, timeout),
    }
  }

  pub fn key_up(&self, hold: auv_driver_common::KeyboardHoldId) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().key_up(hold),
      LocalDriverSession::LinuxX11(session) => session.input().key_up(hold),
    }
  }

  pub fn hold_keys(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    duration: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().hold_keys(target, keys, policy, duration),
      LocalDriverSession::LinuxX11(session) => session.input().hold_keys(target, keys, policy, duration),
    }
  }

  pub fn input_keyboard(
    &self,
    target: &InputTarget,
    inputs: Vec<KeyboardInput>,
    dry_run: bool,
  ) -> Result<Option<Vec<InputActionResult>>, KeyboardInputError> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().input_keyboard(target, inputs, dry_run),
      LocalDriverSession::LinuxX11(session) => session.input().input_keyboard(target, inputs, dry_run),
    }
  }

  pub fn drag_mouse(&self, request: auv_driver_common::MoveMouseRequest, button: MouseButton) -> DriverResult<(Point, InputActionResult)> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().drag_mouse(request, button),
      LocalDriverSession::LinuxX11(session) => session.input().drag_mouse(request, button),
    }
  }

  pub fn move_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    notify: impl FnMut(auv_driver_common::mouse_input::MotionEvent) -> bool,
  ) -> DriverResult<(Point, InputActionResult)> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().move_mouse(request, notify),
      LocalDriverSession::LinuxX11(session) => session.input().move_mouse(request, notify),
    }
  }

  pub fn hold_mouse(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    duration: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().hold_mouse(target, mouse, point, button, duration),
      LocalDriverSession::LinuxX11(session) => session.input().hold_mouse(target, mouse, point, button, duration),
    }
  }

  pub fn create_mouse(&self) -> DriverResult<u64> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().create_mouse(),
      LocalDriverSession::LinuxX11(session) => session.input().create_mouse(),
    }
  }

  pub fn remove_mouse(&self, mouse: u64) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().remove_mouse(mouse),
      LocalDriverSession::LinuxX11(session) => session.input().remove_mouse(mouse),
    }
  }

  pub fn mouse_down(
    &self,
    target: &InputTarget,
    mouse: u64,
    point: Point,
    button: MouseButton,
    timeout: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().mouse_down(target, mouse, point, button, timeout),
      LocalDriverSession::LinuxX11(session) => session.input().mouse_down(target, mouse, point, button, timeout),
    }
  }

  pub fn mouse_up(&self, mouse: u64) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().mouse_up(mouse),
      LocalDriverSession::LinuxX11(session) => session.input().mouse_up(mouse),
    }
  }

  pub fn move_mouse_to(&self, mouse: u64, point: Point) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().move_mouse_to(mouse, point),
      LocalDriverSession::LinuxX11(session) => session.input().move_mouse_to(mouse, point),
    }
  }

  pub fn current_position(&self) -> DriverResult<Point> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().current_position(),
      LocalDriverSession::LinuxX11(session) => session.input().current_position(),
    }
  }

  pub fn move_to(&self, point: Point) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().move_to(point),
      LocalDriverSession::LinuxX11(session) => session.input().move_to(point),
    }
  }

  pub fn click_at(&self, point: Point, button: MouseButton, click: Click, modifiers: ClickModifiers) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().click_at(point, button, click, modifiers),
      LocalDriverSession::LinuxX11(session) => session.input().click_button_at(point, button, click, modifiers),
    }
  }

  pub fn click_button_at(
    &self,
    point: Point,
    button: MouseButton,
    click: Click,
    modifiers: ClickModifiers,
  ) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().click_at(point, button, click, modifiers),
      LocalDriverSession::LinuxX11(session) => session.input().click_button_at(point, button, click, modifiers),
    }
  }

  pub fn drag(&self, from: Point, to: Point, button: MouseButton) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(_) => Err(DriverError::unsupported("Linux Wayland screen drag")),
      LocalDriverSession::LinuxX11(session) => session.input().drag(from, to, button),
    }
  }

  pub fn scroll_at(&self, point: Point, scroll: Scroll, settle: std::time::Duration) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().scroll_at(point, scroll, settle),
      LocalDriverSession::LinuxX11(session) => session.input().scroll_at(point, scroll, settle),
    }
  }

  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().type_text(text, options),
      LocalDriverSession::LinuxX11(session) => session.input().type_text(text, options),
    }
  }

  pub fn paste_text(&self, options: PasteTextOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().paste_text(options),
      // TODO(x11-clipboard): add this only with an approved X11 selection
      // ownership and restoration contract; see the X11 compatibility note.
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard paste")),
    }
  }

  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().press_key(options),
      LocalDriverSession::LinuxX11(session) => session.input().press_key(options),
    }
  }

  pub fn press_keys(&self, options: PressKeysOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session
        .input()
        .input_keyboard(
          &InputTarget::Foreground,
          vec![KeyboardInput::PressKeys {
            options,
            policy: InputPolicy::ForegroundPreferred,
          }],
          false,
        )
        .map_err(|error| error.cause)?
        .and_then(|mut actions| actions.pop())
        .ok_or_else(|| DriverError::Backend {
          message: "Linux keyboard batch returned no action".into(),
        }),
      LocalDriverSession::LinuxX11(session) => session.input().press_keys(options),
    }
  }

  pub fn copy(&self) -> DriverResult<()> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().copy(),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard copy")),
    }
  }

  pub fn paste(&self) -> DriverResult<()> {
    match self.session {
      LocalDriverSession::Linux(session) => session.input().paste(),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard paste")),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowApi<'a> {
  session: &'a LocalDriverSession,
}

// TODO(x11-window-api): X11 window observation/targeted delivery remains
// outside this integration slice; add it only with an owner-approved contract.
impl WindowApi<'_> {
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().list(),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.list")),
    }
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().resolve(selector),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.resolve")),
    }
  }

  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().capture(window),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.capture")),
    }
  }

  pub fn capture_with(&self, window: &Window, options: CaptureOptions) -> DriverResult<Capture> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().capture_with(window, options),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.capture_with")),
    }
  }

  pub fn find_text(&self, window: &Window, query: &str, region: RelativeRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().find_text(window, query, region, wait),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.find_text")),
    }
  }

  pub fn wait_text(&self, window: &Window, query: &str, region: RelativeRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().wait_text(window, query, region, wait),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.wait_text")),
    }
  }

  #[allow(clippy::wrong_self_convention)] // Preserve the former Wayland facade signature.
  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().to_screen_point(window, point),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.to_screen_point")),
    }
  }

  #[allow(clippy::wrong_self_convention)] // Preserve the former Wayland facade signature.
  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().to_window_point(window, point),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.to_window_point")),
    }
  }
}

impl WindowInput for WindowApi<'_> {
  fn click(&self, window: &Window, point: WindowPoint, options: ClickOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().click(window, point, options),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.click")),
    }
  }

  fn scroll(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().scroll(window, point, scroll, options),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.scroll")),
    }
  }

  fn drag(
    &self,
    window: &Window,
    movement: auv_driver_common::MoveMouseRequest,
    button: MouseButton,
    policy: InputPolicy,
  ) -> DriverResult<(Point, InputActionResult)> {
    match self.session {
      LocalDriverSession::Linux(session) => session.window().drag(window, movement, button, policy),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 window.drag")),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct VisionApi<'a> {
  session: &'a LocalDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct PermissionApi<'a> {
  session: &'a LocalDriverSession,
}

impl PermissionApi<'_> {
  pub fn authorize_portals(&self) -> DriverResult<()> {
    match self.session {
      LocalDriverSession::Linux(session) => session.permission().authorize_portals(),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 portal authorization")),
    }
  }

  pub fn probe_linux(&self) -> LinuxPortalProbe {
    match self.session {
      LocalDriverSession::Linux(session) => session.permission().probe_linux(),
      LocalDriverSession::LinuxX11(_) => auv_driver_linux::probe_portals(),
    }
  }

  pub fn probe(&self) -> PermissionProbe {
    match self.session {
      LocalDriverSession::Linux(session) => session.permission().probe(),
      LocalDriverSession::LinuxX11(_) => self.probe_linux().as_permission_probe(),
    }
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
    match self.session {
      LocalDriverSession::Linux(session) => session.vision().recognize_text_in_capture_with_options(capture, region, options),
      LocalDriverSession::LinuxX11(_) => auv_driver_linux::vision::recognize_text_in_capture(capture, region, &options),
    }
  }

  pub fn find_text_in_capture_with_options(
    &self,
    capture: &Capture,
    query: &str,
    region: RelativeRect,
    options: TextRecognitionOptions,
  ) -> DriverResult<OcrMatches> {
    match self.session {
      LocalDriverSession::Linux(session) => session.vision().find_text_in_capture_with_options(capture, query, region, options),
      LocalDriverSession::LinuxX11(_) => auv_driver_linux::vision::find_text_in_capture(capture, query, region, &options),
    }
  }

  pub fn find_text_in_capture(&self, capture: &Capture, query: &str, region: RelativeRect) -> DriverResult<OcrMatches> {
    self.find_text_in_capture_with_options(capture, query, region, TextRecognitionOptions::default())
  }
}

#[derive(Clone, Copy, Debug)]
pub struct AccessibilityApi<'a> {
  session: &'a LocalDriverSession,
}

impl AccessibilityApi<'_> {
  pub fn snapshot_window(&self, window: &Window) -> DriverResult<AxTreeSnapshot> {
    match self.session {
      LocalDriverSession::Linux(session) => session.accessibility().snapshot_window(window),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 accessibility.snapshot_window")),
    }
  }

  pub fn focus_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.accessibility().focus_node(window, node_path),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 accessibility.focus_node")),
    }
  }

  pub fn select_node(&self, window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
    match self.session {
      LocalDriverSession::Linux(session) => session.accessibility().select_node(window, node_path),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 accessibility.select_node")),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  session: &'a LocalDriverSession,
}

impl ClipboardApi<'_> {
  pub fn snapshot(&self) -> DriverResult<String> {
    match self.session {
      LocalDriverSession::Linux(session) => session.clipboard().snapshot(),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard.snapshot")),
    }
  }

  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    match self.session {
      LocalDriverSession::Linux(session) => session.clipboard().restore(snapshot),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard.restore")),
    }
  }

  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    match self.session {
      LocalDriverSession::Linux(session) => session.clipboard().set_text(text),
      LocalDriverSession::LinuxX11(_) => Err(DriverError::unsupported("X11 clipboard.set_text")),
    }
  }
}

impl LocalDriverSession {
  pub fn linux_descriptor(&self) -> LinuxDriverDescriptor {
    match self {
      Self::Linux(session) => session.linux_descriptor(),
      Self::LinuxX11(_) => LinuxDriverDescriptor {
        id: "linux.x11",
        platform: auv_driver_common::PlatformKind::Linux,
        description: "Legacy X11 display capture and foreground XTEST input; not recommended for new deployments.",
      },
    }
  }

  pub fn display(&self) -> DisplayApi<'_> {
    DisplayApi { session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { session: self }
  }

  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
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

#[cfg(test)]
mod tests {
  use super::*;

  // This compile-only contract mirrors the former Deref surface. Keeping it in
  // the facade crate catches accidental method loss before downstream builds.
  #[allow(dead_code)]
  fn existing_wayland_session_surface_compiles(session: &LocalDriverSession, window: &Window, capture: &Capture) {
    let _ = session.linux_descriptor();
    let _ = session.display().list();
    let _ = session.display().capture(CaptureOptions::default());
    let _ = session.display().capture_region(CaptureOptions::default());
    let _ = session.window().list();
    let _ = session.window().resolve(WindowSelector::default());
    let _ = session.window().capture(window);
    let _ = session.window().capture_with(window, CaptureOptions::default());
    let _ = session.window().find_text(window, "query", RelativeRect::new(0.0, 0.0, 1.0, 1.0), WaitOptions::default());
    let _ = session.window().wait_text(window, "query", RelativeRect::new(0.0, 0.0, 1.0, 1.0), WaitOptions::default());
    let _ = session.window().to_screen_point(window, WindowPoint::new(0.0, 0.0));
    let _ = session.window().to_window_point(window, ScreenPoint::new(0.0, 0.0));
    let _ = session.input().current_position();
    let _ = session.input().move_to(Point::new(0.0, 0.0));
    let _ = session.input().click_at(Point::new(0.0, 0.0), MouseButton::Left, Click::Single, ClickModifiers::default());
    let _ = session.input().key_down(
      &InputTarget::Foreground,
      vec!["A".into()],
      InputPolicy::ForegroundPreferred,
      std::time::Duration::from_secs(1),
    );
    let _ = session.input().key_up(1);
    let _ =
      session.input().hold_keys(&InputTarget::Foreground, vec!["A".into()], InputPolicy::ForegroundPreferred, std::time::Duration::ZERO);
    let _ = session.input().drag_mouse(auv_driver_common::MoveMouseRequest::direct(Point::new(0.0, 0.0)), MouseButton::Left);
    let _ = session.input().move_mouse(auv_driver_common::MoveMouseRequest::direct(Point::new(0.0, 0.0)), |_| true);
    let _ = session.input().create_mouse();
    let _ = session.input().remove_mouse(1);
    let _ =
      session.input().mouse_down(&InputTarget::Foreground, 1, Point::new(0.0, 0.0), MouseButton::Left, std::time::Duration::from_secs(1));
    let _ = session.input().mouse_up(1);
    let _ = session.input().move_mouse_to(1, Point::new(0.0, 0.0));
    let _ = session.input().hold_mouse(&InputTarget::Foreground, 1, Point::new(0.0, 0.0), MouseButton::Left, std::time::Duration::ZERO);
    let _ = session.input().scroll_at(Point::new(0.0, 0.0), Scroll::new(0.0, 1.0), std::time::Duration::ZERO);
    let _ = session.input().type_text("text", TypeTextOptions::default());
    let _ = session.input().press_key(KeyPressOptions::default());
    let _ = session.input().copy();
    let _ = session.input().paste();
    let _ = session.input().paste_text(PasteTextOptions::default());
    let _ = session.vision().recognize_text_in_capture(capture, RelativeRect::new(0.0, 0.0, 1.0, 1.0));
    let _ = session.vision().recognize_text_in_capture_with_options(
      capture,
      RelativeRect::new(0.0, 0.0, 1.0, 1.0),
      TextRecognitionOptions::default(),
    );
    let _ = session.vision().find_text_in_capture(capture, "query", RelativeRect::new(0.0, 0.0, 1.0, 1.0));
    let _ = session.vision().find_text_in_capture_with_options(
      capture,
      "query",
      RelativeRect::new(0.0, 0.0, 1.0, 1.0),
      TextRecognitionOptions::default(),
    );
    let _ = session.permission().authorize_portals();
    let _ = session.permission().probe_linux();
    let _ = session.permission().probe();
    let _ = session.accessibility().snapshot_window(window);
    let _ = session.accessibility().focus_node(window, "0");
    let _ = session.accessibility().select_node(window, "0");
    let _ = session.clipboard().snapshot();
    let _ = session.clipboard().restore("snapshot");
    let _ = session.clipboard().set_text("text");
  }

  #[test]
  fn pure_x11_environment_selects_x11() {
    assert_eq!(selected_backend(None, Some(OsStr::new(":99"))), Backend::X11);
    assert_eq!(selected_backend(Some(OsStr::new("")), Some(OsStr::new(":99"))), Backend::X11);
  }

  #[test]
  fn wayland_environment_remains_wayland_even_with_xwayland_display() {
    assert_eq!(selected_backend(Some(OsStr::new("wayland-0")), Some(OsStr::new(":0"))), Backend::Wayland);
  }

  #[test]
  fn missing_desktop_environment_preserves_wayland_default() {
    assert_eq!(selected_backend(None, None), Backend::Wayland);
    assert_eq!(selected_backend(None, Some(OsStr::new(""))), Backend::Wayland);
  }
}
