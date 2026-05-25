use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_driver::capture::{Activation, Capture, CaptureOptions};
use auv_driver::error::{DriverError, DriverResult};
use auv_driver::geometry::{CoordinateSpace, Point, RatioRect, Rect};
use auv_driver::input::{Click, PasteTextOptions, TextSubmit, WaitOptions};
use auv_driver::selector::{AppSelector, TextMatcher, WindowSelector};
use auv_driver::vision::{RecognizedText, TextRecognition};
use auv_driver::window::{Window, WindowRef};
use image::RgbaImage;

use crate::driver::MacosDriverSession;
use crate::native::ocr::NativeOcrTextCapture;
use crate::native::types::{ObservedRect, ObservedWindow, ObservedWindowSnapshot};
use crate::support::{build_window_candidates, parse_app_selector, resolve_app_ref};
use crate::types::{WindowRef as NativeWindowRef, WindowSelection};

static TEMP_CAPTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

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
pub struct VisionApi<'a> {
  session: &'a MacosDriverSession,
}

impl MacosDriverSession {
  pub fn window(&self) -> WindowApi<'_> {
    WindowApi { session: self }
  }

  pub fn input(&self) -> InputApi<'_> {
    InputApi { session: self }
  }

  pub fn clipboard(&self) -> ClipboardApi<'_> {
    ClipboardApi { session: self }
  }

  pub fn vision(&self) -> VisionApi<'_> {
    VisionApi { session: self }
  }
}

impl WindowApi<'_> {
  pub fn list(&self) -> DriverResult<Vec<Window>> {
    let _ = self.session;
    let snapshot = crate::native::window::observe_windows_snapshot(256, "").map_err(backend)?;
    Ok(
      snapshot
        .windows
        .iter()
        .map(|window| window_from_observed(window, None))
        .collect(),
    )
  }

  pub fn resolve(&self, selector: WindowSelector) -> DriverResult<Window> {
    let snapshot = crate::native::window::observe_windows_snapshot(256, "").map_err(backend)?;
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
      let resolved_app = resolve_app_ref(
        &snapshot,
        &parse_app_selector(&app_selector).map_err(invalid_input)?,
      )
      .map_err(backend)?;
      let displays = Vec::new();
      let candidate = if selector.main_visible && selector.title.is_none() {
        build_window_candidates(&snapshot, &resolved_app, &displays)
          .map_err(backend)?
          .into_iter()
          .find(|candidate| candidate.is_main_candidate)
      } else {
        let selection = window_selection_from_selector(&selector);
        crate::support::resolve_window_candidate(&snapshot, &resolved_app, &displays, &selection)
          .ok()
      }
      .ok_or_else(|| not_found("window selector"))?;
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
    let _ = self.session;
    let path = write_temp_capture_png(capture)?;
    let crop = ratio_rect_to_observed(capture, region);
    let native_result =
      crate::native::ocr::find_text(&path, "", false, false, 256, Some(&crop)).map_err(backend);
    let remove_result = std::fs::remove_file(&path);
    let native = native_result?;
    if let Err(error) = remove_result {
      return Err(backend(format!(
        "OCR succeeded but failed to remove temporary image {}: {error}",
        path.display()
      )));
    }
    Ok(text_recognition_from_native(&native, capture))
  }

  pub fn find_text_in_capture(
    &self,
    capture: &Capture,
    query: &str,
    region: RatioRect,
  ) -> DriverResult<OcrMatches> {
    let recognition = self.recognize_text_in_capture(capture, region)?;
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

#[cfg(target_os = "macos")]
fn capture_window(window: &Window) -> DriverResult<Capture> {
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

fn write_temp_capture_png(capture: &Capture) -> DriverResult<PathBuf> {
  let path = temp_png_path("auv-driver-macos-capture");
  capture.image.save(&path).map_err(|error| {
    backend(format!(
      "failed to write temporary OCR image {}: {error}",
      path.display()
    ))
  })?;
  Ok(path)
}

fn temp_png_path(label: &str) -> PathBuf {
  let millis = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis();
  let sequence = TEMP_CAPTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "{label}-{}-{millis}-{sequence}.png",
    std::process::id()
  ))
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

fn text_submit_key_code(submit: TextSubmit) -> DriverResult<Option<u32>> {
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
}
