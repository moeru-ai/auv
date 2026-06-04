use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use auv_driver::capture::{Activation, Capture, CaptureOptions, DisplayCapture, RegionCapture};
use auv_driver::display::{Display, ObservedDisplays};
use auv_driver::error::{DriverError, DriverResult};
use auv_driver::geometry::{
  CoordinateSpace, Point, RatioRect, Rect, ScreenPoint, Size, WindowPoint,
};
use auv_driver::input::{
  ActivationPolicy, Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt,
  InputDeliveryPath, InputPolicy, InputPreparationLease, PasteTextOptions, PrepareForInputOptions,
  Scroll, ScrollDeliveryCandidate, ScrollOptions, TextSubmit, TypeTextOptions, WaitOptions,
  WindowClickStrategy,
};
use auv_driver::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver::vision::{RecognizedText, TextRecognition, TextRecognitionOptions};
use auv_driver::window::{
  Window, WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind,
  WindowMutationOptions, WindowMutationPath, WindowMutationPolicy, WindowMutationResult,
  WindowMutationVerification, WindowRef, WindowState,
};
use image::RgbaImage;

use crate::driver::MacosDriverSession;
use crate::native::ocr::NativeOcrTextCapture;
use crate::native::types::{ObservedRect, ObservedWindow, ObservedWindowSnapshot};
use crate::native::window::ListWindowsOptions;
use crate::support::{build_window_candidates, parse_app_selector, resolve_app_ref};
use crate::types::{WindowRef as NativeWindowRef, WindowSelection};

#[derive(Clone, Debug, PartialEq)]
pub struct OcrMatch {
  pub text: String,
  pub confidence: f64,
  pub bounds: Rect,
}

impl OcrMatch {
  pub fn action_point(&self) -> Point {
    self.bounds.center()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OcrMatches {
  pub matches: Vec<OcrMatch>,
}

impl OcrMatches {
  pub fn best_match(&self) -> Option<&OcrMatch> {
    self.matches.first()
  }
}

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
}

impl DisplayApi<'_> {
  pub fn list(&self) -> DriverResult<ObservedDisplays> {
    let _ = self.session;
    Ok(ObservedDisplays {
      displays: list_display_targets()?
        .into_iter()
        .map(|target| target.display)
        .collect(),
    })
  }

  pub fn capture(&self, options: CaptureOptions) -> DriverResult<DisplayCapture> {
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
    capture_display_xcap(options.display.as_deref())
  }

  pub fn capture_region(&self, options: CaptureOptions) -> DriverResult<RegionCapture> {
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
    capture_region_xcap(options.display.as_deref(), region)
  }
}

impl WindowApi<'_> {
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    let _ = self.session;
    let snapshot =
      crate::native::window::list_windows(ListWindowsOptions::all_visible(256)).map_err(backend)?;
    Ok(
      snapshot
        .windows
        .iter()
        .map(|window| window_from_observed(window, None))
        .collect(),
    )
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    let mut snapshot =
      crate::native::window::list_windows(ListWindowsOptions::all_visible(256)).map_err(backend)?;
    if selector.app.as_ref().is_some_and(|app| app.frontmost) {
      let Some(window) = resolve_frontmost_window(&snapshot, &selector) else {
        return Err(not_found("frontmost window"));
      };
      return Ok(window);
    }

    if let Some(app) = &selector.app {
      if app.process_id.is_some() {
        return resolve_from_observed_windows(&snapshot, &selector);
      }
      let app_selector = app_selector_string(app).ok_or_else(|| {
        invalid_input("window selector app must use bundle, name, pid, or frontmost")
      })?;
      let parsed_app_selector = parse_app_selector(&app_selector).map_err(invalid_input)?;
      let resolved_app = match resolve_app_ref(&snapshot, &parsed_app_selector) {
        Ok(resolved_app) => resolved_app,
        Err(_) => {
          // NOTICE: The unfiltered WindowServer snapshot can omit windows that
          // an app-filtered query returns. Retry with the explicit app selector
          // before reporting target_window_not_found.
          let filtered_snapshot =
            crate::native::window::list_windows(ListWindowsOptions::app(256, &app_selector))
              .map_err(backend)?;
          let resolved_app =
            resolve_app_ref(&filtered_snapshot, &parsed_app_selector).map_err(backend)?;
          snapshot = filtered_snapshot;
          resolved_app
        }
      };
      let displays = Vec::new();
      let candidate = if selector.main_visible && selector.title.is_none() {
        let candidates = build_window_candidates(&snapshot, &resolved_app, &displays)
          .map_err(backend)?
          .into_iter()
          .collect::<Vec<_>>();
        candidates
          .into_iter()
          .find(|candidate| candidate.is_main_candidate)
      } else {
        let selection = window_selection_from_selector(&selector);
        crate::support::resolve_window_candidate(&snapshot, &resolved_app, &displays, &selection)
          .ok()
      };
      let Some(candidate) = candidate else {
        return resolve_from_observed_windows(&snapshot, &selector);
      };
      return Ok(window_from_native_ref(
        &candidate.window_ref,
        Some(candidate.is_main_candidate),
      ));
    }

    resolve_from_observed_windows(&snapshot, &selector)
  }

  pub fn capture(&self, window: &Window) -> DriverResult<Capture> {
    self.capture_with(window, CaptureOptions::default())
  }

  pub fn capture_with(&self, window: &Window, options: CaptureOptions) -> DriverResult<Capture> {
    if options.display.is_some() || options.region.is_some() || options.window.is_some() {
      return Err(invalid_input(
        "window.capture_with does not accept display, region, or nested window capture options",
      ));
    }
    if let Activation::ActivateFirst { settle } = options.activation {
      activate_app_for_window(window)?;
      thread::sleep(settle);
    }
    capture_window(window)
  }

  pub fn find_text(
    &self,
    window: &Window,
    query: &str,
    region: RatioRect,
    wait: WaitOptions,
  ) -> DriverResult<OcrMatches> {
    let started = std::time::Instant::now();
    loop {
      let capture = self.capture(window)?;
      let matches = self
        .session
        .vision()
        .find_text_in_capture(&capture, query, region)?;
      if !matches.matches.is_empty() || started.elapsed() >= wait.timeout {
        return Ok(matches);
      }
      thread::sleep(wait.poll_interval);
    }
  }

  pub fn wait_text(
    &self,
    window: &Window,
    query: &str,
    region: RatioRect,
    wait: WaitOptions,
  ) -> DriverResult<OcrMatches> {
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

  pub fn move_to(
    &self,
    window: &Window,
    point: Point,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::MoveTo { point }, options)
  }

  pub fn resize(
    &self,
    window: &Window,
    size: Size,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::Resize { size }, options)
  }

  pub fn set_frame(
    &self,
    window: &Window,
    frame: Rect,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::SetFrame { frame }, options)
  }

  pub fn minimize(
    &self,
    window: &Window,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::Minimize, options)
  }

  pub fn restore(
    &self,
    window: &Window,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::Restore, options)
  }

  pub fn zoom(
    &self,
    window: &Window,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    self.mutate(window, WindowMutationKind::Zoom, options)
  }

  pub fn click(
    &self,
    window: &Window,
    point: WindowPoint,
    options: ClickOptions,
  ) -> DriverResult<InputActionResult> {
    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let screen_point = self.to_screen_point(window, point)?;
    let screen = screen_point.point();
    let window_point = point.point();
    let (click_count, click_interval_ms) = click_parts(&options.click)?;
    let window_strategy_code = match options.window_strategy {
      WindowClickStrategy::ChromiumCompatible => 0,
      WindowClickStrategy::PidTargeted => 1,
    };
    let background_result = crate::native::input::click_window_point(
      pid,
      number,
      screen.x,
      screen.y,
      window_point.x,
      window_point.y,
      0,
      click_count,
      click_interval_ms,
      window_strategy_code,
    )
    .map_err(backend);

    match background_result {
      Ok(()) => Ok(InputActionResult::single_success(
        InputDeliveryPath::WindowTargetedMouse,
      )),
      Err(background_error) => match options.policy {
        InputPolicy::BackgroundOnly | InputPolicy::BackgroundPreferred => Err(background_error),
        InputPolicy::ForegroundPreferred => {
          let fallback_reason = background_error.to_string();
          let lease = self.prepare_for_input(window, foreground_prepare_options(Duration::ZERO))?;
          let action_result = self
            .session
            .input()
            .click_at(screen_point.point(), options.click.clone());
          let restore_result = self.restore_input(lease);
          action_result?;
          restore_result?;
          Ok(InputActionResult {
            selected_path: InputDeliveryPath::ForegroundSystemEvents,
            attempts: vec![
              InputAttempt::failure(
                InputDeliveryPath::WindowTargetedMouse,
                fallback_reason.clone(),
              ),
              InputAttempt::success(InputDeliveryPath::ForegroundSystemEvents),
            ],
            fallback_reason: Some(fallback_reason),
            mouse_disturbance: DisturbanceLevel::Temporary,
            focus_disturbance: DisturbanceLevel::Foreground,
            clipboard_disturbance: DisturbanceLevel::None,
          })
        }
      },
    }
  }

  pub fn type_text(
    &self,
    window: &Window,
    text: &str,
    options: TypeTextOptions,
  ) -> DriverResult<InputActionResult> {
    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let background_result = type_text_in_window(pid, number, text, options);

    match background_result {
      Ok(()) => Ok(InputActionResult::single_success(
        InputDeliveryPath::WindowTargetedKeyboard,
      )),
      Err(background_error @ DriverError::InvalidInput { .. }) => Err(background_error),
      Err(background_error) => match options.policy {
        // Task 4 intentionally keeps BackgroundPreferred background-only during no-steal rollout.
        InputPolicy::BackgroundOnly | InputPolicy::BackgroundPreferred => Err(background_error),
        InputPolicy::ForegroundPreferred if options.allow_clipboard_fallback => {
          let fallback_reason = background_error.to_string();
          let lease = self.prepare_for_input(window, foreground_prepare_options(Duration::ZERO))?;
          let action_result = self.session.input().paste_text(PasteTextOptions {
            text: text.to_string(),
            replace_existing: options.replace_existing,
            submit: options.submit,
            settle: options.settle,
          });
          let restore_result = self.restore_input(lease);
          action_result?;
          restore_result?;
          Ok(InputActionResult {
            selected_path: InputDeliveryPath::ClipboardPaste,
            attempts: vec![
              InputAttempt::failure(
                InputDeliveryPath::WindowTargetedKeyboard,
                fallback_reason.clone(),
              ),
              InputAttempt::success(InputDeliveryPath::ClipboardPaste),
            ],
            fallback_reason: Some(fallback_reason),
            mouse_disturbance: DisturbanceLevel::None,
            focus_disturbance: DisturbanceLevel::Foreground,
            clipboard_disturbance: DisturbanceLevel::Temporary,
          })
        }
        InputPolicy::ForegroundPreferred => Err(background_error),
      },
    }
  }

  pub fn scroll(
    &self,
    window: &Window,
    point: WindowPoint,
    scroll: Scroll,
    options: ScrollOptions,
  ) -> DriverResult<InputActionResult> {
    let mut attempts = Vec::new();
    let mut fallback_reason = None;
    for candidate in scroll_attempt_candidates(&options) {
      match candidate {
        ScrollDeliveryCandidate::AxScroll => {
          // TODO(background-scroll-ax): AX scrollbar/value scrolling is deferred
          // until this policy slice has verification evidence for the
          // window-targeted wheel path; implement when owner approves AX scroll
          // mutation against captured AX tree state.
          let message = "AX scroll is not implemented in this slice";
          attempts.push(InputAttempt::failure(InputDeliveryPath::AxScroll, message));
          fallback_reason.get_or_insert_with(|| message.to_string());
        }
        ScrollDeliveryCandidate::WindowTargetedWheel => {
          match self.scroll_window_targeted_wheel(window, point, scroll, options.settle) {
            Ok(()) => {
              attempts.push(InputAttempt::success(
                InputDeliveryPath::WindowTargetedWheel,
              ));
              return Ok(InputActionResult {
                selected_path: InputDeliveryPath::WindowTargetedWheel,
                attempts,
                fallback_reason,
                mouse_disturbance: DisturbanceLevel::None,
                focus_disturbance: DisturbanceLevel::None,
                clipboard_disturbance: DisturbanceLevel::None,
              });
            }
            Err(error) => {
              let message = error.to_string();
              attempts.push(InputAttempt::failure(
                InputDeliveryPath::WindowTargetedWheel,
                message.clone(),
              ));
              fallback_reason.get_or_insert(message);
            }
          }
        }
        ScrollDeliveryCandidate::WindowTargetedKeyboardScroll => {
          // TODO(background-scroll-keyboard): Keyboard scroll needs target state
          // and reliability rules before it can be enabled; add only after
          // owner-approved verification for focus/element anchoring.
          let message = "window-targeted keyboard scroll is reserved but disabled";
          attempts.push(InputAttempt::failure(
            InputDeliveryPath::WindowTargetedKeyboardScroll,
            message,
          ));
          fallback_reason.get_or_insert_with(|| message.to_string());
        }
        ScrollDeliveryCandidate::ForegroundHid => {
          if options.policy == InputPolicy::BackgroundOnly {
            continue;
          }
          let screen_point = self.to_screen_point(window, point)?;
          let result =
            self
              .session
              .input()
              .scroll_global_hid(screen_point.point(), scroll, options.settle)?;
          attempts.extend(result.attempts);
          return Ok(InputActionResult {
            selected_path: result.selected_path,
            attempts,
            fallback_reason,
            mouse_disturbance: result.mouse_disturbance,
            focus_disturbance: result.focus_disturbance,
            clipboard_disturbance: result.clipboard_disturbance,
          });
        }
      }
    }

    Err(DriverError::unsupported("background_scroll"))
  }

  fn mutate(
    &self,
    window: &Window,
    kind: WindowMutationKind,
    options: WindowMutationOptions,
  ) -> DriverResult<WindowMutationResult> {
    let _ = self.session;
    validate_window_mutation_kind(kind)?;
    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let title = window.title.clone().unwrap_or_default();
    let mut attempts = Vec::new();
    let mut fallback_reason = None;

    for candidate in window_mutation_candidates(&options) {
      let path = window_mutation_path(candidate);
      if candidate == WindowMutationCandidate::ForegroundSystemEvents {
        // TODO(window-management-foreground-fallback): pointer/system-events
        // fallback is deferred because this slice wires only the native AX
        // bridge; enable when the owner approves foreground repositioning.
        let message = "foreground window mutation fallback is not implemented in this slice";
        attempts.push(WindowMutationAttempt::failure(path, message));
        fallback_reason.get_or_insert_with(|| message.to_string());
        continue;
      }

      if !candidate_supports_window_mutation(candidate, kind) {
        let message = format!(
          "{} does not support {}",
          window_mutation_candidate_name(candidate),
          window_mutation_kind_name(kind)
        );
        attempts.push(WindowMutationAttempt::failure(path, message.clone()));
        fallback_reason.get_or_insert(message);
        continue;
      }

      let request = decoded_window_mutation_request(pid, number, title.clone(), kind)?;
      match crate::native::window::mutate_window(request).map_err(backend) {
        Ok(response) => {
          attempts.push(WindowMutationAttempt::success(
            path,
            format!("{} via {}", response.performed_action, response.path),
          ));
          if !options.settle.is_zero() {
            thread::sleep(options.settle);
          }
          let result = window_mutation_result(path, attempts, fallback_reason, response);
          verify_window_mutation(kind, &options.verification, &result)?;
          return Ok(result);
        }
        Err(error) => {
          let message = error.to_string();
          attempts.push(WindowMutationAttempt::failure(path, message.clone()));
          fallback_reason.get_or_insert(message);
          if options.policy == WindowMutationPolicy::NativeOnly {
            break;
          }
        }
      }
    }

    Err(window_mutation_failure(attempts, fallback_reason))
  }

  pub fn prepare_for_input(
    &self,
    window: &Window,
    options: PrepareForInputOptions,
  ) -> DriverResult<InputPreparationLease> {
    let _ = self.session;
    if options.install_focus_guard {
      return Err(DriverError::unsupported("focus_guard"));
    }
    match options.activation {
      ActivationPolicy::NoChange | ActivationPolicy::Background => {
        if !options.settle.is_zero() {
          thread::sleep(options.settle);
        }
        Ok(InputPreparationLease::noop())
      }
      ActivationPolicy::FocusWithoutRaise => Err(DriverError::unsupported("focus_without_raise")),
      ActivationPolicy::Foreground { settle } => {
        if options.preserve_frontmost {
          return Err(DriverError::unsupported("foreground_restore"));
        }
        activate_app_for_window(window)?;
        if !settle.is_zero() {
          thread::sleep(settle);
        }
        if !options.settle.is_zero() {
          thread::sleep(options.settle);
        }
        Ok(InputPreparationLease::noop())
      }
    }
  }

  pub fn restore_input(&self, mut lease: InputPreparationLease) -> DriverResult<()> {
    let _ = self.session;
    lease.mark_restored();
    Ok(())
  }

  fn scroll_window_targeted_wheel(
    &self,
    window: &Window,
    point: WindowPoint,
    scroll: Scroll,
    settle: Duration,
  ) -> DriverResult<()> {
    let pid = window_pid(window)?;
    let number = window_number(window)?;
    let screen_point = self.to_screen_point(window, point)?;
    let screen = screen_point.point();
    let window_point = point.point();
    crate::native::input::scroll_window_point(
      pid,
      number,
      screen.x,
      screen.y,
      window_point.x,
      window_point.y,
      scroll.delta_x,
      scroll.delta_y,
    )
    .map_err(backend)?;
    if !settle.is_zero() {
      thread::sleep(settle);
    }
    Ok(())
  }
}

impl InputApi<'_> {
  pub fn click_at(&self, point: Point, click: Click) -> DriverResult<()> {
    let _ = self.session;
    let (count, interval) = match click {
      Click::Single => (1, 0),
      Click::Double { interval } => (2, duration_millis(interval)?),
    };
    crate::native::pointer::click_point(point.x, point.y, 0, count, interval).map_err(backend)
  }

  pub fn scroll_global_hid(
    &self,
    point: Point,
    scroll: Scroll,
    settle: Duration,
  ) -> DriverResult<InputActionResult> {
    let _ = self.session;
    crate::native::pointer::scroll_point(point.x, point.y, scroll.delta_x, scroll.delta_y)
      .map_err(backend)?;
    if !settle.is_zero() {
      thread::sleep(settle);
    }
    Ok(InputActionResult {
      selected_path: InputDeliveryPath::ForegroundSystemEvents,
      attempts: vec![InputAttempt::success(
        InputDeliveryPath::ForegroundSystemEvents,
      )],
      fallback_reason: None,
      mouse_disturbance: DisturbanceLevel::Temporary,
      focus_disturbance: DisturbanceLevel::Unknown,
      clipboard_disturbance: DisturbanceLevel::None,
    })
  }

  pub fn copy(&self) -> DriverResult<()> {
    let _ = self.session;
    run_osascript(&["tell application \"System Events\" to keystroke \"c\" using command down"])
  }

  pub fn paste(&self) -> DriverResult<()> {
    let _ = self.session;
    run_osascript(&["tell application \"System Events\" to keystroke \"v\" using command down"])
  }

  pub fn paste_text(&self, options: PasteTextOptions) -> DriverResult<()> {
    let _ = self.session;
    let _lock = acquire_clipboard_lock(Duration::from_millis(5_000))?;
    let snapshot = crate::native::clipboard::capture_clipboard_snapshot().map_err(backend)?;
    let result = (|| {
      let submit_key_code = text_submit_key_code(options.submit)?;
      crate::native::clipboard::set_clipboard_text(&options.text).map_err(backend)?;

      let mut lines = vec!["tell application \"System Events\"".to_string()];
      if options.replace_existing {
        lines.push("keystroke \"a\" using {command down}".to_string());
        lines.push("delay 0.05".to_string());
        lines.push("key code 51".to_string());
        lines.push("delay 0.05".to_string());
      }
      lines.push("keystroke \"v\" using {command down}".to_string());
      lines.push("delay 0.15".to_string());
      if let Some(key_code) = submit_key_code {
        lines.push("delay 0.05".to_string());
        lines.push(format!("key code {key_code}"));
      }
      lines.push("end tell".to_string());
      run_osascript_lines(&lines)?;
      if !options.settle.is_zero() {
        thread::sleep(options.settle);
      }
      Ok(())
    })();
    let restore_result =
      crate::native::clipboard::restore_clipboard_snapshot(&snapshot).map_err(backend);
    match (result, restore_result) {
      (Ok(()), Ok(())) => Ok(()),
      (Err(action_error), Ok(())) => Err(action_error),
      (Ok(()), Err(restore_error)) => Err(backend(format!(
        "pasted text but failed to restore clipboard: {restore_error}"
      ))),
      (Err(action_error), Err(restore_error)) => Err(backend(format!(
        "{action_error}; additionally failed to restore clipboard: {restore_error}"
      ))),
    }
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
    let crop = ratio_rect_to_observed(capture, region);
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
    let recognition = self.recognize_text_in_capture_with_options(capture, region, options)?;
    Ok(ocr_matches_from_recognition(&recognition, query))
  }
}

fn resolve_from_observed_windows(
  snapshot: &ObservedWindowSnapshot,
  selector: &WindowSelector,
) -> DriverResult<Window> {
  let mut candidates = snapshot
    .windows
    .iter()
    .map(|window| window_from_observed(window, None))
    .filter(|window| matches_window_selector_except_main_visible(window, selector))
    .collect::<Vec<_>>();
  if selector.main_visible {
    candidates.sort_by_key(|window| {
      std::cmp::Reverse((
        window.is_main,
        window
          .title
          .as_ref()
          .is_some_and(|title| !title.trim().is_empty()),
        (window.frame.size.width * window.frame.size.height).round() as i64,
      ))
    });
    if let Some(window) = candidates.first() {
      return Ok(window.clone());
    }
    return Err(not_found("main visible window"));
  }
  match candidates.as_slice() {
    [window] => Ok(window.clone()),
    [] => Err(not_found("window selector")),
    _ => Err(invalid_input(format!(
      "window selector was ambiguous: {} windows matched",
      candidates.len()
    ))),
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
  if !window.is_visible {
    return false;
  }
  if let Some(app) = &selector.app
    && !matches_app_selector(window, app)
  {
    return false;
  }
  if let Some(title) = &selector.title {
    let Some(window_title) = &window.title else {
      return false;
    };
    return matches_text(window_title, title);
  }
  true
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
  if let Some(bundle) = &selector.bundle {
    let Some(app_bundle_id) = &window.app_bundle_id else {
      return false;
    };
    if !matches_text(app_bundle_id, bundle) {
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
  Window {
    reference: WindowRef {
      id: window.window_number.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    app_bundle_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: rect_from_observed(&window.bounds),
    coordinate_space: CoordinateSpace::Screen,
    is_main: is_main.unwrap_or(false),
    is_visible: window.bounds.width > 0 && window.bounds.height > 0,
  }
}

fn window_from_native_ref(window: &NativeWindowRef, is_main: Option<bool>) -> Window {
  Window {
    reference: WindowRef {
      id: window.window_number.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    app_bundle_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: rect_from_observed(&window.bounds),
    coordinate_space: CoordinateSpace::Screen,
    is_main: is_main.unwrap_or(true),
    is_visible: window.bounds.width > 0 && window.bounds.height > 0,
  }
}

fn resolve_frontmost_window(
  snapshot: &ObservedWindowSnapshot,
  selector: &WindowSelector,
) -> Option<Window> {
  snapshot
    .windows
    .iter()
    .filter(|window| {
      snapshot.frontmost_app_bundle_id.is_empty()
        || window
          .owner_bundle_id
          .eq_ignore_ascii_case(&snapshot.frontmost_app_bundle_id)
    })
    .filter(|window| {
      snapshot.frontmost_window_title.is_empty() || window.title == snapshot.frontmost_window_title
    })
    .map(|window| window_from_observed(window, Some(true)))
    .find(|window| matches_window_selector(window, selector))
}

fn app_selector_string(selector: &AppSelector) -> Option<String> {
  if let Some(bundle) = &selector.bundle {
    return matcher_value(bundle).map(ToOwned::to_owned);
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
    title: selector
      .title
      .as_ref()
      .and_then(matcher_value)
      .map(str::to_string),
  }
}

fn rect_from_observed(rect: &ObservedRect) -> Rect {
  Rect::new(
    rect.x as f64,
    rect.y as f64,
    rect.width as f64,
    rect.height as f64,
  )
}

fn screen_point_for_window_point(window: &Window, point: WindowPoint) -> ScreenPoint {
  let point = point.point();
  ScreenPoint::new(
    window.frame.origin.x + point.x,
    window.frame.origin.y + point.y,
  )
}

fn window_point_for_screen_point(window: &Window, point: ScreenPoint) -> WindowPoint {
  let point = point.point();
  WindowPoint::new(
    point.x - window.frame.origin.x,
    point.y - window.frame.origin.y,
  )
}

fn window_number(window: &Window) -> DriverResult<i64> {
  if window.reference.id.trim().is_empty() {
    return Err(invalid_input("window is missing a native macOS window id"));
  }
  window.reference.id.parse::<i64>().map_err(|error| {
    invalid_input(format!(
      "window ref {} was not a native macOS window id: {error}",
      window.reference.id
    ))
  })
}

fn window_pid(window: &Window) -> DriverResult<i64> {
  window
    .process_id
    .map(i64::from)
    .ok_or_else(|| invalid_input("window is missing an owner process id"))
}

fn click_parts(click: &Click) -> DriverResult<(i64, u64)> {
  match click {
    Click::Single => Ok((1, 0)),
    Click::Double { interval } => Ok((2, duration_millis(*interval)?)),
  }
}

fn type_text_in_window(
  pid: i64,
  number: i64,
  text: &str,
  options: TypeTextOptions,
) -> DriverResult<()> {
  let (submit_key_code, inter_char_delay_ms) = type_text_parts(options)?;
  if options.replace_existing {
    crate::native::input::hotkey_in_window(pid, number, 0, true, false, false, false)
      .map_err(backend)?;
    crate::native::input::press_key_in_window(pid, number, 51).map_err(backend)?;
    thread::sleep(Duration::from_millis(100));
  }
  crate::native::input::type_text_in_window(pid, number, text.to_string(), inter_char_delay_ms)
    .map_err(backend)?;
  if let Some(key_code) = submit_key_code {
    crate::native::input::press_key_in_window(pid, number, key_code).map_err(backend)?;
  }
  if !options.settle.is_zero() {
    thread::sleep(options.settle);
  }
  Ok(())
}

fn type_text_parts(options: TypeTextOptions) -> DriverResult<(Option<i32>, u64)> {
  let submit_key_code = text_submit_key_code(options.submit)?;
  let inter_char_delay_ms = duration_millis(options.inter_char_delay)?;
  Ok((submit_key_code, inter_char_delay_ms))
}

fn scroll_attempt_candidates(options: &ScrollOptions) -> Vec<ScrollDeliveryCandidate> {
  match options.policy {
    InputPolicy::ForegroundPreferred => vec![ScrollDeliveryCandidate::ForegroundHid],
    InputPolicy::BackgroundOnly => options
      .delivery_strategy
      .candidates
      .iter()
      .copied()
      .filter(|candidate| *candidate != ScrollDeliveryCandidate::ForegroundHid)
      .collect(),
    InputPolicy::BackgroundPreferred => options.delivery_strategy.candidates.clone(),
  }
}

fn window_mutation_candidates(options: &WindowMutationOptions) -> Vec<WindowMutationCandidate> {
  match options.policy {
    WindowMutationPolicy::ForegroundPreferred => {
      vec![WindowMutationCandidate::ForegroundSystemEvents]
    }
    WindowMutationPolicy::NativeOnly | WindowMutationPolicy::NativePreferred => {
      options.strategy.candidates.iter().copied().collect()
    }
  }
}

fn candidate_supports_window_mutation(
  candidate: WindowMutationCandidate,
  kind: WindowMutationKind,
) -> bool {
  matches!(
    (candidate, kind),
    (
      WindowMutationCandidate::AxWindowAttribute | WindowMutationCandidate::PlatformNative,
      WindowMutationKind::MoveTo { .. }
        | WindowMutationKind::Resize { .. }
        | WindowMutationKind::SetFrame { .. }
    ) | (
      WindowMutationCandidate::AxWindowAction | WindowMutationCandidate::PlatformNative,
      WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom
    )
  )
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
    WindowMutationKind::MoveTo { .. } => "move_to",
    WindowMutationKind::Resize { .. } => "resize",
    WindowMutationKind::SetFrame { .. } => "set_frame",
    WindowMutationKind::Minimize => "minimize",
    WindowMutationKind::Restore => "restore",
    WindowMutationKind::Zoom => "zoom",
  }
}

fn decoded_window_mutation_request(
  pid: i64,
  window_number: i64,
  title: String,
  kind: WindowMutationKind,
) -> DriverResult<crate::native::window::DecodedWindowMutationRequest> {
  let (native_kind, x, y, width, height) = match kind {
    WindowMutationKind::MoveTo { point } => (
      crate::native::window::DecodedWindowMutationKind::MoveTo,
      rounded_i64(point.x, "point.x")?,
      rounded_i64(point.y, "point.y")?,
      0,
      0,
    ),
    WindowMutationKind::Resize { size } => (
      crate::native::window::DecodedWindowMutationKind::Resize,
      0,
      0,
      rounded_positive_i64(size.width, "size.width")?,
      rounded_positive_i64(size.height, "size.height")?,
    ),
    WindowMutationKind::SetFrame { frame } => (
      crate::native::window::DecodedWindowMutationKind::SetFrame,
      rounded_i64(frame.origin.x, "frame.origin.x")?,
      rounded_i64(frame.origin.y, "frame.origin.y")?,
      rounded_positive_i64(frame.size.width, "frame.size.width")?,
      rounded_positive_i64(frame.size.height, "frame.size.height")?,
    ),
    WindowMutationKind::Minimize => (
      crate::native::window::DecodedWindowMutationKind::Minimize,
      0,
      0,
      0,
      0,
    ),
    WindowMutationKind::Restore => (
      crate::native::window::DecodedWindowMutationKind::Restore,
      0,
      0,
      0,
      0,
    ),
    WindowMutationKind::Zoom => (
      crate::native::window::DecodedWindowMutationKind::Zoom,
      0,
      0,
      0,
      0,
    ),
  };

  Ok(crate::native::window::DecodedWindowMutationRequest {
    pid,
    window_number,
    title,
    kind: native_kind,
    x,
    y,
    width,
    height,
  })
}

fn validate_window_mutation_kind(kind: WindowMutationKind) -> DriverResult<()> {
  match kind {
    WindowMutationKind::MoveTo { point } => {
      let _ = rounded_i64(point.x, "point.x")?;
      let _ = rounded_i64(point.y, "point.y")?;
    }
    WindowMutationKind::Resize { size } => {
      let _ = rounded_positive_i64(size.width, "size.width")?;
      let _ = rounded_positive_i64(size.height, "size.height")?;
    }
    WindowMutationKind::SetFrame { frame } => {
      let _ = rounded_i64(frame.origin.x, "frame.origin.x")?;
      let _ = rounded_i64(frame.origin.y, "frame.origin.y")?;
      let _ = rounded_positive_i64(frame.size.width, "frame.size.width")?;
      let _ = rounded_positive_i64(frame.size.height, "frame.size.height")?;
    }
    WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom => {}
  }
  Ok(())
}

fn rounded_i64(value: f64, field: &str) -> DriverResult<i64> {
  if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
    return Err(invalid_input(format!(
      "{field} must be a finite i64-sized value"
    )));
  }
  Ok(value.round() as i64)
}

fn rounded_positive_i64(value: f64, field: &str) -> DriverResult<i64> {
  let rounded = rounded_i64(value, field)?;
  if rounded <= 0 {
    return Err(invalid_input(format!("{field} must be greater than zero")));
  }
  Ok(rounded)
}

fn window_mutation_result(
  selected_path: WindowMutationPath,
  attempts: Vec<WindowMutationAttempt>,
  fallback_reason: Option<String>,
  response: crate::native::window::DecodedWindowMutationResponse,
) -> WindowMutationResult {
  WindowMutationResult {
    selected_path,
    attempts,
    fallback_reason,
    before_frame: Some(Rect::new(
      response.before_x as f64,
      response.before_y as f64,
      response.before_width as f64,
      response.before_height as f64,
    )),
    after_frame: Some(Rect::new(
      response.after_x as f64,
      response.after_y as f64,
      response.after_width as f64,
      response.after_height as f64,
    )),
    before_state: Some(WindowState {
      is_minimized: Some(response.was_minimized),
      is_visible: Some(!response.was_minimized),
    }),
    after_state: Some(WindowState {
      is_minimized: Some(response.is_minimized),
      is_visible: Some(!response.is_minimized),
    }),
    focus_disturbance: DisturbanceLevel::None,
    mouse_disturbance: DisturbanceLevel::None,
  }
}

fn verify_window_mutation(
  kind: WindowMutationKind,
  verification: &WindowMutationVerification,
  result: &WindowMutationResult,
) -> DriverResult<()> {
  match verification {
    WindowMutationVerification::BestEffortState => verify_window_state(kind, result),
    WindowMutationVerification::FrameTolerance { points } => {
      verify_window_frame(kind, result, *points)?;
      verify_window_state(kind, result)
    }
  }
}

fn verify_window_frame(
  kind: WindowMutationKind,
  result: &WindowMutationResult,
  tolerance: f64,
) -> DriverResult<()> {
  if !tolerance.is_finite() || tolerance < 0.0 {
    return Err(invalid_input(
      "window mutation frame tolerance must be a finite non-negative value",
    ));
  }
  let Some(after_frame) = result.after_frame else {
    return Err(backend("window mutation did not report an after frame"));
  };
  match kind {
    WindowMutationKind::MoveTo { point } => {
      verify_close(after_frame.origin.x, point.x, tolerance, "frame.origin.x")?;
      verify_close(after_frame.origin.y, point.y, tolerance, "frame.origin.y")?;
    }
    WindowMutationKind::Resize { size } => {
      verify_close(
        after_frame.size.width,
        size.width,
        tolerance,
        "frame.size.width",
      )?;
      verify_close(
        after_frame.size.height,
        size.height,
        tolerance,
        "frame.size.height",
      )?;
    }
    WindowMutationKind::SetFrame { frame } => {
      verify_close(
        after_frame.origin.x,
        frame.origin.x,
        tolerance,
        "frame.origin.x",
      )?;
      verify_close(
        after_frame.origin.y,
        frame.origin.y,
        tolerance,
        "frame.origin.y",
      )?;
      verify_close(
        after_frame.size.width,
        frame.size.width,
        tolerance,
        "frame.size.width",
      )?;
      verify_close(
        after_frame.size.height,
        frame.size.height,
        tolerance,
        "frame.size.height",
      )?;
    }
    WindowMutationKind::Minimize | WindowMutationKind::Restore | WindowMutationKind::Zoom => {}
  }
  Ok(())
}

fn verify_window_state(
  kind: WindowMutationKind,
  result: &WindowMutationResult,
) -> DriverResult<()> {
  match kind {
    WindowMutationKind::Minimize => {
      let minimized = result
        .after_state
        .as_ref()
        .and_then(|state| state.is_minimized);
      if minimized != Some(true) {
        return Err(backend(
          "window mutation verification failed: window was not minimized",
        ));
      }
    }
    WindowMutationKind::Restore => {
      let minimized = result
        .after_state
        .as_ref()
        .and_then(|state| state.is_minimized);
      if minimized != Some(false) {
        return Err(backend(
          "window mutation verification failed: window was still minimized",
        ));
      }
    }
    WindowMutationKind::MoveTo { .. }
    | WindowMutationKind::Resize { .. }
    | WindowMutationKind::SetFrame { .. }
    | WindowMutationKind::Zoom => {}
  }
  Ok(())
}

fn verify_close(actual: f64, expected: f64, tolerance: f64, field: &str) -> DriverResult<()> {
  if (actual - expected).abs() <= tolerance {
    return Ok(());
  }
  Err(backend(format!(
    "window mutation verification failed: {field} expected {expected:.3} got {actual:.3} tolerance {tolerance:.3}"
  )))
}

fn window_mutation_failure(
  attempts: Vec<WindowMutationAttempt>,
  fallback_reason: Option<String>,
) -> DriverError {
  let mut parts = attempts
    .into_iter()
    .filter_map(|attempt| {
      attempt
        .message
        .map(|message| format!("{:?}: {message}", attempt.path))
    })
    .collect::<Vec<_>>();
  if parts.is_empty() {
    if let Some(reason) = fallback_reason {
      parts.push(reason);
    }
  }
  if parts.is_empty() {
    return DriverError::unsupported("window_mutation");
  }
  backend(format!(
    "window mutation failed after {} attempt(s): {}",
    parts.len(),
    parts.join("; ")
  ))
}

fn foreground_prepare_options(settle: Duration) -> PrepareForInputOptions {
  PrepareForInputOptions {
    activation: ActivationPolicy::Foreground { settle },
    preserve_frontmost: false,
    install_focus_guard: false,
    settle: Duration::ZERO,
  }
}

fn activate_app_for_window(window: &Window) -> DriverResult<()> {
  if let Some(bundle_id) = &window.app_bundle_id {
    run_osascript(&[&format!(
      "tell application id \"{}\" to activate",
      escape_applescript(bundle_id)
    )])
  } else if let Some(app_name) = &window.app_name {
    run_osascript(&[&format!(
      "tell application \"{}\" to activate",
      escape_applescript(app_name)
    )])
  } else {
    Ok(())
  }
}

#[derive(Clone, Debug)]
struct MacosDisplayTarget {
  index: usize,
  display: Display,
}

fn resolve_display_target(
  targets: &[MacosDisplayTarget],
  selector: Option<&str>,
) -> DriverResult<MacosDisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| {
        target.display.id == selector
          || target
            .display
            .name
            .as_deref()
            .is_some_and(|display_ref| display_ref == selector)
      })
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets
    .iter()
    .find(|target| target.display.is_primary)
    .or_else(|| targets.first())
    .cloned()
    .ok_or_else(|| not_found("primary display"))
}

#[cfg(target_os = "macos")]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  display_targets_from_monitors(&monitors)
}

#[cfg(not(target_os = "macos"))]
fn list_display_targets() -> DriverResult<Vec<MacosDisplayTarget>> {
  Err(DriverError::unsupported("display.list"))
}

#[cfg(target_os = "macos")]
fn capture_display_xcap(selector: Option<&str>) -> DriverResult<DisplayCapture> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_target(&targets, selector)?;
  let monitor = monitors
    .get(target.index)
    .ok_or_else(|| not_found(format!("display index {}", target.index)))?;
  let image = monitor
    .capture_image()
    .map_err(|error| backend(format!("failed to capture display: {error}")))?;
  let image = RgbaImage::from_raw(image.width(), image.height(), image.into_raw())
    .ok_or_else(|| backend("failed to decode captured display RGBA image"))?;
  let capture = Capture {
    image,
    bounds: target.display.frame,
    scale_factor: target.display.scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason: None,
  };
  Ok(DisplayCapture {
    display: target.display,
    capture,
  })
}

#[cfg(not(target_os = "macos"))]
fn capture_display_xcap(_selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(DriverError::unsupported("display.capture"))
}

#[cfg(target_os = "macos")]
fn capture_region_xcap(selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  let monitors = xcap::Monitor::all()
    .map_err(|error| backend(format!("failed to enumerate displays: {error}")))?;
  let targets = display_targets_from_monitors(&monitors)?;
  let target = resolve_display_for_global_region(&targets, selector, region)?;
  let monitor = monitors
    .get(target.index)
    .ok_or_else(|| not_found(format!("display index {}", target.index)))?;
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
fn display_targets_from_monitors(
  monitors: &[xcap::Monitor],
) -> DriverResult<Vec<MacosDisplayTarget>> {
  if monitors.is_empty() {
    return Err(not_found("display"));
  }
  monitors
    .iter()
    .enumerate()
    .map(|(index, monitor)| {
      let x = monitor
        .x()
        .map_err(|error| backend(format!("failed to read display x: {error}")))?
        as f64;
      let y = monitor
        .y()
        .map_err(|error| backend(format!("failed to read display y: {error}")))?
        as f64;
      let width = monitor
        .width()
        .map_err(|error| backend(format!("failed to read display width: {error}")))?
        as f64;
      let height = monitor
        .height()
        .map_err(|error| backend(format!("failed to read display height: {error}")))?
        as f64;
      let scale_factor = monitor
        .scale_factor()
        .map_err(|error| backend(format!("failed to read display scale: {error}")))?
        as f64;
      let is_builtin = monitor
        .is_builtin()
        .map_err(|error| backend(format!("failed to read display built-in flag: {error}")))?;
      let native_id = monitor
        .id()
        .map_err(|error| backend(format!("failed to read display id: {error}")))?
        .to_string();
      Ok(MacosDisplayTarget {
        index,
        display: Display {
          id: native_id,
          name: Some(format!("display_{index}")),
          frame: Rect::new(x, y, width, height),
          coordinate_space: CoordinateSpace::Screen,
          scale_factor,
          is_primary: monitor
            .is_primary()
            .map_err(|error| backend(format!("failed to read display primary flag: {error}")))?,
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
  selected
    .into_iter()
    .find(|target| rect_contains_rect(target.display.frame, region))
    .ok_or_else(|| not_found("display containing region"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

fn integral_capture_dimension(name: &str, value: f64) -> DriverResult<u32> {
  if value.fract() != 0.0 {
    return Err(invalid_input(format!(
      "region {name} must be an integer in backend capture units"
    )));
  }
  if value < 0.0 || value > u32::MAX as f64 {
    return Err(invalid_input(format!(
      "region {name} is outside the capture backend range"
    )));
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
fn capture_window(window: &Window) -> DriverResult<Capture> {
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
  match capture_window_swift(window) {
    Ok(capture) => Ok(capture),
    Err(swift_error) => {
      let fallback_reason = swift_error.to_string();
      capture_window_xcap(window, Some(fallback_reason.clone())).map_err(|xcap_error| {
        backend(format!(
          "native Swift window capture failed before xcap fallback: {fallback_reason}; xcap fallback also failed: {xcap_error}"
        ))
      })
    }
  }
}

#[cfg(target_os = "macos")]
fn capture_window_swift(window: &Window) -> DriverResult<Capture> {
  let native_window_id = window.reference.id.parse::<i64>().map_err(|error| {
    invalid_input(format!(
      "window ref {} was not a native macOS window id: {error}",
      window.reference.id
    ))
  })?;
  let capture = crate::native::capture::capture_window_rgba(native_window_id).map_err(backend)?;
  let width = u32::try_from(capture.image_width)
    .map_err(|error| backend(format!("native capture returned invalid width: {error}")))?;
  let height = u32::try_from(capture.image_height)
    .map_err(|error| backend(format!("native capture returned invalid height: {error}")))?;
  let image = RgbaImage::from_raw(width, height, capture.rgba_bytes)
    .ok_or_else(|| backend("failed to decode native captured window RGBA image"))?;
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };
  Ok(Capture {
    image,
    bounds: window.frame,
    scale_factor,
    backend: "macos.screencapturekit.ffi".to_string(),
    fallback_reason: None,
  })
}

#[cfg(target_os = "macos")]
fn capture_window_xcap(window: &Window, fallback_reason: Option<String>) -> DriverResult<Capture> {
  let native_window_id = window.reference.id.parse::<u32>().map_err(|error| {
    invalid_input(format!(
      "window ref {} was not a native macOS window id: {error}",
      window.reference.id
    ))
  })?;
  let windows = xcap::Window::all()
    .map_err(|error| backend(format!("failed to enumerate windows: {error}")))?;
  let xcap_window = windows
    .iter()
    .find(|candidate| candidate.id().is_ok_and(|id| id == native_window_id))
    .ok_or_else(|| not_found(format!("native window {}", window.reference.id)))?;
  let image = xcap_window.capture_image().map_err(|error| {
    backend(format!(
      "failed to capture window {}: {error}",
      window.reference.id
    ))
  })?;
  let width = image.width();
  let height = image.height();
  let scale_factor = if window.frame.size.width > 0.0 {
    f64::from(width) / window.frame.size.width
  } else {
    1.0
  };
  let image = RgbaImage::from_raw(width, height, image.into_raw())
    .ok_or_else(|| backend("failed to decode captured window RGBA image"))?;
  Ok(Capture {
    image,
    bounds: window.frame,
    scale_factor,
    backend: "xcap.macos".to_string(),
    fallback_reason,
  })
}

#[cfg(not(target_os = "macos"))]
fn capture_window(_window: &Window) -> DriverResult<Capture> {
  Err(DriverError::unsupported("capture_window"))
}

fn ratio_rect_to_observed(capture: &Capture, region: RatioRect) -> ObservedRect {
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

fn text_recognition_from_native(
  native: &NativeOcrTextCapture,
  capture: &Capture,
) -> TextRecognition {
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
  let text = matches
    .iter()
    .map(|recognized| recognized.text.as_str())
    .collect::<Vec<_>>()
    .join("\n");
  TextRecognition {
    text,
    regions: matches,
  }
}

fn duration_millis(duration: Duration) -> DriverResult<u64> {
  u64::try_from(duration.as_millis())
    .map_err(|error| invalid_input(format!("duration too large: {error}")))
}

fn run_osascript(scripts: &[&str]) -> DriverResult<()> {
  let mut command = Command::new("osascript");
  for script in scripts {
    command.arg("-e").arg(script);
  }
  let output = command
    .output()
    .map_err(|error| backend(format!("failed to run osascript: {error}")))?;
  if output.status.success() {
    Ok(())
  } else {
    Err(backend(String::from_utf8_lossy(&output.stderr).trim()))
  }
}

fn run_osascript_lines(lines: &[String]) -> DriverResult<()> {
  let mut command = Command::new("osascript");
  for line in lines {
    command.arg("-e").arg(line);
  }
  let output = command
    .output()
    .map_err(|error| backend(format!("failed to run osascript: {error}")))?;
  if output.status.success() {
    Ok(())
  } else {
    Err(backend(String::from_utf8_lossy(&output.stderr).trim()))
  }
}

fn text_submit_key_code(submit: TextSubmit) -> DriverResult<Option<i32>> {
  match submit {
    TextSubmit::No => Ok(None),
    TextSubmit::Return => Ok(Some(36)),
    TextSubmit::Search | TextSubmit::Done | TextSubmit::Go => Err(invalid_input(format!(
      "text submit {submit:?} is not supported by the macOS desktop driver yet"
    ))),
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
    match std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&path)
    {
      Ok(mut file) => {
        let _ = writeln!(file, "pid={}", std::process::id());
        return Ok(ClipboardLock { path });
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => {
        clear_stale_clipboard_lock(&path)?;
        if started.elapsed() >= timeout {
          return Err(backend(format!(
            "timed out waiting for macOS clipboard lock at {}",
            path.display()
          )));
        }
        thread::sleep(Duration::from_millis(50));
      }
      Err(error) => {
        return Err(backend(format!(
          "failed to acquire macOS clipboard lock {}: {error}",
          path.display()
        )));
      }
    }
  }
}

fn clear_stale_clipboard_lock(path: &PathBuf) -> DriverResult<()> {
  let Ok(content) = std::fs::read_to_string(path) else {
    return Ok(());
  };
  let Some(pid) = content
    .lines()
    .find_map(|line| line.strip_prefix("pid="))
    .and_then(|raw| raw.trim().parse::<u32>().ok())
  else {
    return Ok(());
  };
  if process_is_alive(pid) {
    return Ok(());
  }
  std::fs::remove_file(path).map_err(|error| {
    backend(format!(
      "failed to remove stale clipboard lock {}: {error}",
      path.display()
    ))
  })
}

fn process_is_alive(pid: u32) -> bool {
  if pid == 0 {
    return false;
  }
  Command::new("/bin/kill")
    .args(["-0", &pid.to_string()])
    .status()
    .is_ok_and(|status| status.success())
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

#[cfg(test)]
mod no_steal_tests {
  use auv_driver::geometry::{ScreenPoint, WindowPoint};

  use super::*;

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
  fn window_number_parses_native_window_id() {
    let window = sample_window();

    let number = window_number(&window).expect("window number");

    assert_eq!(number, 42);
  }

  #[test]
  fn window_number_rejects_missing_or_invalid_native_window_id() {
    let mut missing = sample_window();
    missing.reference.id.clear();
    let mut invalid = sample_window();
    invalid.reference.id = "not-a-window-number".to_string();

    assert!(matches!(
      window_number(&missing),
      Err(DriverError::InvalidInput { .. })
    ));
    assert!(matches!(
      window_number(&invalid),
      Err(DriverError::InvalidInput { .. })
    ));
  }

  #[test]
  fn window_pid_requires_owner_process_id() {
    let mut window = sample_window();
    window.process_id = None;

    assert!(matches!(
      window_pid(&window),
      Err(DriverError::InvalidInput { .. })
    ));
  }

  #[test]
  fn click_parts_converts_click_count_and_interval() {
    assert_eq!(click_parts(&Click::Single).expect("single click"), (1, 0));
    assert_eq!(
      click_parts(&Click::Double {
        interval: Duration::from_millis(75),
      })
      .expect("double click"),
      (2, 75)
    );
  }

  #[test]
  fn type_text_parts_validate_submit_and_delay_without_delivery() {
    let parts = type_text_parts(TypeTextOptions {
      submit: TextSubmit::Return,
      inter_char_delay: Duration::from_millis(12),
      ..TypeTextOptions::default()
    })
    .expect("type text parts");

    assert_eq!(parts, (Some(36), 12));

    assert!(matches!(
      type_text_parts(TypeTextOptions {
        submit: TextSubmit::Search,
        ..TypeTextOptions::default()
      }),
      Err(DriverError::InvalidInput { .. })
    ));
    assert!(matches!(
      type_text_parts(TypeTextOptions {
        inter_char_delay: Duration::MAX,
        ..TypeTextOptions::default()
      }),
      Err(DriverError::InvalidInput { .. })
    ));
  }

  #[test]
  fn input_api_exposes_explicit_global_hid_scroll_method() {
    if false {
      let session = MacosDriverSession { _private: () };
      let _ = session.input().scroll_global_hid(
        Point::new(20.0, 30.0),
        Scroll::new(0.0, -120.0),
        Duration::ZERO,
      );
    }
  }

  #[test]
  fn scroll_attempt_candidates_background_preferred_keep_background_before_foreground() {
    let candidates = scroll_attempt_candidates(&ScrollOptions::default());

    assert_eq!(
      candidates,
      vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ]
    );
  }

  #[test]
  fn scroll_attempt_candidates_foreground_preferred_uses_foreground_hid_first() {
    let candidates = scroll_attempt_candidates(&ScrollOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ScrollOptions::default()
    });

    assert_eq!(candidates, vec![ScrollDeliveryCandidate::ForegroundHid]);
  }

  #[test]
  fn scroll_attempt_candidates_background_only_drops_foreground_hid() {
    let candidates = scroll_attempt_candidates(&ScrollOptions {
      policy: InputPolicy::BackgroundOnly,
      ..ScrollOptions::default()
    });

    assert_eq!(
      candidates,
      vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
      ]
    );
  }

  #[test]
  fn prepare_for_input_rejects_unimplemented_focus_guard_without_activation() {
    let session = MacosDriverSession { _private: () };
    let window = sample_window();
    let options = PrepareForInputOptions {
      activation: ActivationPolicy::NoChange,
      preserve_frontmost: false,
      install_focus_guard: true,
      settle: Duration::ZERO,
    };

    let result = session.window().prepare_for_input(&window, options);

    assert!(matches!(
      result,
      Err(DriverError::Unsupported {
        operation: "focus_guard"
      })
    ));
  }

  #[test]
  fn foreground_input_with_default_restore_options_is_unsupported() {
    let session = MacosDriverSession { _private: () };
    let window = sample_window();
    let options = PrepareForInputOptions {
      activation: ActivationPolicy::Foreground {
        settle: Duration::ZERO,
      },
      ..PrepareForInputOptions::default()
    };

    let result = session.window().prepare_for_input(&window, options);

    assert!(matches!(
      result,
      Err(DriverError::Unsupported {
        operation: "foreground_restore"
      })
    ));
  }

  #[test]
  fn window_mutation_candidates_use_native_candidate_for_kind() {
    let candidates = window_mutation_candidates(&WindowMutationOptions::default());

    assert_eq!(
      candidates,
      vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ]
    );
    assert!(candidate_supports_window_mutation(
      candidates[0],
      WindowMutationKind::MoveTo {
        point: Point::new(10.0, 20.0),
      }
    ));
    assert!(!candidate_supports_window_mutation(
      candidates[1],
      WindowMutationKind::MoveTo {
        point: Point::new(10.0, 20.0),
      }
    ));

    assert!(candidate_supports_window_mutation(
      candidates[1],
      WindowMutationKind::Minimize,
    ));
  }

  #[test]
  fn window_mutation_foreground_policy_is_explicit_deferred_candidate() {
    let candidates = window_mutation_candidates(&WindowMutationOptions {
      policy: WindowMutationPolicy::ForegroundPreferred,
      ..WindowMutationOptions::default()
    });

    assert_eq!(
      candidates,
      vec![WindowMutationCandidate::ForegroundSystemEvents]
    );
  }

  #[test]
  fn window_mutation_native_only_preserves_explicit_foreground_candidate() {
    let candidates = window_mutation_candidates(&WindowMutationOptions {
      policy: WindowMutationPolicy::NativeOnly,
      strategy: auv_driver::WindowMutationStrategy {
        candidates: vec![WindowMutationCandidate::ForegroundSystemEvents],
      },
      ..WindowMutationOptions::default()
    });

    assert_eq!(
      candidates,
      vec![WindowMutationCandidate::ForegroundSystemEvents]
    );
  }

  #[test]
  fn decoded_window_mutation_request_rounds_geometry_for_native_bridge() {
    let request = decoded_window_mutation_request(
      123,
      42,
      "Library".to_string(),
      WindowMutationKind::SetFrame {
        frame: Rect::new(10.4, 20.5, 800.2, 600.8),
      },
    )
    .expect("request");

    assert_eq!(request.pid, 123);
    assert_eq!(request.window_number, 42);
    assert_eq!(request.title, "Library");
    assert_eq!(
      request.kind,
      crate::native::window::DecodedWindowMutationKind::SetFrame
    );
    assert_eq!(
      (request.x, request.y, request.width, request.height),
      (10, 21, 800, 601)
    );
  }

  #[test]
  fn decoded_window_mutation_request_rejects_non_positive_size() {
    let result = decoded_window_mutation_request(
      123,
      42,
      String::new(),
      WindowMutationKind::Resize {
        size: Size::new(0.0, 100.0),
      },
    );

    assert!(matches!(result, Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn window_mutation_result_maps_native_frames_and_disturbance() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAttribute,
      vec![WindowMutationAttempt::success(
        WindowMutationPath::AxWindowAttribute,
        "set AXPosition",
      )],
      None,
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "move_to".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 30,
        after_y: 40,
        after_width: 800,
        after_height: 600,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    assert_eq!(result.selected_path, WindowMutationPath::AxWindowAttribute);
    assert_eq!(
      result.before_frame,
      Some(Rect::new(10.0, 20.0, 800.0, 600.0))
    );
    assert_eq!(
      result.after_frame,
      Some(Rect::new(30.0, 40.0, 800.0, 600.0))
    );
    assert_eq!(result.focus_disturbance, DisturbanceLevel::None);
    assert_eq!(result.mouse_disturbance, DisturbanceLevel::None);
  }

  #[test]
  fn window_mutation_frame_verification_rejects_clamped_frame() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAttribute,
      Vec::new(),
      None,
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "resize".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 10,
        after_y: 20,
        after_width: 400,
        after_height: 300,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    let error = verify_window_mutation(
      WindowMutationKind::Resize {
        size: Size::new(800.0, 600.0),
      },
      &WindowMutationVerification::FrameTolerance { points: 2.0 },
      &result,
    )
    .expect_err("clamped frame should fail verification");

    assert!(error.to_string().contains("frame.size.width"));
  }

  #[test]
  fn window_mutation_state_verification_rejects_failed_minimize() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAction,
      Vec::new(),
      None,
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "minimize".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 10,
        after_y: 20,
        after_width: 800,
        after_height: 600,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    let error = verify_window_mutation(
      WindowMutationKind::Minimize,
      &WindowMutationVerification::BestEffortState,
      &result,
    )
    .expect_err("failed minimize should fail verification");

    assert!(error.to_string().contains("not minimized"));
  }

  #[test]
  fn window_mutation_failure_preserves_attempt_messages() {
    let error = window_mutation_failure(
      vec![
        WindowMutationAttempt::failure(WindowMutationPath::AxWindowAttribute, "stale window"),
        WindowMutationAttempt::failure(
          WindowMutationPath::ForegroundSystemEvents,
          "foreground fallback deferred",
        ),
      ],
      Some("stale window".to_string()),
    );

    let message = error.to_string();
    assert!(message.contains("stale window"));
    assert!(message.contains("foreground fallback deferred"));
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::selector::{App, Window as SelectWindow};

  use super::*;

  #[test]
  fn main_visible_picks_visible_window_without_requiring_main_flag() {
    let snapshot = observed_windows(vec![
      observed_window(1, 10, "com.example.music", "Music", "", 100, 80),
      observed_window(2, 10, "com.example.music", "Music", "Library", 300, 220),
    ]);
    let selector = SelectWindow::main_visible();

    let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

    assert_eq!(resolved.reference.id, "2");
  }

  #[test]
  fn main_visible_owned_by_pid_picks_visible_window_for_owner() {
    let snapshot = observed_windows(vec![
      observed_window(1, 10, "com.example.music", "Music", "Search", 320, 240),
      observed_window(2, 20, "com.example.chat", "Chat", "Conversation", 640, 480),
    ]);
    let selector = SelectWindow::main_visible().owned_by(App::pid(10));

    let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

    assert_eq!(resolved.reference.id, "1");
  }

  #[test]
  fn main_visible_owned_by_bundle_picks_visible_window_without_candidate_display_context() {
    let snapshot = observed_windows(vec![observed_window(
      307,
      15679,
      "com.netease.163music",
      "NetEaseMusic",
      "",
      1389,
      1050,
    )]);
    let selector = SelectWindow::main_visible().owned_by(App::bundle("com.netease.163music"));

    let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

    assert_eq!(resolved.reference.id, "307");
  }

  #[test]
  fn display_selector_matches_native_id_and_compat_display_ref() {
    let targets = vec![display_target(0, "100", "display_0", false)];

    let by_native = resolve_display_target(&targets, Some("100")).unwrap();
    let by_ref = resolve_display_target(&targets, Some("display_0")).unwrap();

    assert_eq!(by_native.display.id, "100");
    assert_eq!(by_ref.display.name.as_deref(), Some("display_0"));
  }

  #[test]
  fn display_selector_defaults_to_primary_display() {
    let targets = vec![
      display_target(0, "100", "display_0", false),
      display_target(1, "200", "display_1", true),
    ];

    let resolved = resolve_display_target(&targets, None).unwrap();

    assert_eq!(resolved.display.id, "200");
  }

  #[test]
  fn display_region_resolution_requires_contained_global_region() {
    let targets = vec![display_target(0, "100", "display_0", true)];

    let resolved =
      resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 40.0, 50.0)).unwrap();
    let outside =
      resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 2000.0, 50.0));

    assert_eq!(resolved.display.id, "100");
    assert!(matches!(outside, Err(DriverError::NotFound { .. })));
  }

  fn observed_windows(windows: Vec<ObservedWindow>) -> ObservedWindowSnapshot {
    ObservedWindowSnapshot {
      frontmost_app_name: String::new(),
      frontmost_app_bundle_id: String::new(),
      frontmost_window_title: String::new(),
      observed_at: "test".to_string(),
      windows,
    }
  }

  fn observed_window(
    window_number: i64,
    owner_pid: i64,
    owner_bundle_id: &str,
    app_name: &str,
    title: &str,
    width: i64,
    height: i64,
  ) -> ObservedWindow {
    ObservedWindow {
      window_number,
      app_name: app_name.to_string(),
      owner_pid,
      owner_bundle_id: owner_bundle_id.to_string(),
      layer: 0,
      title: title.to_string(),
      bounds: ObservedRect {
        x: 0,
        y: 0,
        width,
        height,
      },
    }
  }

  fn display_target(
    index: usize,
    native_id: &str,
    display_ref: &str,
    is_primary: bool,
  ) -> MacosDisplayTarget {
    MacosDisplayTarget {
      index,
      display: Display {
        id: native_id.to_string(),
        name: Some(display_ref.to_string()),
        frame: Rect::new(0.0, 0.0, 1000.0, 800.0),
        coordinate_space: CoordinateSpace::Screen,
        scale_factor: 2.0,
        is_primary,
        is_builtin: Some(false),
      },
    }
  }
}
