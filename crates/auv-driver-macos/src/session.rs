use std::collections::HashSet;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use auv_driver_capture::{Activation, Capture, CaptureOptions, CaptureResolution, DisplayCapture, RegionCapture};
use auv_driver_common::display::{Display, ObservedDisplays};
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::geometry::{CoordinateSpace, Point, Positional, Rect, RelativeRect, ScreenPoint, Size, WindowPoint};
use auv_driver_common::input::{
  ActivationPolicy, Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy,
  InputPreparationLease, InputTarget, KeyPressOptions, KeyboardInput, KeyboardInputError, PasteTextOptions, PrepareForInputOptions,
  PressKeysOptions, Scroll, ScrollDeliveryCandidate, ScrollOptions, TextSubmit, TypeTextOptions, WaitOptions, WindowClickStrategy,
  WindowInput,
};
use auv_driver_common::permission::{PermissionProbe, PermissionStatus};
use auv_driver_common::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver_common::vision::{OcrMatch, OcrMatches, RecognizedText, TextRecognition, TextRecognitionOptions};
use auv_driver_common::window::{
  Window, WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind, WindowMutationOptions, WindowMutationPath,
  WindowMutationPolicy, WindowMutationResult, WindowMutationVerification, WindowRef, WindowState, WindowStateTarget, find_window,
  select_main_window,
};
#[cfg(target_os = "macos")]
use image::RgbaImage;

use crate::driver::{MacosDriverSession, MaximizedFrames};
use crate::native::ocr::NativeOcrTextCapture;
use crate::native::types::{ObservedDisplay, ObservedRect, ObservedWindow, ObservedWindowSnapshot};
use crate::native::window::ListWindowsOptions;
use crate::support::{AppLiveness, app_resolution_message, filter_windows_for_app, parse_app_selector, resolve_app_ref};
use crate::types::{WindowRef as NativeWindowRef, WindowSelection};

#[cfg(feature = "overlay")]
use auv_driver_overlay::{Overlay, ShowOptions};

#[derive(Clone, Copy, Debug)]
pub struct WindowApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct InputApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipboardApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct DisplayApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct VisionApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct PermissionApi<'a> {
  session: &'a MacosDriverSession,
}

#[derive(Clone, Copy, Debug)]
pub struct AccessibilityApi<'a> {
  session: &'a MacosDriverSession,
}

#[cfg(feature = "overlay")]
#[derive(Clone, Copy, Debug)]
pub struct OverlayApi<'a> {
  session: &'a MacosDriverSession,
}

impl MacosDriverSession {
  // Session APIs are grouped by automation target, not by native backend
  // mechanism. Window operations are relative to an application window;
  // input remains a lower-level escape hatch for raw pointer, keyboard, and
  // paste primitives that are not tied to one target domain.
  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { session: self }
  }

  pub fn clipboard(&self) -> ClipboardApi<'_> {
    ClipboardApi { session: self }
  }

  pub fn display(&self) -> DisplayApi<'_> {
    DisplayApi { session: self }
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

  #[cfg(feature = "overlay")]
  pub fn overlay(&self) -> OverlayApi<'_> {
    OverlayApi { session: self }
  }
}

#[cfg(feature = "overlay")]
impl OverlayApi<'_> {
  pub fn show(&self, overlay: &Overlay, options: ShowOptions) -> DriverResult<()> {
    let _ = self.session;
    auv_driver_overlay::show(overlay, options).map_err(|error| DriverError::Backend {
      message: error.to_string(),
    })
  }

  pub fn remove(&self) -> DriverResult<()> {
    let _ = self.session;
    auv_driver_overlay::remove().map_err(|error| DriverError::Backend {
      message: error.to_string(),
    })
  }
}

impl PermissionApi<'_> {
  pub fn probe(&self) -> DriverResult<PermissionProbe> {
    let _ = self.session;
    let native = crate::native::permission::probe_native_permissions().map_err(backend)?;
    Ok(PermissionProbe {
      screen_recording: permission_status_from_label(native.screen_recording),
      screen_capture_kit: permission_status_from_label(native.screen_capture_kit),
      accessibility: permission_status_from_label(native.accessibility),
      automation_to_system_events: probe_automation_to_system_events(),
    })
  }
}

impl AccessibilityApi<'_> {
  /// Captures a flattened AX tree for an application name or bundle id.
  pub fn capture_app_tree(&self, app: &str, max_depth: i64, max_children: i64) -> DriverResult<crate::types::ObservedAxTreeSnapshot> {
    let _ = self.session;
    crate::accessibility::capture_app_tree(app, max_depth, max_children)
  }

  /// Focuses a previously observed AX node path within a process.
  pub fn focus_node_path(&self, pid: i32, path: &str, expected_role: &str) -> DriverResult<InputActionResult> {
    let _ = self.session;
    crate::accessibility::focus_node_path(pid, path, expected_role)
  }

  /// Resolves a document/text node by query (or exact path candidate) and focuses it.
  pub fn focus_text(&self, options: auv_driver_common::FocusTextOptions) -> DriverResult<auv_driver_common::AxFocusResult> {
    let _ = self.session;
    crate::accessibility::focus_text(options)
  }

  /// Captures the current AX tree and verifies expected text on a role-matched node.
  pub fn verify_text(&self, app: &str, expected_text: &str, expected_role: &str) -> DriverResult<auv_driver_common::AxTextRead> {
    let _ = self.session;
    crate::accessibility::verify_text(app, expected_text, expected_role)
  }
}

impl DisplayApi<'_> {
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    let _ = self.session;
    Ok(ObservedDisplays {
      displays: list_display_targets()?.into_iter().map(|target| target.display).collect(),
    })
  }

  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    if options.window.is_some() || options.region.is_some() {
      return Err(invalid_input("display.capture does not accept window or region capture options"));
    }
    if let Activation::ActivateFirst { .. } = options.activation {
      return Err(invalid_input("display.capture cannot activate an application without an application target"));
    }
    let resolution = options.resolution;
    capture_display_xcap(options.display.as_deref()).map(|captured| DisplayCapture {
      capture: captured.capture.at_resolution(resolution),
      ..captured
    })
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
    capture_region_xcap(options.display.as_deref(), region).map(|captured| RegionCapture {
      capture: captured.capture.at_resolution(resolution),
      ..captured
    })
  }

  /// Capture a display without windows owned by this process, leaving those
  /// windows visible. Requires ScreenCaptureKit screenshots (macOS 14+).
  /// Applies the requested resolution after capturing native pixels.
  /// Never falls back to an unfiltered screenshot if exclusion fails.
  #[cfg(target_os = "macos")]
  pub fn capture_excluding_self(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
    if options.window.is_some() || options.region.is_some() || matches!(options.activation, Activation::ActivateFirst { .. }) {
      return Err(invalid_input("display.capture_excluding_self accepts only display selection and resolution"));
    }
    let target = resolve_display_target(&list_display_targets()?, options.display.as_deref())?;
    let id = target.display.id.parse::<u32>().map_err(|error| invalid_input(format!("invalid native display id: {error}")))?;
    let pixels = crate::native::capture::capture_display_rgba_without_self(id).map_err(backend)?;
    let width = u32::try_from(pixels.image_width).map_err(|error| backend(error.to_string()))?;
    let height = u32::try_from(pixels.image_height).map_err(|error| backend(error.to_string()))?;
    let image = RgbaImage::from_raw(width, height, pixels.rgba_bytes).ok_or_else(|| backend("invalid native display RGBA image"))?;
    let scale_factor = f64::from(width) / target.display.frame.size.width;
    let captured = display_capture(target.display, image, scale_factor, "macos.screencapturekit.excluding_self");
    Ok(DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}

impl WindowApi<'_> {
  /// Clicks a window-local target using its original window identity and the
  /// window's current frame. Never substitutes another main/frontmost window
  /// or re-recognizes the target. Delivery policy and evidence remain driver-owned.
  ///
  /// NOTICE: This consumer currently accepts window positions on macOS only.
  /// Screen/display targets and other platform consumers are deferred until a
  /// concrete caller needs their routing and input-policy contracts.
  pub fn click_target(&self, target: &(impl Positional + ?Sized), options: ClickOptions) -> DriverResult<InputActionResult> {
    let position = target.position()?;
    let CoordinateSpace::Window(id) = position.coordinate_space else {
      return Err(invalid_input("window.click_target requires a window-local position"));
    };
    if id.is_empty() || !position.point.x.is_finite() || !position.point.y.is_finite() {
      return Err(invalid_input("window.click_target requires a window id and finite coordinates"));
    }
    // A click needs only the current frame of an on-screen window, so the
    // cheaper on-screen listing without AX state is enough.
    let snapshot = crate::native::window::list_windows(ListWindowsOptions::all_visible(256)).map_err(backend)?;
    let window = snapshot
      .windows
      .iter()
      .find(|window| window.window_number.to_string() == id)
      .map(|window| window_from_observed(window, None))
      .ok_or_else(|| not_found(format!("window {id}")))?;
    self.click(&window, WindowPoint::from(position.point), options)
  }

  /// Every window on the current Space, including minimized windows, with
  /// AX-backed `state`.
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    let _ = self.session;
    let snapshot = crate::native::window::list_windows(ListWindowsOptions::all_windows(256)).map_err(backend)?;
    // Without displays `is_maximized` stays unknown rather than failing the listing.
    let displays = crate::native::window::enumerate_displays().map(|snapshot| snapshot.displays).unwrap_or_default();
    Ok(listed_windows(&snapshot, &displays, &self.maximized_windows()))
  }

  /// A fresh read of one window, or `NotFound` when it no longer exists.
  pub fn get(&self, window_id: &str) -> DriverResult<Window> {
    find_window(self.list()?, window_id)
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    let frontmost = selector.app.as_ref().is_some_and(|app| app.frontmost);
    if selector.main_visible && !frontmost {
      return resolve_main_window(&selector);
    }
    // Lookups by title or application stay on the on-screen listing without
    // AX reads: they never pick a minimized window.
    let mut snapshot = crate::native::window::list_windows(ListWindowsOptions::all_visible(256)).map_err(backend)?;
    if frontmost {
      let Some(window) = resolve_frontmost_window(&snapshot, &selector) else {
        return Err(not_found("frontmost window"));
      };
      return Ok(window);
    }

    if let Some(app) = &selector.app {
      if app.process_id.is_some() {
        return resolve_from_observed_windows(&snapshot, &selector);
      }
      let app_selector =
        app_selector_string(app).ok_or_else(|| invalid_input("window selector app must use bundle, name, pid, or frontmost"))?;
      let parsed_app_selector = parse_app_selector(&app_selector).map_err(invalid_input)?;
      let resolved_app = match resolve_app_ref(&snapshot, &parsed_app_selector) {
        Ok(resolved_app) => resolved_app,
        Err(_) => {
          // NOTICE: The unfiltered WindowServer snapshot can omit windows that
          // an app-filtered query returns. Retry with the explicit app selector
          // before reporting target_window_not_found.
          let filtered_snapshot = crate::native::window::list_windows(ListWindowsOptions::app(256, &app_selector)).map_err(backend)?;
          let resolved_app = resolve_app_ref(&filtered_snapshot, &parsed_app_selector)
            .map_err(|_| app_not_resolved(&app_selector, &parsed_app_selector, &snapshot))?;
          snapshot = filtered_snapshot;
          resolved_app
        }
      };
      let selection = window_selection_from_selector(&selector);
      let candidate = crate::support::resolve_window_candidate(&snapshot, &resolved_app, &[], &selection).ok();
      let Some(candidate) = candidate else {
        return resolve_from_observed_windows(&snapshot, &selector);
      };
      return Ok(window_from_native_ref(&candidate.window_ref, Some(candidate.is_main_candidate)));
    }

    resolve_from_observed_windows(&snapshot, &selector)
  }

  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    self.capture_with(window, CaptureOptions::default())
  }

  pub fn capture_with(&self, window: &Window, options: CaptureOptions) -> DriverResult<Capture> {
    if options.display.is_some() || options.region.is_some() || options.window.is_some() {
      return Err(invalid_input("window.capture_with does not accept display, region, or nested window capture options"));
    }
    if let Activation::ActivateFirst { settle } = options.activation {
      activate_app_for_window(window)?;
      thread::sleep(settle);
    }
    capture_window(window, options.resolution)
  }

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

  pub fn wait_text(&self, window: &Window, query: &str, region: RelativeRect, wait: WaitOptions) -> DriverResult<OcrMatches> {
    let matches = self.find_text(window, query, region, wait)?;
    if matches.matches.is_empty() {
      Err(not_found(format!("text {query:?} before timeout")))
    } else {
      Ok(matches)
    }
  }

  pub fn to_screen_point(&self, window: &Window, point: WindowPoint) -> DriverResult<ScreenPoint> {
    let _ = self.session;
    Ok(screen_point_for_window_point(window, point))
  }

  pub fn to_window_point(&self, window: &Window, point: ScreenPoint) -> DriverResult<WindowPoint> {
    let _ = self.session;
    Ok(window_point_for_screen_point(window, point))
  }

  /// Moves and/or resizes a `Normal` window through AX and confirms the result
  /// against the WindowServer frame. A window the application settled at
  /// another frame succeeds with `adjusted_by_app`; a window that did not
  /// change fails verification.
  pub fn set_frame(
    &self,
    window: &Window,
    origin: Option<Point>,
    size: Option<Size>,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    validate_frame_request(origin, size)?;
    let kind = WindowMutationKind::SetFrame { origin, size };
    let path = window_mutation_path(select_window_mutation_candidate(kind, &options)?);
    let tolerance = frame_tolerance(&options.verification)?;
    let (pid, number) = mutation_target(window)?;
    let before = self.get(&window.reference.id)?;
    require_normal_window(&before.state, "set_frame")?;

    let mut attempts = Vec::new();
    if unsettled_frame_components(before.frame, origin, size, tolerance).is_settled() {
      return Ok(window_mutation_result(path, attempts, &before, &before, DisturbanceLevel::None));
    }
    write_frame((pid, number), before.frame, origin, size, tolerance, path, &mut attempts)?;

    if !options.settle.is_zero() {
      thread::sleep(options.settle);
    }
    let after = self.get(&window.reference.id)?;
    let adjusted_by_app = frame_adjusted_by_app(before.frame, after.frame, origin, size, tolerance, &attempts)?;
    let mut result = window_mutation_result(path, attempts, &before, &after, DisturbanceLevel::None);
    result.adjusted_by_app = adjusted_by_app;
    Ok(result)
  }

  /// Moves a window to `Normal`, `Minimized` or `Maximized`. A window already
  /// in the requested state succeeds without a native call.
  ///
  /// `Maximized` sets the frame to the visible frame of the window's display
  /// (`NOTICE(macos-window-maximized)`) and records the frame it replaced.
  /// `Normal` on a maximized window restores that recorded frame, or leaves
  /// the frame as it is when none was recorded
  /// (`NOTICE(macos-maximize-restore-frame)`). A minimized window is
  /// unminimized first for both.
  pub fn set_state(&self, window: &Window, state: WindowStateTarget, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    let kind = WindowMutationKind::SetState { state };
    let path = window_mutation_path(select_window_mutation_candidate(kind, &options)?);
    let tolerance = frame_tolerance(&options.verification)?;
    let (pid, number) = mutation_target(window)?;
    let window_id = &window.reference.id;
    let before = self.get(window_id)?;
    let step = window_state_step(&before.state, state)?;

    let frontmost_before = frontmost_application_id();
    let mut attempts = Vec::new();
    let mut current = before.clone();
    if let Some(step) = step {
      let request = crate::native::window::DecodedWindowMutationRequest {
        pid,
        window_number: number,
        title: before.title.clone().unwrap_or_default(),
        kind: step,
      };
      match crate::native::window::mutate_window(request) {
        Ok(response) => attempts.push(WindowMutationAttempt::success(path, format!("{} via {}", response.performed_action, response.path))),
        Err(error) => {
          attempts.push(WindowMutationAttempt::failure(path, error));
          return Err(window_mutation_failure(attempts));
        }
      }
      // Unminimize animates; the frame of a window that is still animating
      // is not the frame to maximize from or to record.
      if !options.settle.is_zero() {
        thread::sleep(options.settle);
      }
      wait_for_stable_window_frame(number)?;
      current = self.get(window_id)?;
    }

    let frame_target = match state {
      WindowStateTarget::Minimized => None,
      WindowStateTarget::Maximized if current.state.is_maximized == Some(true) => None,
      WindowStateTarget::Maximized => {
        let displays = crate::native::window::enumerate_displays().map_err(backend)?.displays;
        Some(maximized_frame(current.frame, &displays).ok_or_else(|| DriverError::FailedPrecondition {
          message: format!("window:{window_id} at {:?} is on no display", current.frame),
          recovery: Some("move the window onto a display with set_frame first".to_string()),
        })?)
      }
      WindowStateTarget::Normal => {
        let recorded = self.maximized_windows().get(window_id).map(|frames| frames.normal);
        match normal_frame(&current.state, recorded) {
          NormalFrame::Keep => None,
          NormalFrame::Restore(frame) => Some(frame),
          NormalFrame::Unrecorded => {
            attempts.push(WindowMutationAttempt::success(
              path,
              "frame left unchanged: no frame was recorded by set_state(Maximized) (maximized outside AUV, or before a Runner restart)",
            ));
            None
          }
        }
      }
    };
    if let Some(target) = frame_target {
      write_frame((pid, number), current.frame, Some(target.origin), Some(target.size), tolerance, path, &mut attempts)?;
      if !options.settle.is_zero() {
        thread::sleep(options.settle);
      }
    }
    if attempts.is_empty() {
      return Ok(window_mutation_result(path, attempts, &before, &before, DisturbanceLevel::None));
    }

    let mut after = self.get(window_id)?;
    let focus_disturbance = focus_disturbance(frontmost_before, frontmost_application_id());
    verify_window_state(state, &after.state)?;
    let adjusted_by_app = match frame_target {
      Some(target) => frame_adjusted_by_app(current.frame, after.frame, Some(target.origin), Some(target.size), tolerance, &attempts)?,
      None => false,
    };
    match (state, frame_target) {
      (WindowStateTarget::Maximized, Some(_)) => {
        let frames = MaximizedFrames {
          normal: current.frame,
          maximized: after.frame,
        };
        self.maximized_windows().insert(window_id.clone(), frames);
        // `after` was read before the record existed; with it, the settled
        // frame reads as maximized, as every later read does.
        after.state.is_maximized = Some(true);
      }
      (WindowStateTarget::Normal, Some(_)) => {
        self.maximized_windows().remove(window_id);
      }
      _ => {}
    }
    let mut result = window_mutation_result(path, attempts, &before, &after, focus_disturbance);
    result.adjusted_by_app = adjusted_by_app;
    Ok(result)
  }

  fn maximized_windows(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, MaximizedFrames>> {
    // A panic while the lock was held cannot leave a half-written map entry.
    self.session.maximized_windows.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
  }

  /// Raises one window and makes its application frontmost, so the window
  /// has keyboard focus. The result reports the focus disturbance and the
  /// after-state `is_focused`.
  pub fn focus(&self, window: &Window, options: WindowMutationOptions) -> DriverResult<WindowMutationResult> {
    // NOTICE: focus is foreground by definition, so `options.policy` and
    // `options.strategy` do not select a path; settle and verification apply.
    let path = WindowMutationPath::AxWindowAction;
    let (pid, number) = mutation_target(window)?;
    let before = self.get(&window.reference.id)?;
    if before.state.is_focused == Some(true) {
      return Ok(window_mutation_result(path, Vec::new(), &before, &before, DisturbanceLevel::None));
    }
    if before.state.is_minimized == Some(true) {
      return Err(DriverError::FailedPrecondition {
        message: format!("window:{} is minimized; focus does not unminimize it", window.reference.id),
        recovery: Some("call set_state(Normal) first".to_string()),
      });
    }

    let frontmost_before = frontmost_application_id();
    let mut attempts = Vec::new();
    // Raise first so the target is the application's front window when the
    // application is activated.
    let raise = crate::native::window::DecodedWindowMutationRequest {
      pid,
      window_number: number,
      title: before.title.clone().unwrap_or_default(),
      kind: crate::native::window::DecodedWindowMutationKind::Raise,
    };
    match crate::native::window::mutate_window(raise) {
      Ok(response) => attempts.push(WindowMutationAttempt::success(path, format!("{} via {}", response.performed_action, response.path))),
      Err(error) => {
        attempts.push(WindowMutationAttempt::failure(path, error));
        return Err(window_mutation_failure(attempts));
      }
    }
    if let Err(error) = activate_app_for_window(&before) {
      attempts.push(WindowMutationAttempt::failure(WindowMutationPath::ForegroundSystemEvents, error.to_string()));
      return Err(window_mutation_failure(attempts));
    }
    attempts.push(WindowMutationAttempt::success(WindowMutationPath::ForegroundSystemEvents, "System Events set frontmost"));
    if !options.settle.is_zero() {
      thread::sleep(options.settle);
    }

    let after = self.get(&window.reference.id)?;
    let focus_disturbance = focus_disturbance(frontmost_before, frontmost_application_id());
    if options.verification != WindowMutationVerification::BestEffortState && after.state.is_focused == Some(false) {
      return Err(backend(format!("window mutation verification failed: window:{} did not receive focus", window.reference.id)));
    }
    Ok(window_mutation_result(path, attempts, &before, &after, focus_disturbance))
  }

  fn click_impl(&self, window: &Window, point: WindowPoint, options: ClickOptions) -> DriverResult<InputActionResult> {
    let screen_point = self.to_screen_point(window, point)?;
    if click_attempt_candidates(options.policy) == [InputDeliveryPath::ForegroundSystemEvents] {
      // NOTICE(qemu-foreground-click): A pid-targeted CGEvent can be accepted
      // by macOS while a focus-sensitive host window (notably QEMU) discards
      // it before forwarding input to its guest. ForegroundPreferred must use
      // the global HID route first; remove this note only if the native route
      // gains consumption evidence rather than dispatch-only success.
      let lease = self.prepare_for_input(window, foreground_prepare_options(Duration::from_millis(50)))?;
      let action_result = self.session.input().click_at(screen_point.point(), options.button, options.click, options.modifiers);
      let restore_result = self.restore_input(lease);
      action_result?;
      restore_result?;
      return Ok(InputActionResult {
        selected_path: InputDeliveryPath::ForegroundSystemEvents,
        attempts: vec![InputAttempt::success(
          InputDeliveryPath::ForegroundSystemEvents,
        )],
        verified: false,
        mouse_disturbance: DisturbanceLevel::Temporary,
        focus_disturbance: DisturbanceLevel::Foreground,
        clipboard_disturbance: DisturbanceLevel::None,
      });
    }

    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let screen = screen_point.point();
    let window_point = point.point();
    let (click_count, click_interval_ms) = click_parts(&options.click)?;
    let window_strategy_code = match options.window_strategy {
      WindowClickStrategy::ChromiumCompatible => 0,
      WindowClickStrategy::PidTargeted => 1,
    };
    crate::native::input::click_window_point(
      pid,
      number,
      screen.x,
      screen.y,
      window_point.x,
      window_point.y,
      match options.button {
        auv_driver_common::MouseButton::Left => 0,
        auv_driver_common::MouseButton::Right => 1,
        auv_driver_common::MouseButton::Middle => 2,
      },
      click_count,
      click_interval_ms,
      window_strategy_code,
      options.modifiers,
    )
    .map_err(backend)?;
    Ok(InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse))
  }

  pub fn type_text(&self, window: &Window, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    self
      .session
      .input()
      .input_keyboard(
        &auv_driver_common::InputTarget::Window(window.clone()),
        vec![auv_driver_common::KeyboardInput::TypeText {
          text: text.into(),
          options,
        }],
        false,
      )
      .map_err(|error| error.cause)
      .map(|actions| actions.expect("non-dry input actions").remove(0))
  }

  fn scroll_impl(&self, window: &Window, point: WindowPoint, scroll: Scroll, options: ScrollOptions) -> DriverResult<InputActionResult> {
    let mut attempts = Vec::new();
    for candidate in scroll_attempt_candidates(&options) {
      match candidate {
        ScrollDeliveryCandidate::AxScroll => {
          // TODO(background-scroll-ax): AX scrollbar/value scrolling is deferred
          // until this policy slice has verification evidence for the
          // window-targeted wheel path; implement when owner approves AX scroll
          // mutation against captured AX tree state.
          let message = "AX scroll is not implemented in this slice";
          attempts.push(InputAttempt::failure(InputDeliveryPath::AxScroll, message));
        }
        ScrollDeliveryCandidate::WindowTargetedWheel => match self.scroll_window_targeted_wheel(window, point, scroll, options.settle) {
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
          Err(error) => {
            let message = error.to_string();
            attempts.push(InputAttempt::failure(InputDeliveryPath::WindowTargetedWheel, message));
          }
        },
        ScrollDeliveryCandidate::WindowTargetedKeyboardScroll => {
          // TODO(background-scroll-keyboard): Keyboard scroll needs target state
          // and reliability rules before it can be enabled; add only after
          // owner-approved verification for focus/element anchoring.
          let message = "window-targeted keyboard scroll is reserved but disabled";
          attempts.push(InputAttempt::failure(InputDeliveryPath::WindowTargetedKeyboardScroll, message));
        }
        ScrollDeliveryCandidate::ForegroundHid => {
          if options.policy == InputPolicy::BackgroundOnly {
            continue;
          }
          let screen_point = self.to_screen_point(window, point)?;
          let result = self.session.input().scroll_global_hid(screen_point.point(), scroll, options.settle)?;
          attempts.extend(result.attempts);
          return Ok(InputActionResult {
            selected_path: result.selected_path,
            attempts,
            verified: result.verified,
            mouse_disturbance: result.mouse_disturbance,
            focus_disturbance: result.focus_disturbance,
            clipboard_disturbance: result.clipboard_disturbance,
          });
        }
      }
    }

    Err(DriverError::unsupported("background_scroll"))
  }

  /// Prepare the exact observed window through the shared input lifecycle.
  pub fn prepare_for_input(&self, window: &Window, options: PrepareForInputOptions) -> DriverResult<InputPreparationLease> {
    self.session.input().prepare_for_input(&auv_driver_common::InputTarget::Window(window.clone()), options)
  }

  pub fn restore_input(&self, mut lease: InputPreparationLease) -> DriverResult<()> {
    let _ = self.session;
    lease.mark_restored();
    Ok(())
  }

  fn scroll_window_targeted_wheel(&self, window: &Window, point: WindowPoint, scroll: Scroll, settle: Duration) -> DriverResult<()> {
    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let screen_point = self.to_screen_point(window, point)?;
    let screen = screen_point.point();
    let window_point = point.point();
    let (wheel_x, wheel_y) = core_graphics_wheel_pixels(scroll);
    crate::native::input::scroll_window_point(pid, number, screen.x, screen.y, window_point.x, window_point.y, wheel_x, wheel_y)
      .map_err(backend)?;
    if !settle.is_zero() {
      thread::sleep(settle);
    }
    Ok(())
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

  // The mouse coordinator owns the desktop admission for the whole gesture, so
  // this method does not reserve it again around the drag itself.
  fn drag(
    &self,
    window: &Window,
    mut movement: auv_driver_common::MoveMouseRequest,
    button: auv_driver_common::MouseButton,
    policy: InputPolicy,
  ) -> DriverResult<(Point, InputActionResult)> {
    if policy != InputPolicy::ForegroundPreferred {
      movement.target = Some(InputTarget::Window(window.clone()));
      return self.session.input().drag_mouse(movement, button);
    }
    // Same preparation as a foreground window click: a PID-targeted gesture can
    // be ignored by an inactive application, so foreground uses the HID route.
    let lease = self.prepare_for_input(window, foreground_prepare_options(Duration::from_millis(50)))?;
    movement.target = Some(InputTarget::Foreground);
    let drag_result = self.session.input().drag_mouse(movement, button);
    let restore_result = self.restore_input(lease);
    let (point, mut action) = drag_result?;
    restore_result?;
    action.focus_disturbance = DisturbanceLevel::Foreground;
    Ok((point, action))
  }
}

impl InputApi<'_> {
  /// Begin a bounded hold of one validated key combination. The same native
  /// recipient and key codes are retained for timeout and explicit release.
  pub fn key_down(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    timeout: Duration,
  ) -> DriverResult<auv_driver_common::KeyboardHold> {
    let codes = parse_key_combination(&PressKeysOptions {
      keys,
      ..Default::default()
    })?;
    if matches!(target, InputTarget::Foreground) && policy != InputPolicy::ForegroundPreferred {
      return Err(invalid_input("background keyboard input requires an application or window target"));
    }
    let recipient = match target {
      // Foreground posts through the session event tap; it needs no PID/window pair.
      InputTarget::Foreground => None,
      _ => Some(resolve_input_target(target)?),
    };
    if let Some(recipient) = recipient {
      if policy == InputPolicy::ForegroundPreferred {
        prepare_input_recipient(recipient, foreground_prepare_options(Duration::ZERO))?;
      } else {
        crate::native::window::validate_input_target(recipient.0, recipient.1, false).map_err(backend)?;
      }
    }
    let controller = auv_driver_common::keyboard_hold_controller().clone();
    let backend = std::sync::Arc::new(HeldKeyboardBackend { recipient, codes });
    controller.down(backend, timeout)
  }

  pub fn key_up(&self, hold: auv_driver_common::KeyboardHoldId) -> DriverResult<InputActionResult> {
    auv_driver_common::keyboard_hold_controller().up(hold)
  }

  /// Hold and release under one call, including timeout cleanup.
  pub fn hold_keys(
    &self,
    target: &InputTarget,
    keys: Vec<String>,
    policy: InputPolicy,
    duration: Duration,
  ) -> DriverResult<InputActionResult> {
    // Leave a cleanup margin so the bounded call normally owns its release;
    // the controller deadline remains a fallback if this call stalls.
    let mut hold = self.key_down(target, keys, policy, duration.saturating_add(Duration::from_secs(1)))?;
    hold.wait_and_release(duration)
  }

  /// A complete drag composes press, sampled movement, and release under one admission.
  pub fn drag_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    button: auv_driver_common::MouseButton,
  ) -> DriverResult<(Point, InputActionResult)> {
    let backend = self.pointer_backend(request.target.as_ref())?;
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, Some(button), backend, |_| true)
  }

  fn pointer_backend(
    &self,
    target: Option<&auv_driver_common::InputTarget>,
  ) -> DriverResult<std::sync::Arc<dyn auv_driver_common::mouse_input::MouseBackend>> {
    match target {
      None | Some(auv_driver_common::InputTarget::Foreground) => Ok(std::sync::Arc::new(PointerBackend)),
      Some(auv_driver_common::InputTarget::Window(window)) => Ok(std::sync::Arc::new(WindowPointerBackend {
        session: self.session.clone(),
        window: window.clone(),
      })),
      Some(auv_driver_common::InputTarget::Application { .. }) => {
        Err(auv_driver_common::DriverError::unsupported("mouse input requires a window or foreground target"))
      }
    }
  }

  pub fn move_mouse(
    &self,
    request: auv_driver_common::MoveMouseRequest,
    notify: impl FnMut(auv_driver_common::mouse_input::MotionEvent) -> bool,
  ) -> DriverResult<(Point, InputActionResult)> {
    let backend = self.pointer_backend(request.target.as_ref())?;
    auv_driver_common::mouse_input::mouse_coordinator().motion(request, None, backend, notify)
  }

  /// Local convenience: press, wait, and release under one admission.
  /// This composes the mouse lifecycle; it is not a separate Runner capability.
  pub fn hold_mouse(
    &self,
    target: &auv_driver_common::InputTarget,
    mouse: u64,
    point: Point,
    button: auv_driver_common::MouseButton,
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
    target: &auv_driver_common::InputTarget,
    mouse: u64,
    point: Point,
    button: auv_driver_common::MouseButton,
    timeout: std::time::Duration,
  ) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().down(mouse, point, button, timeout, self.pointer_backend(Some(target))?)
  }

  pub fn mouse_up(&self, mouse: u64) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().up(mouse)
  }

  pub fn move_mouse_to(&self, mouse: u64, point: Point) -> DriverResult<InputActionResult> {
    auv_driver_common::mouse_input::mouse_coordinator().move_to(mouse, point, std::sync::Arc::new(PointerBackend))
  }

  /// Prepare application or exact-window focus. This selects no text control
  /// and installs no focus lease; subsequent delivery must remain PID-bound.
  pub fn prepare_for_input(
    &self,
    target: &auv_driver_common::InputTarget,
    options: PrepareForInputOptions,
  ) -> DriverResult<InputPreparationLease> {
    let recipient = resolve_input_target(target)?;
    prepare_input_recipient(recipient, options)
  }

  /// Execute ordered keyboard actions. Validate the whole request before IO,
  /// bind the recipient once, and recheck it before each action/repetition.
  /// Returns delivery evidence only; errors retain partial progress.
  pub fn input_keyboard(
    &self,
    target: &InputTarget,
    inputs: Vec<KeyboardInput>,
    dry_run: bool,
  ) -> Result<Option<Vec<InputActionResult>>, KeyboardInputError> {
    let fail = |cause, action_index, completed, completed_presses| KeyboardInputError {
      cause,
      progress: auv_driver_common::KeyboardInputProgress {
        action_index,
        completed,
        completed_presses,
      },
    };
    if inputs.is_empty() {
      return Err(fail(invalid_input("keyboard input requires at least one action"), 0, vec![], 0));
    }
    // Compile every key combination before even resolving the recipient. A malformed tail
    // cannot activate an app or deliver the prefix of a request.
    let mut combinations = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
      let validation = match input {
        KeyboardInput::PressKeys { options, .. } => parse_key_combination(options).map(Some),
        KeyboardInput::TypeText { options, .. } => type_text_parts(*options).map(|_| None),
        KeyboardInput::PasteText { options, .. } => text_submit_key_code(options.submit).map(|_| None),
      };
      combinations.push(validation.map_err(|cause| fail(cause, index, vec![], 0))?);
      if matches!(target, InputTarget::Foreground) && input.policy() != InputPolicy::ForegroundPreferred {
        return Err(fail(invalid_input("background keyboard input requires an application or window target"), index, vec![], 0));
      }
    }
    let recipient = match target {
      InputTarget::Foreground => None,
      _ => Some(resolve_input_target(target).map_err(|cause| fail(cause, 0, vec![], 0))?),
    };
    let mut completed = Vec::new();
    for (index, (input, combination)) in inputs.into_iter().zip(combinations).enumerate() {
      let foreground = input.policy() == InputPolicy::ForegroundPreferred;
      let (count, interval, settle) = match &input {
        KeyboardInput::PressKeys { options, .. } => (options.count, options.interval, options.settle),
        _ => (1, Duration::ZERO, Duration::ZERO),
      };
      let mut combined: Option<InputActionResult> = None;
      for repetition in 0..if dry_run { 1 } else { count } {
        if repetition > 0 {
          thread::sleep(interval);
        }
        let outcome = (|| {
          if let Some(recipient) = recipient {
            if dry_run || !foreground {
              crate::native::window::validate_input_target(recipient.0, recipient.1, foreground).map_err(backend)?;
            } else {
              prepare_input_recipient(recipient, foreground_prepare_options(Duration::ZERO))?;
            }
          }
          if dry_run {
            return Ok(None);
          }
          // TODO(background-keyboard-fallback): preserve the no-steal rollout.
          // BackgroundPreferred cannot retry after a potentially partial delivery.
          let mut action = match &input {
            KeyboardInput::PressKeys { .. } => {
              crate::native::input::press_keys(recipient, combination.as_ref().expect("validated combination").clone()).map_err(backend)?;
              match recipient {
                Some(_) => InputActionResult::single_success(InputDeliveryPath::WindowTargetedKeyboard),
                None => foreground_system_events_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None),
              }
            }
            KeyboardInput::TypeText { text, options } => match recipient {
              Some((pid, number)) => match type_text_in_window(pid, number, text, *options) {
                Ok(()) => InputActionResult::single_success(InputDeliveryPath::WindowTargetedKeyboard),
                // Preserve opt-in clipboard fallback after text delivery failure,
                // never after recipient preparation failure.
                Err(error) if foreground && options.allow_clipboard_fallback => {
                  let mut action = paste_text_impl(
                    PasteTextOptions {
                      text: text.clone(),
                      replace_existing: options.replace_existing,
                      submit: options.submit,
                      settle: options.settle,
                    },
                    recipient,
                  )?;
                  action.attempts.insert(0, InputAttempt::failure(InputDeliveryPath::WindowTargetedKeyboard, error.to_string()));
                  action
                }
                Err(error) => return Err(error),
              },
              None => {
                type_text_foreground(text, *options)?;
                foreground_system_events_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None)
              }
            },
            KeyboardInput::PasteText { options, .. } => paste_text_impl(options.clone(), recipient)?,
          };
          if let Some((pid, number)) = recipient {
            action.focus_disturbance = if foreground {
              DisturbanceLevel::Foreground
            } else {
              DisturbanceLevel::None
            };
            let scope = if number == 0 {
              "application scope".to_string()
            } else {
              format!("window={number}")
            };
            action.attempts.last_mut().expect("successful delivery").message =
              Some(format!("events posted to pid={pid}, {scope}; control effect not verified"));
          }
          Ok(Some(action))
        })();
        match outcome {
          Ok(Some(action)) => match &mut combined {
            Some(combined) => combined.attempts.extend(action.attempts),
            None => combined = Some(action),
          },
          Ok(None) => {}
          Err(cause) => return Err(fail(cause, index, completed, repetition)),
        }
      }
      if !dry_run && !settle.is_zero() {
        thread::sleep(settle);
      }
      if let Some(action) = combined {
        completed.push(action);
      }
    }
    Ok((!dry_run).then_some(completed))
  }

  /// Deliver one key combination using the same interpreter as a multi-action request.
  pub fn press_keys(
    &self,
    target: &InputTarget,
    options: PressKeysOptions,
    policy: InputPolicy,
    dry_run: bool,
  ) -> Result<Option<InputActionResult>, KeyboardInputError> {
    self
      .input_keyboard(target, vec![KeyboardInput::PressKeys { options, policy }], dry_run)
      .map(|actions| actions.map(|mut actions| actions.remove(0)))
  }

  pub fn current_position(&self) -> DriverResult<Point> {
    let _ = self.session;
    let (x, y) = crate::native::pointer::current_mouse_logical_point().map_err(backend)?;
    Ok(Point::new(x, y))
  }

  pub fn move_to(&self, point: Point) -> DriverResult<InputActionResult> {
    self.move_mouse_to(0, point)
  }

  pub fn click_at(
    &self,
    point: Point,
    button: auv_driver_common::MouseButton,
    click: Click,
    modifiers: auv_driver_common::ClickModifiers,
  ) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    let _ = self.session;
    let (count, interval) = click_parts(&click)?;
    crate::native::pointer::click_point(
      point.x,
      point.y,
      match button {
        auv_driver_common::MouseButton::Left => 0,
        auv_driver_common::MouseButton::Right => 1,
        auv_driver_common::MouseButton::Middle => 2,
      },
      count,
      interval,
      modifiers,
    )
    .map_err(backend)?;
    Ok(foreground_system_events_result(DisturbanceLevel::Temporary, DisturbanceLevel::Unknown, DisturbanceLevel::None))
  }

  pub fn scroll_global_hid(&self, point: Point, scroll: Scroll, settle: Duration) -> DriverResult<InputActionResult> {
    let _desktop = auv_driver_common::mouse_input::reserve_desktop_input()?;
    let _ = self.session;
    let (wheel_x, wheel_y) = core_graphics_wheel_pixels(scroll);
    crate::native::pointer::scroll_point(point.x, point.y, wheel_x, wheel_y).map_err(backend)?;
    if !settle.is_zero() {
      thread::sleep(settle);
    }
    Ok(InputActionResult {
      selected_path: InputDeliveryPath::ForegroundSystemEvents,
      attempts: vec![InputAttempt::success(
        InputDeliveryPath::ForegroundSystemEvents,
      )],
      verified: false,
      mouse_disturbance: DisturbanceLevel::Temporary,
      focus_disturbance: DisturbanceLevel::Unknown,
      clipboard_disturbance: DisturbanceLevel::None,
    })
  }

  pub fn copy(&self) -> DriverResult<()> {
    let _ = self.session;
    crate::native::input::hotkey_foreground(8, true, false, false, false).map_err(backend)
  }

  pub fn paste(&self) -> DriverResult<()> {
    let _ = self.session;
    crate::native::input::hotkey_foreground(9, true, false, false, false).map_err(backend)
  }

  pub fn type_text(&self, text: &str, options: TypeTextOptions) -> DriverResult<InputActionResult> {
    let _ = self.session;
    // TODO(foreground-input-target-lease): target-aware foreground typing is
    // deferred until an owner-approved slice connects active-control input to
    // window preparation/restoration leases; this API intentionally types into
    // the current foreground control.
    type_text_foreground(text, options)?;
    Ok(foreground_system_events_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None))
  }

  /// Legacy single-key API. Shortcut strings remain accepted for released callers;
  /// new callers use `press_keys` with explicit keys. Both use `input_keyboard`.
  pub fn press_key(&self, options: KeyPressOptions) -> DriverResult<InputActionResult> {
    let combination = options.into();
    self
      .press_keys(&InputTarget::Foreground, combination, InputPolicy::ForegroundPreferred, false)
      .map_err(|error| error.cause)?
      .ok_or_else(|| backend("keyboard delivery omitted its result"))
  }

  pub fn paste_text(&self, options: PasteTextOptions) -> DriverResult<InputActionResult> {
    let _ = self.session;
    paste_text_impl(options, None)
  }
}

impl ClipboardApi<'_> {
  pub fn snapshot(&self) -> DriverResult<String> {
    let _ = self.session;
    crate::native::clipboard::capture_clipboard_snapshot().map_err(backend)
  }

  pub fn restore(&self, snapshot: &str) -> DriverResult<()> {
    let _ = self.session;
    crate::native::clipboard::restore_clipboard_snapshot(snapshot).map_err(backend)
  }

  pub fn set_text(&self, text: &str) -> DriverResult<()> {
    let _ = self.session;
    crate::native::clipboard::set_clipboard_text(text).map_err(backend)
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
    let _ = self.session;
    let crop = relative_rect_to_observed(capture, region);
    let native = crate::native::ocr::find_text_in_rgba(
      capture.image.clone().into_raw(),
      i64::from(capture.image.width()),
      i64::from(capture.image.height()),
      "",
      false,
      false,
      256,
      &options.custom_words,
      options.recognition_languages.as_deref(),
      Some(&crop),
    )
    .map_err(backend)?;
    Ok(text_recognition_from_native(&native, capture))
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
    let recognition = self.recognize_text_in_capture_with_options(capture, region, options)?;
    Ok(ocr_matches_from_recognition(&recognition, query))
  }
}

/// Resolves a `main_visible` selector with the shared main-window cascade.
///
/// The cascade prefers a shown window, so a minimized one can win only when
/// the application shows no normal window. The on-screen listing without AX
/// reads settles every other case; only then does this pay for a listing with
/// AX `state` that includes minimized windows, scoped to the application so
/// AX is asked of that application alone.
///
/// NOTICE(macos-main-window-focus): the on-screen listing carries no focus,
/// so cascade step 4 (focused) does not narrow there and step 7 takes the
/// frontmost normal window. On macOS the key window is the frontmost window
/// of its level and `is_normal_window` keeps only the normal level, so the two
/// agree; a sheet, or an app that keeps an unfocused window in front, differs.
// NOTICE(macos-main-window-latency): measured on this Mac (debug build,
// Ghostty with 6 windows): the on-screen path costs about 2 ms warm, the same
// as before the cascade; the AX-backed app-scoped listing about 6-8 ms warm
// and 40-50 ms in a fresh process, against 15-20 ms warm and 150-200 ms fresh
// for the all-applications `window.list`.
fn resolve_main_window(selector: &WindowSelector) -> DriverResult<Window> {
  let list = |options| crate::native::window::list_windows(options).map_err(backend);
  let app = selector.app.as_ref();
  let pid = app.and_then(|app| app.process_id);
  let app_selector = match app {
    Some(app) if pid.is_none() => {
      Some(app_selector_string(app).ok_or_else(|| invalid_input("window selector app must use bundle, name, pid, or frontmost"))?)
    }
    _ => None,
  };
  let parsed_app_selector = app_selector.as_deref().map(parse_app_selector).transpose().map_err(invalid_input)?;
  let choose = |snapshot: &ObservedWindowSnapshot| match &parsed_app_selector {
    // A bundle id or name resolves with the same heuristics as the other
    // application lookups (case-insensitive bundle id, frontmost fallback).
    Some(parsed) => {
      let resolved_app = resolve_app_ref(snapshot, parsed).ok()?;
      let owned =
        filter_windows_for_app(&snapshot.windows, &resolved_app).into_iter().map(|window| window.window_number).collect::<HashSet<_>>();
      main_window_from_listing(snapshot, selector, |window| owned.contains(&window.window_number))
    }
    None => main_window_from_listing(snapshot, selector, |window| owned_by_selector(window, selector)),
  };

  let on_screen = match &app_selector {
    Some(app_selector) => ListWindowsOptions::app(256, app_selector),
    None => ListWindowsOptions::all_visible(256),
  };
  if let Some(window) = choose(&list(on_screen)?).filter(is_normal_window) {
    return Ok(window);
  }

  let with_state = match (&app_selector, pid) {
    (Some(app_selector), _) => ListWindowsOptions::app_windows(256, app_selector),
    // The listing filters by bundle id or name, so a pid is scoped through its
    // bundle id; a process without one pays the full listing.
    (None, Some(pid)) => match crate::native::window::bundle_ids_by_pid(&[pid].into()).ok().and_then(|ids| ids.get(&pid).cloned()) {
      Some(bundle_id) if !bundle_id.is_empty() => ListWindowsOptions::app_windows(256, bundle_id),
      _ => ListWindowsOptions::all_windows(256),
    },
    (None, None) => ListWindowsOptions::all_windows(256),
  };
  let snapshot = list(with_state)?;
  if let Some(window) = choose(&snapshot) {
    return Ok(window);
  }
  if let (Some(app_selector), Some(parsed)) = (&app_selector, &parsed_app_selector)
    && resolve_app_ref(&snapshot, parsed).is_err()
  {
    // The scoped listing has no other applications to name in the hint.
    return Err(app_not_resolved(app_selector, parsed, &list(ListWindowsOptions::all_visible(256))?));
  }
  Err(not_found("main visible window"))
}

/// The cascade's choice among the listed windows that `owned_by` accepts and
/// the selector's title matches.
fn main_window_from_listing(
  snapshot: &ObservedWindowSnapshot,
  selector: &WindowSelector,
  owned_by: impl Fn(&ObservedWindow) -> bool,
) -> Option<Window> {
  let candidates = snapshot
    .windows
    .iter()
    .zip(listed_windows(snapshot, &[], &std::collections::HashMap::new()))
    .filter(|(observed, window)| owned_by(observed) && matches_title(window, selector))
    .map(|(_, window)| window)
    .collect::<Vec<_>>();
  select_main_window(candidates.iter().filter(|window| is_normal_window(window))).or_else(|| select_main_window(&candidates)).cloned()
}

/// NOTICE(macos-main-window-prefilter): menu bar extras, panels and other
/// surfaces above the normal window layer, and frames smaller than 160x120
/// (the substantial-window rule of `build_window_candidates`), are never an
/// application's main window. They are candidates only when the application
/// has nothing else, as before the cascade.
fn is_normal_window(window: &Window) -> bool {
  window.state.is_always_on_top != Some(true) && window.frame.size.width >= 160.0 && window.frame.size.height >= 120.0
}

fn owned_by_selector(window: &ObservedWindow, selector: &WindowSelector) -> bool {
  selector.app.as_ref().is_none_or(|app| matches_app_selector(&window_from_observed(window, None), app))
}

/// Why an application selector resolved no window. The snapshot cannot
/// separate "not running" from "running without a visible window": both leave
/// nothing to resolve. Ask the process list, which answers without needing a
/// WindowServer window, so the caller learns whether to launch the application
/// or bring back one of its windows.
fn app_not_resolved(app_selector: &str, parsed: &crate::types::AppSelector, snapshot: &ObservedWindowSnapshot) -> DriverError {
  let liveness = parsed
    .bundle_id
    .as_deref()
    .map(|bundle_id| AppLiveness::from_probe(crate::native::window::running_application_pid(bundle_id)))
    .unwrap_or(AppLiveness::Unknown);
  not_found(app_resolution_message(app_selector, liveness, snapshot))
}

fn resolve_from_observed_windows(snapshot: &ObservedWindowSnapshot, selector: &WindowSelector) -> DriverResult<Window> {
  if selector.main_visible {
    return main_window_from_listing(snapshot, selector, |window| owned_by_selector(window, selector))
      .ok_or_else(|| not_found("main visible window"));
  }
  let candidates = snapshot
    .windows
    .iter()
    .map(|window| window_from_observed(window, None))
    .filter(|window| matches_window_selector_except_main_visible(window, selector))
    .collect::<Vec<_>>();
  match candidates.as_slice() {
    [window] => Ok(window.clone()),
    [] => Err(not_found("window selector")),
    _ => Err(invalid_input(format!("window selector was ambiguous: {} windows matched", candidates.len()))),
  }
}

fn matches_window_selector(window: &Window, selector: &WindowSelector) -> bool {
  if !matches_window_selector_except_main_visible(window, selector) {
    return false;
  }
  if selector.main_visible && (!window.is_visible || !window.is_main) {
    return false;
  }
  true
}

fn matches_window_selector_except_main_visible(window: &Window, selector: &WindowSelector) -> bool {
  window.is_visible && selector.app.as_ref().is_none_or(|app| matches_app_selector(window, app)) && matches_title(window, selector)
}

fn matches_title(window: &Window, selector: &WindowSelector) -> bool {
  selector.title.as_ref().is_none_or(|title| window.title.as_deref().is_some_and(|window_title| matches_text(window_title, title)))
}

fn matches_app_selector(window: &Window, selector: &AppSelector) -> bool {
  if selector.frontmost {
    return window.is_main;
  }
  if let Some(pid) = selector.process_id
    && window.process_id != Some(pid)
  {
    return false;
  }
  if let Some(matcher) = &selector.id {
    let Some(application_id) = &window.application_id else {
      return false;
    };
    if !matches_text(application_id, matcher) {
      return false;
    }
  }
  if let Some(name) = &selector.name {
    let Some(app_name) = &window.app_name else {
      return false;
    };
    if !matches_text(app_name, name) {
      return false;
    }
  }
  true
}

fn matches_text(value: &str, matcher: &TextMatcher) -> bool {
  match matcher {
    TextMatcher::Exact(expected) => value == expected,
    TextMatcher::Contains(needle) => value.contains(needle),
  }
}

fn window_from_observed(window: &ObservedWindow, is_main: Option<bool>) -> Window {
  // WindowServer keeps a window on screen during the minimize animation, so
  // AX minimized wins over the on-screen flag.
  let is_visible = !window.state.off_screen && window.state.minimized != Some(true) && window.bounds.width > 0 && window.bounds.height > 0;
  Window {
    reference: WindowRef {
      id: window.window_number.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    application_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: rect_from_observed(&window.bounds),
    coordinate_space: CoordinateSpace::Screen,
    is_main: is_main.unwrap_or(false),
    is_visible,
    state: WindowState {
      // An on-screen window is never minimized, so this holds without AX.
      is_minimized: window.state.minimized.or((!window.state.off_screen).then_some(false)),
      // Needs the display list; `listed_windows` fills it.
      is_maximized: None,
      is_fullscreen: window.state.fullscreen,
      is_visible: Some(is_visible),
      is_focused: window.state.focused,
      is_always_on_top: Some(window.layer > 0),
      // Valid only inside one listing; `listed_windows` fills it.
      stack_index: None,
      // TODO(macos-window-space): reading a window's Space needs the private
      // CGS API; revisit only with an owner-approved need. Stage Manager
      // grouping is not modeled either (TODO(macos-stage-manager)).
      desktop: None,
    },
  }
}

/// Windows of an all-windows listing with the facts that need the whole
/// listing or the displays: front-to-back `stack_index` among visible
/// windows, and the best-effort `is_maximized`.
fn listed_windows(
  snapshot: &ObservedWindowSnapshot,
  displays: &[ObservedDisplay],
  maximized_windows: &std::collections::HashMap<String, MaximizedFrames>,
) -> Vec<Window> {
  let mut next_stack_index = 0;
  snapshot
    .windows
    .iter()
    .map(|observed| {
      let mut window = window_from_observed(observed, None);
      if window.is_visible {
        window.state.stack_index = Some(next_stack_index);
        next_stack_index += 1;
      }
      if !displays.is_empty() {
        let settled_by_auv = maximized_windows
          .get(&window.reference.id)
          .is_some_and(|frames| frames_close(window.frame, frames.maximized, MAXIMIZED_FRAME_TOLERANCE));
        window.state.is_maximized =
          Some(window.state.is_fullscreen != Some(true) && (settled_by_auv || fills_maximized_frame(window.frame, displays)));
      }
      window
    })
    .collect()
}

/// NOTICE(macos-window-maximized): macOS has no maximized state. The green
/// button zooms to an app-defined standard frame that need not fill the
/// screen (TextEdit on a display without a menu bar left a 30 pt gap), so AUV
/// does not use it. `set_state(Maximized)` sets the frame to the visible frame
/// (`NSScreen.visibleFrame`: the display minus menu bar and Dock) of the
/// window's display, as Rectangle and Magnet do, and a window counts as
/// maximized when its frame matches that frame within this tolerance, or
/// matches the frame an AUV maximize settled at when the application
/// constrained it (TextEdit keeps a 30 pt gap at the top of a display without
/// a menu bar). A window the user zoomed with the green button can therefore
/// read `false`, and a window the user sized to the visible frame reads `true`.
const MAXIMIZED_FRAME_TOLERANCE: f64 = 2.0;

/// The frame `set_state(Maximized)` gives a window at `frame`, or `None` when
/// the window is on no display.
fn maximized_frame(frame: Rect, displays: &[ObservedDisplay]) -> Option<Rect> {
  display_for_frame(frame, displays).map(|display| rect_from_observed(&display.visible_bounds))
}

fn fills_maximized_frame(frame: Rect, displays: &[ObservedDisplay]) -> bool {
  maximized_frame(frame, displays).is_some_and(|area| frames_close(frame, area, MAXIMIZED_FRAME_TOLERANCE))
}

/// The display a window is on: the one that contains the window's center, else
/// the one with the largest overlap (a window whose center is off every
/// display). A window that spans displays belongs to the display under its
/// center. `NSWindow.screen` uses the largest overlap instead; the two can
/// differ for a window split across displays of different sizes.
fn display_for_frame(frame: Rect, displays: &[ObservedDisplay]) -> Option<&ObservedDisplay> {
  let center = frame.center();
  displays.iter().find(|display| rect_from_observed(&display.bounds).contains_point(center)).or_else(|| {
    displays
      .iter()
      .filter_map(|display| {
        let overlap = frame.intersect(rect_from_observed(&display.bounds))?;
        Some((display, overlap.size.width * overlap.size.height))
      })
      .max_by(|left, right| left.1.total_cmp(&right.1))
      .map(|(display, _)| display)
  })
}

fn window_from_native_ref(window: &NativeWindowRef, is_main: Option<bool>) -> Window {
  Window {
    reference: WindowRef {
      id: window.window_number.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    application_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: rect_from_observed(&window.bounds),
    coordinate_space: CoordinateSpace::Screen,
    is_main: is_main.unwrap_or(true),
    is_visible: window.bounds.width > 0 && window.bounds.height > 0,
    // Selector resolution uses the on-screen listing without AX reads, so
    // only the facts that listing proves are set.
    state: WindowState {
      is_minimized: Some(false),
      is_visible: Some(window.bounds.width > 0 && window.bounds.height > 0),
      is_always_on_top: Some(window.layer > 0),
      ..WindowState::default()
    },
  }
}

fn resolve_frontmost_window(snapshot: &ObservedWindowSnapshot, selector: &WindowSelector) -> Option<Window> {
  snapshot
    .windows
    .iter()
    .filter(|window| {
      snapshot.frontmost_app_bundle_id.is_empty() || window.owner_bundle_id.eq_ignore_ascii_case(&snapshot.frontmost_app_bundle_id)
    })
    .filter(|window| snapshot.frontmost_window_title.is_empty() || window.title == snapshot.frontmost_window_title)
    .map(|window| window_from_observed(window, Some(true)))
    .find(|window| matches_window_selector(window, selector))
}

fn app_selector_string(selector: &AppSelector) -> Option<String> {
  if let Some(application_id) = &selector.id {
    return matcher_value(application_id).map(ToOwned::to_owned);
  }
  if let Some(name) = &selector.name {
    return matcher_value(name).map(ToOwned::to_owned);
  }
  selector.process_id.map(|pid| pid.to_string())
}

fn matcher_value(matcher: &TextMatcher) -> Option<&str> {
  match matcher {
    TextMatcher::Exact(value) => Some(value),
    TextMatcher::Contains(_) => None,
  }
}

fn window_selection_from_selector(selector: &WindowSelector) -> WindowSelection {
  WindowSelection {
    window_ref: None,
    native_window_id: None,
    title: selector.title.as_ref().and_then(matcher_value).map(str::to_string),
  }
}

fn rect_from_observed(rect: &ObservedRect) -> Rect {
  Rect::new(rect.x as f64, rect.y as f64, rect.width as f64, rect.height as f64)
}

fn screen_point_for_window_point(window: &Window, point: WindowPoint) -> ScreenPoint {
  let point = point.point();
  ScreenPoint::new(window.frame.origin.x + point.x, window.frame.origin.y + point.y)
}

fn window_point_for_screen_point(window: &Window, point: ScreenPoint) -> WindowPoint {
  let point = point.point();
  WindowPoint::new(point.x - window.frame.origin.x, point.y - window.frame.origin.y)
}

fn window_number(window: &Window) -> DriverResult<i64> {
  if window.reference.id.trim().is_empty() {
    return Err(invalid_input("window is missing a native macOS window id"));
  }
  window
    .reference
    .id
    .parse::<i64>()
    .map_err(|error| invalid_input(format!("window ref {} was not a native macOS window id: {error}", window.reference.id)))
}

fn window_pid(window: &Window) -> DriverResult<i64> {
  window.process_id.map(i64::from).ok_or_else(|| invalid_input("window is missing an owner process id"))
}

fn click_parts(click: &Click) -> DriverResult<(i64, u64)> {
  match click {
    Click::Single => Ok((1, 0)),
    Click::Double { interval } => Ok((2, duration_millis(*interval)?)),
    Click::Repeated { count: 0, .. } => Err(invalid_input("repeated click count must be greater than zero")),
    Click::Repeated { count, interval } => Ok((i64::from(*count), duration_millis(*interval)?)),
  }
}

fn type_text_in_window(pid: i64, number: i64, text: &str, options: TypeTextOptions) -> DriverResult<()> {
  let (submit_key_code, inter_char_delay_ms) = type_text_parts(options)?;
  if options.replace_existing {
    crate::native::input::hotkey_in_window(pid, number, 0, true, false, false, false).map_err(backend)?;
    crate::native::input::press_key_in_window(pid, number, 51).map_err(backend)?;
    thread::sleep(Duration::from_millis(100));
  }
  crate::native::input::type_text_in_window(pid, number, text.to_string(), inter_char_delay_ms).map_err(backend)?;
  if let Some(key_code) = submit_key_code {
    crate::native::input::press_key_in_window(pid, number, key_code).map_err(backend)?;
  }
  if !options.settle.is_zero() {
    thread::sleep(options.settle);
  }
  Ok(())
}

fn type_text_foreground(text: &str, options: TypeTextOptions) -> DriverResult<()> {
  if matches!(options.policy, InputPolicy::BackgroundOnly) {
    return Err(invalid_input("foreground type_text cannot use background_only input policy"));
  }

  let (submit_key_code, inter_char_delay_ms) = type_text_parts(options)?;
  if options.replace_existing {
    crate::native::input::hotkey_foreground(0, true, false, false, false).map_err(backend)?;
    thread::sleep(Duration::from_millis(50));
    crate::native::input::press_key_foreground(51).map_err(backend)?;
    thread::sleep(Duration::from_millis(50));
  }
  crate::native::input::type_text_foreground(text.to_string(), inter_char_delay_ms).map_err(backend)?;
  if let Some(key_code) = submit_key_code {
    thread::sleep(Duration::from_millis(50));
    crate::native::input::press_key_foreground(key_code).map_err(backend)?;
  }
  if !options.settle.is_zero() {
    thread::sleep(options.settle);
  }
  Ok(())
}

/// Compile names to native virtual keys before any activation or event creation.
/// Modifiers precede ordinary keys; aliases cannot produce duplicate key-downs.
struct HeldKeyboardBackend {
  recipient: Option<(i64, i64)>,
  codes: Vec<i32>,
}

impl auv_driver_common::KeyboardBackend for HeldKeyboardBackend {
  fn key_count(&self) -> usize {
    self.codes.len()
  }

  fn key(&self, index: usize, down: bool) -> DriverResult<()> {
    let held = if down {
      &self.codes[..=index]
    } else {
      &self.codes[..index]
    };
    let mut flags = 0u64;
    for code in held {
      flags |= match *code {
        55 => 1 << 20, // Command
        56 => 1 << 17, // Shift
        58 => 1 << 19, // Option
        59 => 1 << 18, // Control
        _ => 0,
      };
    }
    crate::native::input::key_transition(self.recipient, self.codes[index], down, flags).map_err(backend)
  }

  fn result(&self) -> InputActionResult {
    match self.recipient {
      Some(_) => InputActionResult::single_success(InputDeliveryPath::WindowTargetedKeyboard),
      None => foreground_system_events_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None),
    }
  }
}

fn parse_key_combination(options: &PressKeysOptions) -> DriverResult<Vec<i32>> {
  if options.keys.is_empty() {
    return Err(invalid_input("keys must not be empty"));
  }
  if !(1..=255).contains(&options.count) {
    return Err(invalid_input("press count must be in 1..=255"));
  }
  if (options.count > 1) == options.interval.is_zero() {
    return Err(invalid_input("repeated presses require a positive interval; a single press requires zero interval"));
  }
  let mut modifiers = Vec::new();
  let mut ordinary = Vec::new();
  let mut implicit_shift = false;
  for key in &options.keys {
    let name = key.trim();
    let modifier = match name.parse::<auv_driver_common::Key>() {
      Ok(auv_driver_common::Key::Modifier(modifier)) => {
        use auv_driver_common::Modifier;
        match modifier {
          Modifier::Meta if matches!(name.to_ascii_lowercase().as_str(), "cmd" | "command") => Some(55),
          Modifier::Shift => Some(56),
          Modifier::Alt => Some(58),
          Modifier::Control => Some(59),
          _ => None,
        }
      }
      _ => None,
    };
    let code = if let Some(code) = modifier {
      code
    } else if let Ok(code) = special_key_code(name) {
      code
    } else {
      let mut chars = name.chars();
      // A spelled-out character name (`comma`, `bracket_left`) resolves through
      // the shared key names to the character it names.
      let character = chars
        .next()
        .filter(|_| chars.next().is_none())
        .or_else(|| match name.parse::<auv_driver_common::Key>() {
          Ok(auv_driver_common::Key::Symbol(symbol)) => symbol.key_char().filter(char::is_ascii_graphic),
          _ => None,
        })
        .ok_or_else(|| invalid_input(format!("invalid key {key:?}; expected one key name or ANSI character")))?;
      implicit_shift |= character.is_ascii_uppercase()
        || matches!(
          character,
          '~' | '!' | '@' | '#' | '$' | '%' | '^' | '&' | '*' | '(' | ')' | '_' | '+' | '{' | '}' | '|' | ':' | '"' | '<' | '>' | '?'
        );
      macos_virtual_key_code(character)?
    };
    if modifiers.contains(&code) || ordinary.contains(&code) {
      return Err(invalid_input(format!("duplicate key {key:?}")));
    }
    if modifier.is_some() {
      modifiers.push(code);
    } else {
      ordinary.push(code);
    }
  }
  if implicit_shift && !modifiers.contains(&56) {
    modifiers.push(56);
  }
  modifiers.extend(ordinary);
  Ok(modifiers)
}

/// Maps an ANSI shortcut character to the physical key used by macOS CGEvent.
/// Text input does not use this table; Unicode payloads preserve layout and
/// non-ASCII characters without pretending they are physical shortcut keys.
fn macos_virtual_key_code(character: char) -> DriverResult<i32> {
  let code = match character.to_ascii_lowercase() {
    'a' => 0,
    's' => 1,
    'd' => 2,
    'f' => 3,
    'h' => 4,
    'g' => 5,
    'z' => 6,
    'x' => 7,
    'c' => 8,
    'v' => 9,
    'b' => 11,
    'q' => 12,
    'w' => 13,
    'e' => 14,
    'r' => 15,
    'y' => 16,
    't' => 17,
    '1' | '!' => 18,
    '2' | '@' => 19,
    '3' | '#' => 20,
    '4' | '$' => 21,
    '6' | '^' => 22,
    '5' | '%' => 23,
    '=' | '+' => 24,
    '9' | '(' => 25,
    '7' | '&' => 26,
    '-' | '_' => 27,
    '8' | '*' => 28,
    '0' | ')' => 29,
    ']' | '}' => 30,
    'o' => 31,
    'u' => 32,
    '[' | '{' => 33,
    'i' => 34,
    'p' => 35,
    'l' => 37,
    'j' => 38,
    '\'' | '"' => 39,
    'k' => 40,
    ';' | ':' => 41,
    '\\' | '|' => 42,
    ',' | '<' => 43,
    '/' | '?' => 44,
    'n' => 45,
    'm' => 46,
    '.' | '>' => 47,
    '`' | '~' => 50,
    other => return Err(invalid_input(format!("unsupported macOS shortcut key {other}"))),
  };
  Ok(code)
}

// Native key constants follow the macOS SDK `HIToolbox/Events.h` kVK_* values.
// NOTICE: These are physical keys; Unicode text uses a separate native payload.
fn special_key_code(raw: &str) -> DriverResult<i32> {
  use auv_driver_common::{Key, Keysym};
  let normalized = raw.trim().to_ascii_lowercase();
  // Preserve macOS Delete as backspace and Enter as keypad Enter.
  if normalized.starts_with('f') && normalized[1..].parse::<u32>().is_ok_and(|number| normalized != format!("f{number}")) {
    return Err(invalid_input(format!("unknown key name {normalized}")));
  }
  let name = if normalized == "delete" {
    "backspace"
  } else {
    &normalized
  };
  if normalized == "back" {
    return Err(invalid_input(format!("unknown key name {normalized}")));
  }
  match name.parse::<Key>() {
    Ok(Key::Symbol(Keysym::Return)) => Ok(36),
    Ok(Key::Symbol(Keysym::KP_Enter)) => Ok(76),
    Ok(Key::Symbol(Keysym::Tab)) => Ok(48),
    Ok(Key::Symbol(Keysym::BackSpace)) => Ok(51),
    Ok(Key::Symbol(Keysym::Escape)) => Ok(53),
    Ok(Key::Symbol(Keysym::space)) => Ok(49),
    Ok(Key::Symbol(Keysym::Home)) => Ok(115),
    Ok(Key::Symbol(Keysym::End)) => Ok(119),
    Ok(Key::Symbol(Keysym::Page_Up)) => Ok(116),
    Ok(Key::Symbol(Keysym::Page_Down)) => Ok(121),
    Ok(Key::Symbol(Keysym::Delete)) => Ok(117),
    Ok(Key::Symbol(Keysym::Left)) => Ok(123),
    Ok(Key::Symbol(Keysym::Right)) => Ok(124),
    Ok(Key::Symbol(Keysym::Down)) => Ok(125),
    Ok(Key::Symbol(Keysym::Up)) => Ok(126),
    Ok(Key::Symbol(Keysym::F1)) => Ok(122),
    Ok(Key::Symbol(Keysym::F2)) => Ok(120),
    Ok(Key::Symbol(Keysym::F3)) => Ok(99),
    Ok(Key::Symbol(Keysym::F4)) => Ok(118),
    Ok(Key::Symbol(Keysym::F5)) => Ok(96),
    Ok(Key::Symbol(Keysym::F6)) => Ok(97),
    Ok(Key::Symbol(Keysym::F7)) => Ok(98),
    Ok(Key::Symbol(Keysym::F8)) => Ok(100),
    Ok(Key::Symbol(Keysym::F9)) => Ok(101),
    Ok(Key::Symbol(Keysym::F10)) => Ok(109),
    Ok(Key::Symbol(Keysym::F11)) => Ok(103),
    Ok(Key::Symbol(Keysym::F12)) => Ok(111),
    Ok(Key::Symbol(Keysym::F13)) => Ok(105),
    Ok(Key::Symbol(Keysym::F14)) => Ok(107),
    Ok(Key::Symbol(Keysym::F15)) => Ok(113),
    Ok(Key::Symbol(Keysym::F16)) => Ok(106),
    Ok(Key::Symbol(Keysym::F17)) => Ok(64),
    Ok(Key::Symbol(Keysym::F18)) => Ok(79),
    Ok(Key::Symbol(Keysym::F19)) => Ok(80),
    Ok(Key::Symbol(Keysym::F20)) => Ok(90),
    _ => Err(invalid_input(format!("unknown key name {normalized}"))),
  }
}

fn foreground_system_events_result(
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

fn type_text_parts(options: TypeTextOptions) -> DriverResult<(Option<i32>, u64)> {
  let submit_key_code = text_submit_key_code(options.submit)?;
  let inter_char_delay_ms = duration_millis(options.inter_char_delay)?;
  Ok((submit_key_code, inter_char_delay_ms))
}

/// Converts an AUV [`Scroll`] into CoreGraphics pixel wheel deltas `(wheel2, wheel1)`.
///
/// NOTICE(scroll-delta-sign): CoreGraphics pixel wheel values are positive
/// toward earlier content (classic wheel-up / wheel-left), while AUV `Scroll`
/// is positive toward later content, so both axes are negated. Live probes on
/// macOS 26.3 showed synthetic HID-tap and pid-posted wheel events are not
/// inverted by the natural-scrolling preference; see
/// `docs/ai/references/driver/2026-10-06-scroll-delta-contract.md`.
fn core_graphics_wheel_pixels(scroll: Scroll) -> (f64, f64) {
  (-scroll.delta_x, -scroll.delta_y)
}

fn scroll_attempt_candidates(options: &ScrollOptions) -> Vec<ScrollDeliveryCandidate> {
  match options.policy {
    InputPolicy::ForegroundPreferred => vec![ScrollDeliveryCandidate::ForegroundHid],
    InputPolicy::BackgroundOnly => {
      options.delivery_strategy.candidates.iter().copied().filter(|candidate| *candidate != ScrollDeliveryCandidate::ForegroundHid).collect()
    }
    InputPolicy::BackgroundPreferred => options.delivery_strategy.candidates.clone(),
  }
}

fn click_attempt_candidates(policy: InputPolicy) -> Vec<InputDeliveryPath> {
  match policy {
    InputPolicy::ForegroundPreferred => vec![InputDeliveryPath::ForegroundSystemEvents],
    InputPolicy::BackgroundOnly | InputPolicy::BackgroundPreferred => vec![InputDeliveryPath::WindowTargetedMouse],
  }
}

fn window_mutation_candidates(options: &WindowMutationOptions) -> Vec<WindowMutationCandidate> {
  match options.policy {
    WindowMutationPolicy::ForegroundPreferred => {
      vec![WindowMutationCandidate::ForegroundSystemEvents]
    }
    WindowMutationPolicy::NativeOnly | WindowMutationPolicy::NativePreferred => options.strategy.candidates.to_vec(),
  }
}

fn candidate_supports_window_mutation(candidate: WindowMutationCandidate, kind: WindowMutationKind) -> bool {
  matches!(
    (candidate, kind),
    (WindowMutationCandidate::AxWindowAttribute | WindowMutationCandidate::PlatformNative, WindowMutationKind::SetFrame { .. })
      | (WindowMutationCandidate::AxWindowAction | WindowMutationCandidate::PlatformNative, WindowMutationKind::SetState { .. })
  )
}

/// The first strategy candidate that can perform `kind`, or a failure that
/// lists why each candidate was skipped.
fn select_window_mutation_candidate(kind: WindowMutationKind, options: &WindowMutationOptions) -> DriverResult<WindowMutationCandidate> {
  let mut attempts = Vec::new();
  for candidate in window_mutation_candidates(options) {
    let path = window_mutation_path(candidate);
    if candidate == WindowMutationCandidate::ForegroundSystemEvents {
      // TODO(window-management-foreground-fallback): pointer/system-events
      // fallback is deferred because this slice wires only the native AX
      // bridge; enable when the owner approves foreground repositioning.
      attempts.push(WindowMutationAttempt::failure(path, "foreground window mutation fallback is not implemented in this slice"));
      continue;
    }
    if candidate_supports_window_mutation(candidate, kind) {
      return Ok(candidate);
    }
    let message = format!("{} does not support {}", window_mutation_candidate_name(candidate), window_mutation_kind_name(kind));
    attempts.push(WindowMutationAttempt::failure(path, message));
  }
  Err(window_mutation_failure(attempts))
}

fn window_mutation_path(candidate: WindowMutationCandidate) -> WindowMutationPath {
  match candidate {
    WindowMutationCandidate::AxWindowAttribute => WindowMutationPath::AxWindowAttribute,
    WindowMutationCandidate::AxWindowAction => WindowMutationPath::AxWindowAction,
    WindowMutationCandidate::PlatformNative => WindowMutationPath::PlatformNative,
    WindowMutationCandidate::ForegroundSystemEvents => WindowMutationPath::ForegroundSystemEvents,
  }
}

fn window_mutation_candidate_name(candidate: WindowMutationCandidate) -> &'static str {
  match candidate {
    WindowMutationCandidate::AxWindowAttribute => "ax_window_attribute",
    WindowMutationCandidate::AxWindowAction => "ax_window_action",
    WindowMutationCandidate::PlatformNative => "platform_native",
    WindowMutationCandidate::ForegroundSystemEvents => "foreground_system_events",
  }
}

fn window_mutation_kind_name(kind: WindowMutationKind) -> &'static str {
  match kind {
    WindowMutationKind::SetFrame { .. } => "set_frame",
    WindowMutationKind::SetState { .. } => "set_state",
  }
}

/// The owner pid and window number of a window that may be mutated.
///
/// NOTICE(macos-ax-self-deadlock): AX writes to windows of the calling process
/// can deadlock, because the process would have to answer its own AX message.
/// Windows of the Runner's own process are refused.
fn mutation_target(window: &Window) -> DriverResult<(i64, i64)> {
  let pid = window_pid(window)?;
  let number = window_number(window)?;
  if pid == i64::from(std::process::id()) {
    return Err(invalid_input(format!(
      "window:{} belongs to the AUV process itself; AX window mutation of the calling process is refused",
      window.reference.id
    )));
  }
  Ok((pid, number))
}

fn validate_frame_request(origin: Option<Point>, size: Option<Size>) -> DriverResult<()> {
  if origin.is_none() && size.is_none() {
    return Err(invalid_input("set_frame needs an origin, a size, or both"));
  }
  if let Some(origin) = origin
    && !(origin.x.is_finite() && origin.y.is_finite())
  {
    return Err(invalid_input("set_frame origin must be finite"));
  }
  if let Some(size) = size
    && !(size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0)
  {
    return Err(invalid_input("set_frame size must be finite and greater than zero"));
  }
  Ok(())
}

/// `set_frame` moves only a `Normal` window and never changes state on its own.
/// A state the platform cannot tell (`None`) does not block the call.
///
/// NOTICE(macos-window-maximized): a maximized macOS window is only a frame,
/// so `set_frame` moves it like a normal window instead of refusing it.
/// Otherwise a window maximized by hand, which `set_state(Normal)` cannot
/// restore, could not be moved at all.
fn require_normal_window(state: &WindowState, operation: &str) -> DriverResult<()> {
  let (blocking, recovery) = if state.is_fullscreen == Some(true) {
    // TODO(window-fullscreen): `set_state` has no full-screen exit yet, so the
    // recovery points at the application.
    ("full screen", "exit full screen in the application first")
  } else if state.is_minimized == Some(true) {
    ("minimized", "call set_state(Normal) first")
  } else {
    return Ok(());
  };
  Err(DriverError::FailedPrecondition {
    message: format!("{operation} needs a normal window, but the window is {blocking}"),
    recovery: Some(recovery.to_string()),
  })
}

/// NOTICE(macos-set-frame-passes): an app can apply an AXPosition or AXSize
/// write partially or undo one with the other. Up to six passes rewrite only
/// the component that has not settled, as trycua/cua
/// `platform-macos/src/tools/set_window_frame.rs` does (checked 2026-11-03).
const MAX_FRAME_PASSES: usize = 6;
/// Time for the WindowServer frame to reflect one AX write before it is read.
const FRAME_PASS_INTERVAL: Duration = Duration::from_millis(50);

/// NOTICE(macos-window-state-animation): minimize and deminiaturize
/// animate in the WindowServer for a few hundred milliseconds after the AX
/// call returns, and `CGWindowList` reports the in-between frame (observed
/// `4430,2628 252x25` 100 ms after unminimizing a TextEdit window). Polling
/// until two consecutive reads agree, bounded by
/// `STATE_FRAME_STABLE_TIMEOUT`, keeps `after_frame` from reporting an
/// animation frame. A window still animating at the timeout is reported as
/// read; the state check does not depend on the frame.
const STATE_FRAME_POLL_INTERVAL: Duration = Duration::from_millis(50);
const STATE_FRAME_STABLE_TIMEOUT: Duration = Duration::from_secs(1);

fn wait_for_stable_window_frame(window_number: i64) -> DriverResult<()> {
  let deadline = std::time::Instant::now() + STATE_FRAME_STABLE_TIMEOUT;
  let mut previous = crate::native::window::window_server_frame(window_number).map_err(backend)?;
  while std::time::Instant::now() < deadline {
    thread::sleep(STATE_FRAME_POLL_INTERVAL);
    let current = crate::native::window::window_server_frame(window_number).map_err(backend)?;
    if current == previous {
      return Ok(());
    }
    previous = current;
  }
  Ok(())
}

/// The frame components that still differ from the requested origin or size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameComponents {
  origin: bool,
  size: bool,
}

impl FrameComponents {
  fn is_settled(self) -> bool {
    !self.origin && !self.size
  }

  fn describe(self) -> &'static str {
    match (self.origin, self.size) {
      (true, true) => "AXPosition then AXSize",
      (true, false) => "AXPosition",
      (false, true) => "AXSize",
      (false, false) => "nothing",
    }
  }
}

fn unsettled_frame_components(current: Rect, origin: Option<Point>, size: Option<Size>, tolerance: f64) -> FrameComponents {
  FrameComponents {
    origin: origin.is_some_and(|origin| (current.origin.x - origin.x).abs() > tolerance || (current.origin.y - origin.y).abs() > tolerance),
    size: size
      .is_some_and(|size| (current.size.width - size.width).abs() > tolerance || (current.size.height - size.height).abs() > tolerance),
  }
}

fn frames_close(left: Rect, right: Rect, tolerance: f64) -> bool {
  unsettled_frame_components(left, Some(right.origin), Some(right.size), tolerance).is_settled()
}

fn frame_tolerance(verification: &WindowMutationVerification) -> DriverResult<f64> {
  match *verification {
    WindowMutationVerification::FrameTolerance { points } if points.is_finite() && points >= 0.0 => Ok(points),
    WindowMutationVerification::FrameTolerance { .. } => {
      Err(invalid_input("window mutation frame tolerance must be a finite non-negative value"))
    }
    WindowMutationVerification::BestEffortState => Ok(2.0),
  }
}

/// The AX minimize or unminimize call `target` needs, or `None` when the
/// window is already (un)minimized. An unknown fact (`None`) counts as "not in
/// the target state" because both writes are idempotent. Maximize and restore
/// are frame writes that `set_state` makes after this step.
fn window_state_step(
  current: &WindowState,
  target: WindowStateTarget,
) -> DriverResult<Option<crate::native::window::DecodedWindowMutationKind>> {
  use crate::native::window::DecodedWindowMutationKind::{Minimize, Unminimize};
  if current.is_fullscreen == Some(true) {
    // TODO(window-fullscreen): leaving full screen needs `AXFullScreen=false`
    // and waiting for the Space animation; deferred with the full-screen target.
    return Err(DriverError::unsupported_with_recovery(
      "window.setState on a full-screen macOS window",
      "exit full screen in the application first",
    ));
  }
  Ok(match target {
    WindowStateTarget::Minimized => (current.is_minimized != Some(true)).then_some(Minimize),
    WindowStateTarget::Normal | WindowStateTarget::Maximized => (current.is_minimized != Some(false)).then_some(Unminimize),
  })
}

/// What `set_state(Normal)` does to the frame of a window that is not minimized.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NormalFrame {
  /// The window is not maximized, so its frame already is its normal frame.
  Keep,
  /// Restore the frame recorded by `set_state(Maximized)`.
  Restore(Rect),
  /// Maximized, but no frame was recorded; the frame stays.
  Unrecorded,
}

fn normal_frame(state: &WindowState, recorded: Option<Rect>) -> NormalFrame {
  match (state.is_maximized, recorded) {
    (Some(true), Some(frame)) => NormalFrame::Restore(frame),
    (Some(true), None) => NormalFrame::Unrecorded,
    _ => NormalFrame::Keep,
  }
}

/// Writes the requested origin and/or size through AX, `AXPosition` before
/// `AXSize`, and rereads the WindowServer frame after every pass. Each
/// corrective pass rewrites only the component that has not settled
/// (`NOTICE(macos-set-frame-passes)`).
fn write_frame(
  (pid, number): (i64, i64),
  current: Rect,
  origin: Option<Point>,
  size: Option<Size>,
  tolerance: f64,
  path: WindowMutationPath,
  attempts: &mut Vec<WindowMutationAttempt>,
) -> DriverResult<()> {
  let mut pending = unsettled_frame_components(current, origin, size, tolerance);
  for pass in 1..=MAX_FRAME_PASSES {
    if pending.is_settled() {
      break;
    }
    let written_origin = origin.filter(|_| pending.origin).map(|origin| (origin.x, origin.y));
    let written_size = size.filter(|_| pending.size).map(|size| (size.width, size.height));
    if let Err(error) = crate::native::window::write_window_frame(pid, number, written_origin, written_size) {
      attempts.push(WindowMutationAttempt::failure(path, format!("pass {pass}: {error}")));
      return Err(window_mutation_failure(std::mem::take(attempts)));
    }
    attempts.push(WindowMutationAttempt::success(path, format!("pass {pass}: wrote {}", pending.describe())));
    thread::sleep(FRAME_PASS_INTERVAL);
    let Some((x, y, width, height)) = crate::native::window::window_server_frame(number).map_err(backend)? else {
      return Err(not_found(format!("window:{number}")));
    };
    pending = unsettled_frame_components(Rect::new(x, y, width, height), origin, size, tolerance);
  }
  Ok(())
}

/// Checks the settled frame after `write_frame`: `false` when it matches the
/// request, `true` when the application settled the window at another frame
/// (`adjusted_by_app`), and an error when the window did not move.
fn frame_adjusted_by_app(
  before: Rect,
  after: Rect,
  origin: Option<Point>,
  size: Option<Size>,
  tolerance: f64,
  attempts: &[WindowMutationAttempt],
) -> DriverResult<bool> {
  if unsettled_frame_components(after, origin, size, tolerance).is_settled() {
    return Ok(false);
  }
  if frames_close(after, before, tolerance) {
    let target = Rect {
      origin: origin.unwrap_or(before.origin),
      size: size.unwrap_or(before.size),
    };
    return Err(backend(format!(
      "window mutation verification failed: window frame stayed at {after:?} instead of {target:?}; {}",
      attempts.iter().filter_map(|attempt| attempt.message.as_deref()).collect::<Vec<_>>().join("; ")
    )));
  }
  Ok(true)
}

/// Reads the target state back after settle: `Minimized` must be minimized,
/// and `Normal` and `Maximized` must not be. The maximize and restore frames
/// are checked by `frame_adjusted_by_app`; an app that keeps the window
/// smaller than the visible frame is reported with `adjusted_by_app`, not as
/// a failure.
fn verify_window_state(target: WindowStateTarget, after: &WindowState) -> DriverResult<()> {
  let failure = match target {
    WindowStateTarget::Minimized if after.is_minimized != Some(true) => "window was not minimized",
    WindowStateTarget::Normal | WindowStateTarget::Maximized if after.is_minimized == Some(true) => "window was still minimized",
    _ => return Ok(()),
  };
  Err(backend(format!("window mutation verification failed: {failure}")))
}

/// The bundle id of the frontmost application, or `None` when it could not be read.
fn frontmost_application_id() -> Option<String> {
  crate::native::window::list_windows(ListWindowsOptions::all_visible(1)).ok().map(|snapshot| snapshot.frontmost_app_bundle_id)
}

fn focus_disturbance(before: Option<String>, after: Option<String>) -> DisturbanceLevel {
  match (before, after) {
    (Some(before), Some(after)) if before == after => DisturbanceLevel::None,
    (Some(_), Some(_)) => DisturbanceLevel::Foreground,
    _ => DisturbanceLevel::Unknown,
  }
}

fn window_mutation_result(
  selected_path: WindowMutationPath,
  attempts: Vec<WindowMutationAttempt>,
  before: &Window,
  after: &Window,
  focus_disturbance: DisturbanceLevel,
) -> WindowMutationResult {
  WindowMutationResult {
    selected_path,
    attempts,
    before_frame: Some(before.frame),
    after_frame: Some(after.frame),
    before_state: Some(before.state.clone()),
    after_state: Some(after.state.clone()),
    adjusted_by_app: false,
    focus_disturbance,
    mouse_disturbance: DisturbanceLevel::None,
  }
}

fn window_mutation_failure(attempts: Vec<WindowMutationAttempt>) -> DriverError {
  let mut parts =
    attempts.into_iter().filter_map(|attempt| attempt.message.map(|message| format!("{:?}: {message}", attempt.path))).collect::<Vec<_>>();
  if parts.is_empty() {
    parts.push("no window mutation candidate succeeded".to_string());
  }
  if parts.is_empty() {
    return DriverError::unsupported("window_mutation");
  }
  backend(format!("window mutation failed after {} attempt(s): {}", parts.len(), parts.join("; ")))
}

fn foreground_prepare_options(settle: Duration) -> PrepareForInputOptions {
  PrepareForInputOptions {
    activation: ActivationPolicy::Foreground { settle },
    preserve_frontmost: false,
    install_focus_guard: false,
    settle: Duration::ZERO,
  }
}

/// Zero denotes application scope only at the native event boundary. Do not
/// stamp a window id for application-scoped events.
fn resolve_input_target(target: &auv_driver_common::InputTarget) -> DriverResult<(i64, i64)> {
  match target {
    InputTarget::Foreground => Err(invalid_input("foreground input has no resolved recipient")),
    auv_driver_common::InputTarget::Window(window) => {
      let pid = window_pid(window)?;
      let number = window_number(window)?;
      if pid <= 0 || pid > i64::from(i32::MAX) || number <= 0 || number > i64::from(u32::MAX) {
        return Err(invalid_input("window input requires a positive native window id and process id"));
      }
      Ok((pid, number))
    }
    auv_driver_common::InputTarget::Application { id: bundle_id } => {
      if bundle_id.trim().is_empty() {
        return Err(invalid_input("application bundle id must not be empty"));
      }
      let pid = crate::native::window::running_application_pid(bundle_id).map_err(backend)?;
      match pid {
        0 => Err(not_found(format!("running application {bundle_id:?}"))),
        -1 => Err(invalid_input(format!("application {bundle_id:?} has multiple running instances; select a window"))),
        pid if pid > 0 => Ok((pid, 0)),
        _ => Err(backend("invalid running application pid")),
      }
    }
  }
}

fn prepare_input_recipient((pid, number): (i64, i64), options: PrepareForInputOptions) -> DriverResult<InputPreparationLease> {
  if options.install_focus_guard {
    return Err(DriverError::unsupported("focus_guard"));
  }
  match options.activation {
    ActivationPolicy::FocusWithoutRaise => return Err(DriverError::unsupported("focus_without_raise")),
    ActivationPolicy::Foreground { settle } => {
      if options.preserve_frontmost {
        return Err(DriverError::unsupported("foreground_restore"));
      }
      crate::native::window::validate_input_target(pid, number, true).map_err(backend)?;
      activate_process(pid)?;
      crate::native::window::confirm_input_focus(pid, number).map_err(backend)?;
      if !settle.is_zero() {
        thread::sleep(settle);
      }
    }
    ActivationPolicy::NoChange | ActivationPolicy::Background => {
      crate::native::window::validate_input_target(pid, number, false).map_err(backend)?;
    }
  }
  if !options.settle.is_zero() {
    thread::sleep(options.settle);
  }
  Ok(InputPreparationLease::noop())
}

fn activate_process(pid: i64) -> DriverResult<()> {
  run_osascript(&[&format!(
    "tell application \"System Events\" to set frontmost of first application process whose unix id is {pid} to true"
  )])
}

fn activate_app_for_window(window: &Window) -> DriverResult<()> {
  if let Some(pid) = window.process_id {
    return activate_process(i64::from(pid));
  }
  run_osascript(&[&foreground_activation_script(window)?])
}

fn foreground_activation_script(window: &Window) -> DriverResult<String> {
  if let Some(pid) = window.process_id {
    // NOTICE(bundleless-window-activation): `tell application ... to activate`
    // cannot resolve executables without a bundle id, including Android
    // Emulator's `qemu-system-aarch64`. System Events can activate the owning
    // application process by pid. Keep the bundle/name fallback only for
    // legacy Window values without an observed owner pid.
    return Ok(format!("tell application \"System Events\" to set frontmost of first application process whose unix id is {pid} to true"));
  }
  if let Some(bundle_id) = &window.application_id {
    return Ok(format!("tell application id \"{}\" to activate", escape_applescript(bundle_id)));
  }
  if let Some(app_name) = &window.app_name {
    return Ok(format!("tell application \"{}\" to activate", escape_applescript(app_name)));
  }
  Err(invalid_input("foreground input requires a window owner pid, bundle id, or application name"))
}

#[derive(Clone, Debug)]
struct MacosDisplayTarget {
  index: usize,
  display: Display,
}

fn resolve_display_target(targets: &[MacosDisplayTarget], selector: Option<&str>) -> DriverResult<MacosDisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| target.display.id == selector || target.display.name.as_deref().is_some_and(|display_ref| display_ref == selector))
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets.iter().find(|target| target.display.is_primary).or_else(|| targets.first()).cloned().ok_or_else(|| not_found("primary display"))
}

#[cfg(target_os = "macos")]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  display_targets_from_monitors(&monitors)
}

#[cfg(not(target_os = "macos"))]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(target_os = "macos")]
fn display_capture(display: Display, image: RgbaImage, scale_factor: f64, backend: &str) -> DisplayCapture {
  DisplayCapture {
    capture: Capture {
      origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(display.frame.origin))),
      image,
      bounds: display.frame,
      scale_factor,
      backend: backend.into(),
      fallback_reason: None,
    },
    display,
  }
}

#[cfg(target_os = "macos")]
fn capture_display_xcap(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_target(&targets, selector)?;
  let monitor = monitors.get(target.index).ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let image = monitor.capture_image().map_err(|error| backend(format!("failed to capture display: {error}")))?;
  let image = RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured display RGBA image"))?;
  let scale_factor = target.display.scale_factor;
  Ok(display_capture(target.display, image, scale_factor, "xcap.macos"))
}

#[cfg(not(target_os = "macos"))]
fn capture_display_xcap(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(DriverError::unsupported("display.capture"))
}

#[cfg(target_os = "macos")]
fn capture_region_xcap(selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  let monitors = xcap::Monitor::all().map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_for_global_region(&targets, selector, region)?;
  // The backend addresses whole capture units, so cover a fractional region
  // (an OCR box, a layout rect) with whole units inside the display, and
  // report the covered rect as the capture bounds.
  let region = region.round_out().intersect(target.display.frame).ok_or_else(|| not_found("display containing region"))?;
  let monitor = monitors.get(target.index).ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let local_x = integral_capture_dimension("x", region.origin.x - target.display.frame.origin.x)?;
  let local_y = integral_capture_dimension("y", region.origin.y - target.display.frame.origin.y)?;
  let width = integral_positive_capture_dimension("width", region.size.width)?;
  let height = integral_positive_capture_dimension("height", region.size.height)?;
  let image = monitor
    .capture_region(local_x, local_y, width, height)
    .map_err(|error| backend(format!("failed to capture display region: {error}")))?;
  let image = RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured region RGBA image"))?;
  let capture = Capture {
    origin: Some(auv_driver_common::Position::in_screen(auv_driver_common::ScreenPoint::from(region.origin))),
    image,
    bounds: region,
    scale_factor: target.display.scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason: None,
  };
  Ok(RegionCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "macos"))]
fn capture_region_xcap(_selector: Option<&str>, _region: Rect) -> DriverResult<RegionCapture> {
  Err(DriverError::unsupported("display.capture_region"))
}

#[cfg(target_os = "macos")]
fn display_targets_from_monitors(monitors: &[xcap::Monitor]) -> DriverResult<Vec<MacosDisplayTarget>> {
  if monitors.is_empty() {
    return Err(DriverError::PermissionDenied {
      permission: "screen_recording",
      message: None,
      recovery: Some(
        "xcap returned no displays; run `auv doctor --json` and grant Screen Recording to the terminal or agent process that launches AUV"
          .to_string(),
      ),
    });
  }
  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor.x().map_err(|error| backend(format!("failed to read display x: {error}")))? as f64;
      let y = monitor.y().map_err(|error| backend(format!("failed to read display y: {error}")))? as f64;
      let width = monitor.width().map_err(|error| backend(format!("failed to read display width: {error}")))? as f64;
      let height = monitor.height().map_err(|error| backend(format!("failed to read display height: {error}")))? as f64;
      let scale_factor = monitor.scale_factor().map_err(|error| backend(format!("failed to read display scale: {error}")))? as f64;
      let is_builtin = monitor.is_builtin().map_err(|error| backend(format!("failed to read display built-in flag: {error}")))?;
      let native_id = monitor.id().map_err(|error| backend(format!("failed to read display id: {error}")))?.to_string();
      Ok(MacosDisplayTarget {
        index,
        display: Display {
          id: native_id,
          name: Some(format!("display_{index}")),
          frame: Rect::new(x, y, width, height),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor,
          is_primary: monitor.is_primary().map_err(|error| backend(format!("failed to read display primary flag: {error}")))?,
          is_builtin: Some(is_builtin),
        },
      })
    })
    .collect()
}

fn resolve_display_for_global_region(
  targets: &[MacosDisplayTarget],
  selector: Option<&str>,
  region: Rect,
) -> DriverResult<MacosDisplayTarget> {
  let selected = if selector.is_some() {
    vec![resolve_display_target(targets, selector)?]
  } else {
    targets.to_vec()
  };
  selected.into_iter().find(|target| rect_contains_rect(target.display.frame, region)).ok_or_else(|| not_found("display containing region"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

fn integral_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  if value.fract() != 0.0 {
    return Err(invalid_input(format!("region {name} must be an integer in backend capture units")));
  }
  if value < 0.0 || value > u32::MAX as f64 {
    return Err(invalid_input(format!("region {name} is outside the capture backend range")));
  }
  Ok(value as u32)
}

fn integral_positive_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  let integral = integral_capture_dimension(name, value)?;
  if integral == 0 {
    return Err(invalid_input(format!("region {name} must be positive")));
  }
  Ok(integral)
}

#[cfg(target_os = "macos")]
fn capture_window(window: &Window, resolution: CaptureResolution) -> DriverResult<Capture> {
  // Prefer the Swift FFI ScreenCaptureKit path for typed window capture.
  //
  // Why this path exists instead of shelling out to `screencapture` or using
  // the Rust `xcap` crate as the primary backend:
  //
  // | Backend | Capture + OCR wall time observed locally | Runtime shape |
  // |---|---:|---|
  // | Swift FFI ScreenCaptureKit + Vision | ~1.28-1.60s | in-process, typed RGBA frame, no required PNG temp file |
  // | `screencapture` CLI + Vision | ~1.10-1.20s | subprocess + filesystem PNG handoff |
  // | `xcap` window/display capture + Vision | ~6-7s | in-process Rust API, but slow on this macOS target |
  //
  // `screencapture` can be slightly faster in the narrow benchmark, but it
  // forces process spawning and file handoff into the operation path. The FFI
  // path keeps capture and OCR composable in memory, avoids making artifact
  // writes part of primitive execution, and gives us a place to expose native
  // permission/error details. `xcap` stays as a fallback while the new native
  // path is still being proven across app/window states.
  match capture_window_swift(window, resolution) {
    Ok(capture) => Ok(capture),
    // The window itself is in the wrong state; xcap would capture the same frame.
    Err(error @ DriverError::StaleUiReference { .. }) => Err(error),
    Err(swift_error) => {
      let fallback_reason = swift_error.to_string();
      capture_window_xcap(window, Some(fallback_reason.clone())).map(|capture| capture.at_resolution(resolution)).map_err(|xcap_error| {
        backend(format!(
          "native Swift window capture failed before xcap fallback: {fallback_reason}; xcap fallback also failed: {xcap_error}"
        ))
      })
    }
  }
}

#[cfg(target_os = "macos")]
fn capture_window_swift(window: &Window, resolution: CaptureResolution) -> DriverResult<Capture> {
  let native_window_id = window
    .reference
    .id
    .parse::<i64>()
    .map_err(|error| invalid_input(format!("window ref {} was not a native macOS window id: {error}", window.reference.id)))?;
  let logical = resolution == CaptureResolution::Logical;
  let capture_and_classify = || -> DriverResult<_> {
    let capture = crate::native::capture::capture_window_rgba(native_window_id, logical).map_err(backend)?;
    let frame = classify_captured_frame(window.frame.size, capture.window_frame.size, || {
      window.process_id.and_then(|pid| crate::native::capture::window_ax_size_for(pid, native_window_id))
    });
    Ok((capture, frame))
  };
  // NOTICE(window-capture-frame-mismatch): ScreenCaptureKit sizes the image
  // from the window's frame at capture time. Mission Control, App Exposé and
  // minimize animations shrink that frame (and the window-server bounds) for a
  // moment; Mission Control frames come back black (measured 2026-10-08:
  // 955x558 pt for a 1644x960 pt window while AX kept 1644x960). Scaling such
  // a frame by `window.frame` gave captures a wrong `scale_factor` (1.17) and
  // garbage pixels. A transformed frame is retried once after the animation,
  // then reported as stale; a real resize (AX agrees with the captured size)
  // is captured at the new frame. Mission Control also returns some black
  // frames at the full frame size while it opens; those pass this check, and
  // only content checks can tell them apart from a dark window.
  let (mut capture, mut frame) = capture_and_classify()?;
  if let CapturedWindowFrame::Transformed { .. } = frame {
    thread::sleep(Duration::from_millis(300));
    (capture, frame) = capture_and_classify()?;
  }
  let bounds = match frame {
    CapturedWindowFrame::Resolved => window.frame,
    CapturedWindowFrame::Resized => capture.window_frame,
    CapturedWindowFrame::Transformed { minimized } => {
      let cause = if minimized {
        "the window is minimized"
      } else {
        "the window is shown by Mission Control, App Exposé or a window animation"
      };
      return Err(DriverError::StaleUiReference {
        message: format!(
          "window {} was {}x{} pt at capture time but resolved as {}x{} pt: {cause}",
          window.reference.id,
          capture.window_frame.size.width,
          capture.window_frame.size.height,
          window.frame.size.width,
          window.frame.size.height
        ),
        recovery: Some("retry once the window is shown normally again".to_string()),
      });
    }
  };
  let width = u32::try_from(capture.image_width).map_err(|error| backend(format!("native capture returned invalid width: {error}")))?;
  let height = u32::try_from(capture.image_height).map_err(|error| backend(format!("native capture returned invalid height: {error}")))?;
  let image =
    RgbaImage::from_raw(width, height, capture.rgba_bytes).ok_or_else(|| backend("failed to decode native captured window RGBA image"))?;
  let scale_factor = if bounds.size.width > 0.0 {
    f64::from(width) / bounds.size.width
  } else {
    1.0
  };
  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image,
    bounds,
    scale_factor,
    backend: "macos.screencapturekit.ffi".to_string(),
    fallback_reason: None,
  })
}

#[cfg(target_os = "macos")]
/// How the frame ScreenCaptureKit captured a window at relates to the frame
/// the window was resolved with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapturedWindowFrame {
  /// The captured frame is the resolved frame.
  Resolved,
  /// The window was resized since it was resolved; its application agrees
  /// with the captured size.
  Resized,
  /// The window server shows the window transformed (Mission Control, App
  /// Exposé, a minimize animation) or minimized. Also used when the
  /// application's size is unavailable, since a resize cannot be confirmed.
  Transformed { minimized: bool },
}

#[cfg(target_os = "macos")]
/// Classifies a captured window frame. `ax_size` is only consulted when the
/// sizes differ, since it costs an Accessibility round trip.
fn classify_captured_frame(
  resolved: Size,
  captured: Size,
  ax_size: impl FnOnce() -> Option<crate::native::capture::WindowAxSize>,
) -> CapturedWindowFrame {
  // A window resolved without a frame has nothing to compare against.
  if resolved.width <= 0.0 || resolved.height <= 0.0 || same_size(resolved, captured) {
    return CapturedWindowFrame::Resolved;
  }
  match ax_size() {
    Some(ax) if ax.minimized => CapturedWindowFrame::Transformed { minimized: true },
    Some(ax) if same_size(ax.size, captured) => CapturedWindowFrame::Resized,
    _ => CapturedWindowFrame::Transformed { minimized: false },
  }
}

#[cfg(target_os = "macos")]
/// Equal up to point rounding between ScreenCaptureKit, the window server and
/// Accessibility.
fn same_size(left: Size, right: Size) -> bool {
  (left.width - right.width).abs() <= 1.0 && (left.height - right.height).abs() <= 1.0
}

#[cfg(target_os = "macos")]
fn capture_window_xcap(window: &Window, fallback_reason: Option<String>) -> DriverResult<Capture> {
  let native_window_id = window
    .reference
    .id
    .parse::<u32>()
    .map_err(|error| invalid_input(format!("window ref {} was not a native macOS window id: {error}", window.reference.id)))?;
  let windows = xcap::Window::all().map_err(|error| backend(format!("failed to enumerate windows: {error}")))?;
  let xcap_window = windows
    .iter()
    .find(|candidate| candidate.id().is_ok_and(|id| id == native_window_id))
    .ok_or_else(|| not_found(format!("native window {}", window.reference.id)))?;
  let image = xcap_window.capture_image().map_err(|error| backend(format!("failed to capture window {}: {error}", window.reference.id)))?;
  let width = image.width();
  let height = image.height();
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };
  let image = RgbaImage::from_raw(width, height, image.into_raw()).ok_or_else(|| backend("failed to decode captured window RGBA image"))?;
  Ok(Capture {
    origin: Some(auv_driver_common::Position::in_window(&window.reference, auv_driver_common::WindowPoint::new(0.0, 0.0))),
    image,
    bounds: window.frame,
    scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason,
  })
}

#[cfg(not(target_os = "macos"))]
fn capture_window(_window: &Window, _resolution: CaptureResolution) -> DriverResult<Capture> {
  Err(DriverError::unsupported("capture_window"))
}

fn relative_rect_to_observed(capture: &Capture, region: RelativeRect) -> ObservedRect {
  let image_width = f64::from(capture.image.width());
  let image_height = f64::from(capture.image.height());
  ObservedRect {
    x: (image_width * region.x).round() as i64,
    y: (image_height * region.y).round() as i64,
    width: (image_width * region.width).round() as i64,
    height: (image_height * region.height).round() as i64,
  }
}

fn ocr_matches_from_recognition(recognition: &TextRecognition, query: &str) -> OcrMatches {
  let matches = recognition
    .find_contains(query)
    .into_iter()
    .map(|recognized| OcrMatch {
      text: recognized.text.clone(),
      confidence: recognized.confidence.unwrap_or_default() as f64,
      bounds: recognized.bounds,
    })
    .collect();
  OcrMatches { matches }
}

fn text_recognition_from_native(native: &NativeOcrTextCapture, capture: &Capture) -> TextRecognition {
  let x_scale = if capture.bounds.size.width > 0.0 {
    f64::from(capture.image.width()) / capture.bounds.size.width
  } else {
    1.0
  };
  let y_scale = if capture.bounds.size.height > 0.0 {
    f64::from(capture.image.height()) / capture.bounds.size.height
  } else {
    1.0
  };
  let matches = native
    .snapshot
    .matches
    .iter()
    .map(|observed| RecognizedText {
      text: observed.text.clone(),
      confidence: Some(observed.confidence as f32),
      bounds: Rect::new(
        capture.bounds.origin.x + observed.bounds.x as f64 / x_scale,
        capture.bounds.origin.y + observed.bounds.y as f64 / y_scale,
        observed.bounds.width as f64 / x_scale,
        observed.bounds.height as f64 / y_scale,
      ),
    })
    .collect::<Vec<_>>();
  let text = matches.iter().map(|recognized| recognized.text.as_str()).collect::<Vec<_>>().join("\n");
  TextRecognition {
    origin: capture.recognition_origin(),
    text,
    regions: matches,
  }
}

fn duration_millis(duration: Duration) -> DriverResult<u64> {
  u64::try_from(duration.as_millis()).map_err(|error| invalid_input(format!("duration too large: {error}")))
}

fn run_osascript(scripts: &[&str]) -> DriverResult<()> {
  let mut command = Command::new("osascript");
  for script in scripts {
    command.arg("-e").arg(script);
  }
  let output = command.output().map_err(|error| backend(format!("failed to run osascript: {error}")))?;
  if output.status.success() {
    Ok(())
  } else {
    Err(backend(String::from_utf8_lossy(&output.stderr).trim()))
  }
}

fn permission_status_from_label(label: &str) -> PermissionStatus {
  match label {
    "granted" => PermissionStatus::Granted,
    "missing" => PermissionStatus::Missing,
    _ => PermissionStatus::Unknown,
  }
}

fn probe_automation_to_system_events() -> PermissionStatus {
  match run_osascript(&[
    "tell application \"System Events\"",
    "return name of first application process whose frontmost is true",
    "end tell",
  ]) {
    Ok(()) => PermissionStatus::Granted,
    Err(_) => PermissionStatus::Missing,
  }
}

fn text_submit_key_code(submit: TextSubmit) -> DriverResult<Option<i32>> {
  match submit {
    TextSubmit::No => Ok(None),
    TextSubmit::Return => Ok(Some(36)),
    TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => {
      Err(invalid_input(format!("text submit {submit:?} is not supported by the macOS desktop driver yet")))
    }
  }
}

struct ClipboardLock {
  path: PathBuf,
}

impl Drop for ClipboardLock {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.path);
  }
}

fn acquire_clipboard_lock(timeout: Duration) -> DriverResult<ClipboardLock> {
  let path = std::env::temp_dir().join("auv-macos-clipboard.lock");
  let started = std::time::Instant::now();
  loop {
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
      Ok(mut file) => {
        let _ = writeln!(file, "pid={}", std::process::id());
        return Ok(ClipboardLock { path });
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => {
        clear_stale_clipboard_lock(&path)?;
        if started.elapsed() >= timeout {
          return Err(backend(format!("timed out waiting for macOS clipboard lock at {}", path.display())));
        }
        thread::sleep(Duration::from_millis(50));
      }
      Err(error) => {
        return Err(backend(format!("failed to acquire macOS clipboard lock {}: {error}", path.display())));
      }
    }
  }
}

fn clear_stale_clipboard_lock(path: &PathBuf) -> DriverResult<()> {
  let Ok(content) = std::fs::read_to_string(path) else {
    return Ok(());
  };
  let Some(pid) = content.lines().find_map(|line| line.strip_prefix("pid=")).and_then(|raw| raw.trim().parse::<u32>().ok()) else {
    return Ok(());
  };
  if process_is_alive(pid) {
    return Ok(());
  }
  std::fs::remove_file(path).map_err(|error| backend(format!("failed to remove stale clipboard lock {}: {error}", path.display())))
}

fn process_is_alive(pid: u32) -> bool {
  if pid == 0 {
    return false;
  }
  Command::new("/bin/kill").args(["-0", &pid.to_string()]).status().is_ok_and(|status| status.success())
}

fn escape_applescript(value: &str) -> String {
  value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn backend(message: impl std::fmt::Display) -> DriverError {
  DriverError::Backend {
    message: message.to_string(),
  }
}

fn invalid_input(message: impl std::fmt::Display) -> DriverError {
  DriverError::InvalidInput {
    message: message.to_string(),
  }
}

fn not_found(target: impl std::fmt::Display) -> DriverError {
  DriverError::NotFound {
    target: target.to_string(),
  }
}

fn paste_text_impl(options: PasteTextOptions, target: Option<(i64, i64)>) -> DriverResult<InputActionResult> {
  let _lock = acquire_clipboard_lock(Duration::from_millis(5_000))?;
  let snapshot = crate::native::clipboard::capture_clipboard_snapshot().map_err(backend)?;
  let result = (|| {
    let submit_key_code = text_submit_key_code(options.submit)?;
    crate::native::clipboard::set_clipboard_text(&options.text).map_err(backend)?;

    if options.replace_existing {
      crate::native::input::press_keys(target, vec![55, 0]).map_err(backend)?;
      thread::sleep(Duration::from_millis(50));
      crate::native::input::press_keys(target, vec![51]).map_err(backend)?;
      thread::sleep(Duration::from_millis(50));
    }
    // Built-in clipboard shortcuts already have known virtual keys; use the
    // same combination event builder without reparsing shortcut strings.
    crate::native::input::press_keys(target, vec![55, 9]).map_err(backend)?;
    thread::sleep(Duration::from_millis(150));
    if let Some(key_code) = submit_key_code {
      thread::sleep(Duration::from_millis(50));
      crate::native::input::press_keys(target, vec![key_code]).map_err(backend)?;
    }
    if !options.settle.is_zero() {
      thread::sleep(options.settle);
    }
    Ok(())
  })();
  let restore_result = crate::native::clipboard::restore_clipboard_snapshot(&snapshot).map_err(backend);
  match (result, restore_result) {
    (Ok(()), Ok(())) => Ok(InputActionResult {
      selected_path: InputDeliveryPath::ClipboardPaste,
      attempts: vec![InputAttempt::success(InputDeliveryPath::ClipboardPaste)],
      verified: false,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::Unknown,
      clipboard_disturbance: DisturbanceLevel::Temporary,
    }),
    (Err(action_error), Ok(())) => Err(action_error),
    (Ok(()), Err(restore_error)) => Err(backend(format!("pasted text but failed to restore clipboard: {restore_error}"))),
    (Err(action_error), Err(restore_error)) => {
      Err(backend(format!("{action_error}; additionally failed to restore clipboard: {restore_error}")))
    }
  }
}

#[cfg(test)]
#[path = "session_test.rs"]
mod tests;

struct PointerBackend;
impl auv_driver_common::mouse_input::MouseBackend for PointerBackend {
  fn current_position(&self) -> DriverResult<Point> {
    let (x, y) = crate::native::pointer::current_mouse_logical_point().map_err(|message| DriverError::Backend { message })?;
    Ok(Point::new(x, y))
  }
  fn move_to(&self, point: Point, held: Option<auv_driver_common::MouseButton>) -> DriverResult<InputActionResult> {
    use auv_driver_common::MouseButton;
    let button = match held {
      None => -1,
      Some(MouseButton::Left) => 0,
      Some(MouseButton::Right) => 1,
      Some(MouseButton::Middle) => 2,
    };
    crate::native::pointer::pointer_event(point.x, point.y, button, 0).map_err(|message| DriverError::Backend { message })?;
    Ok(pointer_delivery_result())
  }
  fn button(&self, point: Point, button: auv_driver_common::MouseButton, down: bool) -> DriverResult<InputActionResult> {
    use auv_driver_common::MouseButton;
    let button = match button {
      MouseButton::Left => 0,
      MouseButton::Right => 1,
      MouseButton::Middle => 2,
    };
    crate::native::pointer::pointer_event(point.x, point.y, button, if down { 1 } else { 2 })
      .map_err(|message| DriverError::Backend { message })?;
    Ok(pointer_delivery_result())
  }
}
fn pointer_delivery_result() -> InputActionResult {
  let mut result = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);
  result.mouse_disturbance = DisturbanceLevel::Temporary;
  result.focus_disturbance = DisturbanceLevel::Unknown;
  result
}

struct WindowPointerBackend {
  session: MacosDriverSession,
  window: Window,
}
impl WindowPointerBackend {
  fn post(&self, point: Point, button: Option<auv_driver_common::MouseButton>, phase: u8) -> DriverResult<InputActionResult> {
    let current = self
      .session
      .window()
      .list()?
      .into_iter()
      .find(|window| window.reference == self.window.reference)
      .ok_or_else(|| not_found("held mouse target window"))?;
    if current.process_id != self.window.process_id {
      return Err(invalid_input("held mouse target owner changed"));
    }
    let code = match button {
      None => -1,
      Some(auv_driver_common::MouseButton::Left) => 0,
      Some(auv_driver_common::MouseButton::Right) => 1,
      Some(auv_driver_common::MouseButton::Middle) => 2,
    };
    crate::native::pointer::window_pointer_event(
      window_pid(&current)?,
      window_number(&current)?,
      point,
      Point::new(point.x - current.frame.origin.x, point.y - current.frame.origin.y),
      code,
      phase,
    )
    .map_err(backend)?;
    Ok(InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse))
  }
}
impl auv_driver_common::mouse_input::MouseBackend for WindowPointerBackend {
  fn target(&self) -> InputTarget {
    InputTarget::Window(self.window.clone())
  }
  fn move_to(&self, point: Point, held: Option<auv_driver_common::MouseButton>) -> DriverResult<InputActionResult> {
    self.post(point, held, 0)
  }
  fn button(&self, point: Point, button: auv_driver_common::MouseButton, down: bool) -> DriverResult<InputActionResult> {
    self.post(point, Some(button), if down { 1 } else { 2 })
  }
}
