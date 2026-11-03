use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportValue, OptionalReportText, invoke_command,
};
use auv_cli_common::{TableRow, outputs::formats::table::TableOptions};
use auv_driver::ScreenPoint;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use auv_driver::WindowInput as _;
use auv_driver::overlay::{
  Overlay,
  components::{CaptureFrame, ClickTarget},
  layers::Outline,
  style::{Insets, OutlineStyle},
};
use auv_tracing::ArtifactMetadata;
use clap::{Args, ValueEnum};
use std::time::Duration;

use crate::artifact::{emit_capture, emit_capture_with_receipt};

pub fn group() -> CommandGroup {
  // TODO(invoke-window-stubs): incomplete window commands stay intentionally
  // unregistered until owner-approved implementations have behavioral evidence.
  let group = CommandGroup::new("window", "WINDOW")
    .command(list_windows_invoke_command())
    .command(get_window_invoke_command())
    .command(set_window_frame_invoke_command())
    .command(set_window_state_invoke_command())
    .command(focus_window_invoke_command())
    .command(capture_window_invoke_command())
    .command(find_window_text_invoke_command())
    .command(wait_for_window_text_invoke_command())
    .command(click_window_text_invoke_command());
  super::input::window_scroll_commands().into_iter().fold(group, CommandGroup::command)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke window.list --wide\n  auv invoke window.list --exclude-minimized")]
pub(crate) struct ListWindowsArgs {
  /// Leave out minimized windows.
  #[arg(long)]
  #[serde(rename = "exclude-minimized", default)]
  exclude_minimized: bool,
  /// Leave out windows known to be on another virtual desktop.
  #[arg(long)]
  #[serde(rename = "exclude-other-desktops", default)]
  exclude_other_desktops: bool,
}

impl ListWindowsArgs {
  pub(crate) fn filter(&self) -> auv_driver::WindowListFilter {
    auv_driver::WindowListFilter {
      exclude_minimized: self.exclude_minimized,
      exclude_other_desktops: self.exclude_other_desktops,
    }
  }
}

#[invoke_command(
  id = "window.list",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = Direct,
  group = "window",
  description = "List the windows a user could switch to, minimized windows and windows on other desktops included, with frames and state.",
  input = ListWindowsArgs,
)]
async fn list_windows(input: InvokeCommandInput, args: ListWindowsArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let filter = args.filter();
    let windows = observe_windows().await?.into_iter().filter(|window| filter.keeps(window)).collect::<Vec<_>>();
    Ok(list_windows_output(&windows)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.list is only available on macOS, Linux and Windows"))
  }
}

/// Builds the transport-independent direct result for `window.list`.
///
/// Local and daemon-backed frontends use this same projection so selecting a
/// Device changes placement without changing the command result schema.
pub fn list_windows_output(windows: &[auv_driver::Window]) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(windows)?.with_report(window_list_report(windows)))
}

pub async fn observe_windows() -> Result<Vec<auv_driver::Window>, crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    Ok(session.window().list().map_err(|error| error.to_string())?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.list is only available on macOS, Linux and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.get --target window:1234\n  auv invoke window.get --target app:com.apple.TextEdit"
)]
pub(crate) struct GetWindowArgs {
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  pub(crate) title: Option<String>,
}

#[invoke_command(
  id = "window.get",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = Direct,
  target = RequiredWindow,
  group = "window",
  description = "Read one window fresh: its frame and state (minimized, maximized, focused, stacking, desktop).",
  input = GetWindowArgs,
)]
async fn get_window(input: InvokeCommandInput, args: GetWindowArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }
    let session = auv::local::open()?;
    let window = super::input::resolve_local_window(&session, &input, args.title.as_deref())?;
    Ok(window_output(&window)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.get is only available on macOS, Linux and Windows"))
  }
}

/// The direct result for `window.get`, shared by the local and Runner routes.
pub fn window_output(window: &auv_driver::Window) -> InvokeCommandResult {
  let mut fields = window_report_fields(window);
  fields.push(InvokeReportField::new("State", window_state_report(&window.state)));
  Ok(InvokeCommandOutput::from_result(window)?.with_report(InvokeReport::new(fields, Vec::new())))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.setFrame --target window:1234 --x 100 --y 80\n  auv invoke window.setFrame --target app:com.apple.TextEdit --width 800 --height 600\n\nCoordinates are the outer frame in logical screen points. An omitted value keeps the window's current one. A maximized window becomes normal at the new frame. A minimized window is refused: call window.setState normal first."
)]
pub(crate) struct SetWindowFrameArgs {
  /// New left edge in logical screen points.
  #[arg(long, allow_negative_numbers = true)]
  x: Option<f64>,
  /// New top edge in logical screen points.
  #[arg(long, allow_negative_numbers = true)]
  y: Option<f64>,
  /// New outer width in logical points.
  #[arg(long)]
  width: Option<f64>,
  /// New outer height in logical points.
  #[arg(long)]
  height: Option<f64>,
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  pub(crate) title: Option<String>,
}

impl SetWindowFrameArgs {
  /// Fails unless at least one finite value is given and every size is positive.
  pub(crate) fn validate(&self) -> Result<(), crate::InvokeFailure> {
    let invalid = |message: &str| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, format!("window.setFrame {message}"));
    let values = [self.x, self.y, self.width, self.height];
    if values.iter().all(Option::is_none) {
      return Err(invalid("needs at least one of --x, --y, --width and --height"));
    }
    if values.iter().flatten().any(|value| !value.is_finite()) {
      return Err(invalid("values must be finite"));
    }
    if [self.width, self.height].iter().flatten().any(|value| *value <= 0.0) {
      return Err(invalid("--width and --height must be positive"));
    }
    Ok(())
  }

  /// The origin and size to request, filling omitted values from `current`.
  /// A component without any given value stays `None`, so the driver keeps it.
  pub(crate) fn request(&self, current: auv_driver::Rect) -> (Option<auv_driver::Point>, Option<auv_driver::Size>) {
    let origin = (self.x.is_some() || self.y.is_some())
      .then(|| auv_driver::Point::new(self.x.unwrap_or(current.origin.x), self.y.unwrap_or(current.origin.y)));
    let size = (self.width.is_some() || self.height.is_some())
      .then(|| auv_driver::Size::new(self.width.unwrap_or(current.size.width), self.height.unwrap_or(current.size.height)));
    (origin, size)
  }
}

#[invoke_command(
  id = "window.setFrame",
  support = { macos: Supported, linux: Unsupported("not supported on Linux yet; pull requests are welcome"), windows: Supported },
  runner = Direct,
  target = RequiredWindow,
  group = "window",
  description = "Move and/or resize one normal window in logical screen points (--x --y --width --height, each optional) and report the frame before and after.",
  input = SetWindowFrameArgs,
)]
async fn set_window_frame(input: InvokeCommandInput, args: SetWindowFrameArgs) -> crate::InvokeExecutionResult {
  args.validate()?;
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }
    let session = auv::local::open()?;
    let window = super::input::resolve_local_window(&session, &input, args.title.as_deref())?;
    let (origin, size) = args.request(window.frame);
    let target = window.clone();
    let result = super::input::run_cancellable_input(&input.cancellation, move || {
      session.window().set_frame(&target, origin, size, Default::default())
    })
    .await?;
    Ok(window_mutation_output(&window, &result)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = input;
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.setFrame is only available on macOS and Windows"))
  }
}

/// A target window state on the command line.
#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowStateArg {
  Normal,
  Minimized,
  Maximized,
}

impl From<WindowStateArg> for auv_driver::WindowStateTarget {
  fn from(state: WindowStateArg) -> Self {
    match state {
      WindowStateArg::Normal => Self::Normal,
      WindowStateArg::Minimized => Self::Minimized,
      WindowStateArg::Maximized => Self::Maximized,
    }
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.setState minimized --target window:1234\n  auv invoke window.setState normal --target window:1234\n\nnormal leaves both minimized and maximized. On macOS, maximized sets the frame to the visible frame of the window's display, and normal restores the frame recorded by the same Runner (each invoke without --run gets a new Runner)."
)]
pub(crate) struct SetWindowStateArgs {
  /// The state to move the window to.
  #[arg(value_enum, value_name = "STATE")]
  pub(crate) state: WindowStateArg,
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  pub(crate) title: Option<String>,
}

#[invoke_command(
  id = "window.setState",
  support = { macos: Supported, linux: Unsupported("not supported on Linux yet; pull requests are welcome"), windows: Supported },
  runner = Direct,
  target = RequiredWindow,
  group = "window",
  description = "Move one window to normal, minimized or maximized and report its state before and after.",
  input = SetWindowStateArgs,
)]
async fn set_window_state(input: InvokeCommandInput, args: SetWindowStateArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }
    let session = auv::local::open()?;
    let window = super::input::resolve_local_window(&session, &input, args.title.as_deref())?;
    let (target, state) = (window.clone(), args.state.into());
    let result =
      super::input::run_cancellable_input(&input.cancellation, move || session.window().set_state(&target, state, Default::default()))
        .await?;
    Ok(window_mutation_output(&window, &result)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.setState is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.focus --target window:1234\n\nFocus brings the window's application to the front. A minimized window fails: call window.setState normal first."
)]
pub(crate) struct FocusWindowArgs {
  /// Window title text used with an app target.
  #[arg(long, value_name = "TEXT")]
  pub(crate) title: Option<String>,
}

#[invoke_command(
  id = "window.focus",
  support = { macos: Supported, linux: Unsupported("not supported on Linux yet; pull requests are welcome"), windows: Supported },
  runner = Direct,
  target = RequiredWindow,
  group = "window",
  description = "Raise one window and give it keyboard focus, bringing its application to the front; report the focus disturbance and whether it is focused afterwards.",
  input = FocusWindowArgs,
)]
async fn focus_window(input: InvokeCommandInput, args: FocusWindowArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }
    let session = auv::local::open()?;
    let window = super::input::resolve_local_window(&session, &input, args.title.as_deref())?;
    let target = window.clone();
    let result =
      super::input::run_cancellable_input(&input.cancellation, move || session.window().focus(&target, Default::default())).await?;
    Ok(window_mutation_output(&window, &result)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.focus is only available on macOS and Windows"))
  }
}

#[derive(serde::Serialize)]
struct WindowMutationOutput<'a> {
  window: &'a auv_driver::WindowRef,
  result: &'a auv_driver::WindowMutationResult,
}

/// The direct result for `window.setFrame`, `window.setState` and
/// `window.focus`, shared by the local and Runner routes.
pub fn window_mutation_output(window: &auv_driver::Window, result: &auv_driver::WindowMutationResult) -> InvokeCommandResult {
  let mut fields = vec![InvokeReportField::new(
    "Window ID",
    window.reference.id.clone(),
  )];
  if let Some(title) = &window.title {
    fields.push(InvokeReportField::new("Window title", title));
  }
  let frame = |frame: Option<auv_driver::Rect>| frame.map(|frame| frame.report_value()).unwrap_or_else(|| "unknown".to_string());
  let state = |state: Option<&auv_driver::WindowState>| state.map(window_state_report).unwrap_or_else(|| "unknown".to_string());
  fields.extend([
    InvokeReportField::new("Path", format!("{:?}", result.selected_path)),
    InvokeReportField::new("Attempts", result.attempts.len().to_string()),
    InvokeReportField::new("Frame before", frame(result.before_frame)),
    InvokeReportField::new("Frame after", frame(result.after_frame)),
    InvokeReportField::new("State before", state(result.before_state.as_ref())),
    InvokeReportField::new("State after", state(result.after_state.as_ref())),
    InvokeReportField::new("Adjusted by app", result.adjusted_by_app.to_string()),
    InvokeReportField::new("Focus disturbance", result.focus_disturbance.as_str()),
  ]);
  let output = InvokeCommandOutput::from_result(&WindowMutationOutput {
    window: &window.reference,
    result,
  })?;
  Ok(output.with_report(InvokeReport::new(fields, Vec::new())))
}

/// `normal`, `minimized`, `maximized` or `fullscreen`, plus `focused`, from
/// the facts the platform reported; `unknown` when it reported none.
fn window_state_report(state: &auv_driver::WindowState) -> String {
  let mut parts = [
    (state.is_minimized, "minimized"),
    (state.is_maximized, "maximized"),
    (state.is_fullscreen, "fullscreen"),
  ]
  .into_iter()
  .filter(|(fact, _)| *fact == Some(true))
  .map(|(_, name)| name)
  .collect::<Vec<_>>();
  if parts.is_empty() && state.is_minimized.is_some() {
    parts.push("normal");
  }
  if state.is_focused == Some(true) {
    parts.push("focused");
  }
  if parts.is_empty() {
    return "unknown".to_string();
  }
  parts.join(",")
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.capture --title Untitled\n  auv invoke window.capture --target com.apple.TextEdit --title Untitled\n  auv invoke window.capture --target notepad.exe\n\nOn Linux, app targets match the AT-SPI AccessibleId in window.list (APP ID in --wide output). Use --title when that field is absent. Linux captures a display crop and does not activate or unocclude the window.\n\nOn Windows, an app target is the executable file name shown as APP in window.list, matched exactly and ignoring case (for example notepad.exe). Windows renders the window itself with PrintWindow, without activating, raising or restoring it, and keeps the visible frame."
)]
struct CaptureWindowArgs {
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TEXT")]
  title: Option<String>,
}

#[invoke_command(
  id = "window.capture",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = Direct,
  target = OptionalApplication,
  group = "window",
  description = "Capture one single-display window without activating it and emit a coordinate contract. Linux uses an AT-SPI display crop; Windows uses PrintWindow.",
  input = CaptureWindowArgs,
)]
async fn capture_window(input: InvokeCommandInput, args: CaptureWindowArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let selector = window_selector(&input, args.title.as_deref())?;
    input.overlay_enabled()?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let session = auv::local::open().map_err(|error| error.to_string())?;
    let (result, artifact) = capture_selected_window_recorded_with_session(&session, selector).await?;
    input.cancellation.check().map_err(|error| error.to_string())?;
    let capture_overlay = Overlay::new().with_layer(
      CaptureFrame::new(result.window.frame).with_label(result.window.title.clone().unwrap_or_else(|| "selected window".to_string())),
    );
    let overlay = super::overlay::show_overlay(&input, &session, capture_overlay, show_options(120, 180))?;
    let mut output = window_capture_output_with_artifact(&result.window, super::capture_result(&result.capture), artifact)?;
    output.report.as_mut().expect("window capture output always has a report").fields.push(overlay.report_field());
    // TODO(invoke-window-capture-backend): live testing on 2026-06-18 showed
    // ScreenCaptureKit single-window capture can time out and xcap fallback can
    // fail for Chrome/NetEase windows. Stabilize the typed window capture backend
    // before treating window.* evidence as reliably available.
    Ok(output)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.capture is only available on macOS, Linux and Windows"))
  }
}

#[derive(Clone, Debug)]
pub struct WindowCapture {
  pub window: auv_driver::Window,
  pub capture: auv_driver::Capture,
}

#[derive(serde::Serialize)]
pub struct WindowCaptureResult<'a> {
  window: &'a auv_driver::Window,
  capture: super::CaptureResult<'a>,
}

pub fn window_capture_result(result: &WindowCapture) -> WindowCaptureResult<'_> {
  WindowCaptureResult {
    window: &result.window,
    capture: super::capture_result(&result.capture),
  }
}

/// Records and projects a Runner-held window capture. `evidence` is its pixels
/// when this call records artifacts.
pub async fn recorded_window_capture_output(
  window: &auv_driver::Window,
  capture: super::CaptureResult<'_>,
  evidence: Option<&auv_driver::Capture>,
) -> InvokeCommandResult {
  let artifact = match evidence {
    Some(capture) => emit_capture_with_receipt("auv.driver.window_capture", capture).await,
    None => None,
  };
  window_capture_output_with_artifact(window, capture, artifact)
}

fn window_capture_output_with_artifact(
  window: &auv_driver::Window,
  capture: super::CaptureResult<'_>,
  artifact: Option<ArtifactMetadata>,
) -> InvokeCommandResult {
  let pixel_size = capture.pixel_size_report();
  let mut output = InvokeCommandOutput::from_result(&WindowCaptureResult { window, capture })?;
  let mut fields = window_report_fields(window);
  fields.push(InvokeReportField::new("Pixel size", pixel_size));
  output.report = Some(InvokeReport::new(fields, Vec::new()));
  Ok(output.with_artifacts(artifact))
}

pub async fn capture_selected_window(selector: auv_driver::WindowSelector) -> Result<WindowCapture, crate::InvokeFailure> {
  capture_selected_window_recorded(selector).await.map(|(capture, _)| capture)
}

async fn capture_selected_window_recorded(
  selector: auv_driver::WindowSelector,
) -> Result<(WindowCapture, Option<ArtifactMetadata>), crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    Ok(capture_selected_window_recorded_with_session(&session, selector).await?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = selector;
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.capture is only available on macOS, Linux and Windows"))
  }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn capture_selected_window_recorded_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
) -> Result<(WindowCapture, Option<ArtifactMetadata>), String> {
  let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
  let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
  // NOTICE: window is the selector snapshot; a Driver can refresh geometry
  // before capture. The captured region is authoritative in capture.bounds.
  // TODO: descriptor refresh remains deferred until an owner-approved result
  // contract defines how the resolved and captured observations agree.
  let artifact = emit_capture_with_receipt("auv.driver.window_capture", &capture).await;
  Ok((WindowCapture { window, capture }, artifact))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.findText \"Settings\" --title Preferences\n\nOn Windows, an app target is the executable file name shown as APP in window.list (for example notepad.exe). Text comes from the system OCR of a PrintWindow capture of the window itself.\nOn Linux, an app target is the application id shown in window.list (for example org.gnome.TextEditor); text comes from the visible part of the window."
)]
struct FindWindowTextArgs {
  /// Text to locate in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
}

#[invoke_command(
  id = "window.findText",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = Direct,
  target = OptionalApplication,
  group = "window",
  description = "Capture a resolved window and locate OCR text anchors in logical screen coordinates.",
  input = FindWindowTextArgs,
)]
async fn find_window_text(input: InvokeCommandInput, args: FindWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = args.query;
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let result =
      recognize_window_text_with_session(&session, window_selector(&input, args.title.as_deref())?, query, None, &input.cancellation)
        .await?;
    let overlay = super::overlay::show_overlay(&input, &session, window_text_overlay(&result.matches, None), show_options(120, 420))?;
    Ok(window_text_matches_output(&input.command_id, &result, overlay)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS, Linux and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.waitForText \"Ready\" --title Import\n\nOn Windows, an app target is the executable file name shown as APP in window.list (for example notepad.exe).\nOn Linux, an app target is the application id shown in window.list (for example org.gnome.TextEditor); text comes from the visible part of the window."
)]
pub(crate) struct WaitForWindowTextArgs {
  /// Text to wait for in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
  /// Milliseconds to keep polling before failing (default 10000, 100..=120000).
  #[arg(long = "timeout-ms", value_name = "MS")]
  #[serde(rename = "timeout-ms", default)]
  timeout_ms: Option<u64>,
  /// Milliseconds between OCR attempts (default 250, 50..=10000).
  #[arg(long = "interval-ms", value_name = "MS")]
  #[serde(rename = "interval-ms", default)]
  interval_ms: Option<u64>,
}

impl WaitForWindowTextArgs {
  fn options(&self) -> Result<auv_driver::WaitOptions, String> {
    super::screen::text_wait_options(self.timeout_ms, self.interval_ms)
  }

  /// Decodes the query and wait budget from protocol inputs (Runner path).
  pub(crate) fn decode(input: &InvokeCommandInput) -> Result<(String, auv_driver::WaitOptions), String> {
    let args = crate::command::decode_args::<Self>(input)?;
    let options = args.options()?;
    Ok((args.query, options))
  }
}

#[invoke_command(
  id = "window.waitForText",
  support = { macos: Supported, linux: Supported, windows: Supported },
  runner = Direct,
  target = OptionalApplication,
  group = "window",
  description = "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  input = WaitForWindowTextArgs,
)]
async fn wait_for_window_text(input: InvokeCommandInput, args: WaitForWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
  {
    let options = args.options().map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
    if input.dry_run {
      return Ok(InvokeCommandOutput::completed());
    }

    let query = args.query;
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let result = recognize_window_text_with_session(
      &session,
      window_selector(&input, args.title.as_deref())?,
      query,
      Some(options),
      &input.cancellation,
    )
    .await?;
    let overlay = super::overlay::show_overlay(&input, &session, window_text_overlay(&result.matches, None), show_options(120, 420))?;
    Ok(window_text_matches_output(&input.command_id, &result, overlay)?)
  }
  #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS, Linux and Windows"))
  }
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(
  after_long_help = "Examples:\n  auv invoke window.clickText \"Continue\" --title Setup\n  auv invoke window.clickText \"AGENTS.md\" --index 1"
)]
struct ClickWindowTextArgs {
  /// Text anchor to click in the selected window.
  #[arg(value_name = "TEXT")]
  query: String,
  /// Window title text used to select the capture target.
  #[arg(long, value_name = "TITLE")]
  title: Option<String>,
  /// Zero-based OCR candidate index from this invocation's capture.
  #[arg(long, value_name = "INDEX", default_value_t = 0)]
  #[serde(default)]
  index: usize,
  /// Driver policy used to deliver the click.
  #[arg(long, value_enum)]
  #[serde(rename = "input-policy")]
  input_policy: Option<WindowClickPolicyArg>,
  /// Number of clicks to deliver.
  #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
  #[serde(
    rename = "click-count",
    deserialize_with = "crate::command::deserialize_optional_nonzero_u8",
    default
  )]
  click_count: Option<u8>,
  /// Delay between repeated clicks.
  #[arg(long)]
  #[serde(rename = "click-interval-ms")]
  click_interval_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WindowClickPolicyArg {
  BackgroundOnly,
  BackgroundPreferred,
  ForegroundPreferred,
}

// Only `window.clickText` reads it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl WindowClickPolicyArg {
  fn driver_policy(self) -> auv_driver::InputPolicy {
    match self {
      Self::BackgroundOnly => auv_driver::InputPolicy::BackgroundOnly,
      Self::BackgroundPreferred => auv_driver::InputPolicy::BackgroundPreferred,
      Self::ForegroundPreferred => auv_driver::InputPolicy::ForegroundPreferred,
    }
  }
}

#[invoke_command(
  id = "window.clickText",
  support = { macos: Supported, linux: Unsupported, windows: Partial("test-owned Win32 surface, background-only") },
  // The Runner route resolves the window and clicks through the window API, so Linux delivers
  // foreground portal input (background-only rejected) instead of refusing the command.
  runner = {
    macos: Supported,
    linux: Partial("foreground portal delivery only; background_only input policy is rejected"),
    windows: Partial("background delivery posts window messages, so whether an application acts on them depends on its toolkit"),
  },
  target = OptionalApplication,
  group = "window",
  description = "Capture a resolved window, resolve an OCR text anchor, and click the match point in logical screen coordinates.",
  input = ClickWindowTextArgs,
)]
async fn click_window_text(input: InvokeCommandInput, args: ClickWindowTextArgs) -> crate::InvokeExecutionResult {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    let options =
      super::input::click_options(args.input_policy.map(WindowClickPolicyArg::driver_policy), args.click_count, args.click_interval_ms);
    if input.dry_run {
      return Ok(super::input::validation_only_output());
    }

    let session = auv::local::open().map_err(crate::InvokeFailure::from)?;
    let result =
      click_recognized_window_text_with_session(&session, window_selector(&input, args.title.as_deref())?, args.query, args.index, options)
        .await?;
    let overlay = super::overlay::show_overlay(
      &input,
      &session,
      window_text_overlay(&result.matches, Some(result.selected_index)),
      show_options(120, 240),
    )?;

    let mut output = window_text_click_output_base(&result)?;
    output.report.as_mut().expect("window text click output always has a report").fields.push(overlay.report_field());
    Ok(output)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (input, args);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.clickText is only available on macOS and Windows"))
  }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextClick {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
  pub selected_index: usize,
  pub point: auv_driver::geometry::ScreenPoint,
  /// The same point projected into the window, which is the form the driver
  /// received for delivery.
  pub window_point: auv_driver::geometry::WindowPoint,
  pub options: auv_driver::ClickOptions,
  pub action: auv_driver::InputActionResult,
}

/// Records the exact OCR source and typed delivery evidence, then builds the
/// transport-independent `window.clickText` result.
pub fn recorded_window_text_click_output(result: &WindowTextClick, ocr_source: Option<&auv_driver::Capture>) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.window_ocr_source", capture);
  }
  super::input::emit_input_action_result(&result.action);
  window_text_click_output_base(result)
}

fn window_text_click_output_base(result: &WindowTextClick) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, Some(result.selected_index));
  report.fields.extend(window_report_fields(&result.window));
  report.fields.extend(super::input::input_action_report_fields(&result.action));
  report.fields.push(InvokeReportField::new("Input policy", result.options.policy.as_str()));
  report.fields.push(InvokeReportField::new("Click count", result.options.click.count().to_string()));
  if let Some(interval) = result.options.click.interval() {
    report.fields.push(InvokeReportField::new("Click interval", format!("{} ms", interval.as_millis())));
  }
  report.fields.push(InvokeReportField::new("Screen point", format!("{:.1},{:.1}", result.point.point().x, result.point.point().y)));
  report
    .fields
    .push(InvokeReportField::new("Window point", format!("{:.1},{:.1}", result.window_point.point().x, result.window_point.point().y)));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

pub async fn click_recognized_window_text(
  selector: auv_driver::WindowSelector,
  query: String,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  click_recognized_window_text_with_options(selector, query, auv_driver::ClickOptions::default()).await
}

pub async fn click_recognized_window_text_with_options(
  selector: auv_driver::WindowSelector,
  query: String,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  click_recognized_window_text_with_index_and_options(selector, query, 0, options).await
}

pub async fn click_recognized_window_text_with_index_and_options(
  selector: auv_driver::WindowSelector,
  query: String,
  index: usize,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  #[cfg(any(target_os = "macos", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(crate::InvokeFailure::from)?;
    click_recognized_window_text_with_session(&session, selector, query, index, options).await
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    let _ = (selector, query, index, options);
    Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window.clickText is only available on macOS and Windows"))
  }
}

/// Selects one OCR match for `window.clickText` with the failure codes the
/// direct local path and the selected-Runner route must both report: a text
/// miss is `not_found` and an out-of-range `--index` is `invalid_input`.
pub fn selected_window_text_match<'a>(
  matches: &'a auv_driver::OcrMatches,
  query: &str,
  index: usize,
) -> Result<&'a auv_driver::OcrMatch, crate::InvokeFailure> {
  if matches.matches.is_empty() {
    return Err(crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("window.clickText did not find text {query:?}")));
  }
  matches.matches.get(index).ok_or_else(|| {
    crate::InvokeFailure::new(
      crate::FailureCode::InvalidInput,
      format!("window.clickText --index {index} is out of range for {} text match(es)", matches.matches.len()),
    )
  })
}

/// Projects an OCR match point into window space and verifies it falls within
/// the resolved `window.frame`. Both direct local invoke and selected Runner routes
/// share this check so out-of-bounds candidates are rejected before input delivery.
///
/// NOTICE: The boundary check evaluates against the `resolved window.frame`, NOT
/// against `capture.bounds` or the native desktop geometry at the moment of delivery.
///
/// Enforces continuous half-open valid pixel containment `[0.0, width)` and `[0.0, height)`
/// alongside non-finite and positive dimension validation.
pub(crate) fn matched_window_point(
  window: &auv_driver::Window,
  matched: &auv_driver::OcrMatch,
) -> Result<auv_driver::geometry::WindowPoint, crate::InvokeFailure> {
  let screen_point = matched.action_point();
  let point = auv_driver::geometry::WindowPoint::new(screen_point.x - window.frame.origin.x, screen_point.y - window.frame.origin.y);
  let point_value = point.point();
  let width = window.frame.size.width;
  let height = window.frame.size.height;
  if !point_value.x.is_finite()
    || !point_value.y.is_finite()
    || !width.is_finite()
    || !height.is_finite()
    || width <= 0.0
    || height <= 0.0
    || point_value.x < 0.0
    || point_value.x >= width
    || point_value.y < 0.0
    || point_value.y >= height
  {
    return Err(crate::InvokeFailure::new(
      crate::FailureCode::InvalidTarget,
      format!("recognized text point {},{} is outside resolved window bounds; no input delivered", screen_point.x, screen_point.y),
    ));
  }

  Ok(point)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn click_recognized_window_text_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
  query: String,
  index: usize,
  options: auv_driver::ClickOptions,
) -> Result<WindowTextClick, crate::InvokeFailure> {
  let window = session.window().resolve(selector).map_err(crate::InvokeFailure::from)?;
  let capture = session.window().capture(&window).map_err(crate::InvokeFailure::from)?;
  let matches = session
    .vision()
    .find_text_in_capture(&capture, &query, auv_driver::RelativeRect::new(0.0, 0.0, 1.0, 1.0))
    .map_err(crate::InvokeFailure::from)?;
  if matches.matches.is_empty() {
    return Err(crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("window.clickText did not find text {query:?}")));
  }
  let matched = selected_window_text_match(&matches, &query, index)?;
  let point = auv_driver::geometry::ScreenPoint::from(matched.action_point());
  let window_point = matched_window_point(&window, matched)?;
  let action = session.window().click(&window, window_point, options.clone()).map_err(crate::InvokeFailure::from)?;
  emit_capture("auv.driver.window_ocr_source", &capture);
  Ok(WindowTextClick {
    window,
    matches,
    selected_index: index,
    point,
    window_point,
    options,
    action,
  })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct WindowTextRecognition {
  pub window: auv_driver::Window,
  pub matches: auv_driver::OcrMatches,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub async fn recognize_window_text(
  selector: auv_driver::WindowSelector,
  query: String,
  wait: bool,
) -> Result<WindowTextRecognition, crate::InvokeFailure> {
  let session = auv::local::open().map_err(|error| error.to_string())?;
  let wait = wait.then(super::screen::default_text_wait_options);
  Ok(recognize_window_text_with_session(&session, selector, query, wait, &Default::default()).await?)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn recognize_window_text_with_session(
  session: &auv_driver::LocalDriverSession,
  selector: auv_driver::WindowSelector,
  query: String,
  wait: Option<auv_driver::WaitOptions>,
  cancellation: &crate::InvokeCancellation,
) -> Result<WindowTextRecognition, String> {
  use auv_driver::RelativeRect;

  cancellation.check().map_err(|error| error.to_string())?;
  let window = session.window().resolve(selector).map_err(|error| error.to_string())?;
  let observe = || {
    let window = &window;
    let query = &query;
    async move {
      let capture = session.window().capture(window).map_err(|error| error.to_string())?;
      let matches =
        session.vision().find_text_in_capture(&capture, query, RelativeRect::new(0.0, 0.0, 1.0, 1.0)).map_err(|error| error.to_string())?;
      // The ascription keeps this local OCR observation text-errors, which also
      // pins the shared wait helper's error type on this path.
      Ok::<_, String>((matches, capture))
    }
  };
  let (matches, capture) = match wait {
    Some(options) => {
      crate::runner::wait_for_selected_text("window.waitForText", &query, options, cancellation, observe, |(matches, _)| {
        !matches.matches.is_empty()
      })
      .await?
    }
    None => observe().await?,
  };

  // TODO(invoke-recognition-result-artifacts): this records the window OCR
  // source screenshot and typed OCR matches, but not a structured
  // recognition-result artifact with query/bounds/confidence. Add it after
  // the artifact shape is accepted in the direct-command handoff.
  emit_capture("auv.driver.window_ocr_source", &capture);
  Ok(WindowTextRecognition { window, matches })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub async fn recognize_window_text(
  _selector: auv_driver::WindowSelector,
  _query: String,
  _wait: bool,
) -> Result<WindowTextRecognition, crate::InvokeFailure> {
  Err(crate::InvokeFailure::new(crate::FailureCode::Unsupported, "window text OCR is only available on macOS, Linux and Windows"))
}

fn window_text_matches_output(
  _command_id: &str,
  result: &WindowTextRecognition,
  overlay: super::overlay::OverlayStatus,
) -> InvokeCommandResult {
  let mut output = window_text_matches_output_base(result)?;
  output.report.as_mut().expect("window text output always has a report").fields.push(overlay.report_field());
  Ok(output)
}

/// Records the OCR source and builds the transport-independent
/// `window.findText` result.
pub fn recorded_window_text_matches_output(result: &WindowTextRecognition, ocr_source: Option<&auv_driver::Capture>) -> InvokeCommandResult {
  if let Some(capture) = ocr_source {
    emit_capture("auv.driver.window_ocr_source", capture);
  }
  window_text_matches_output_base(result)
}

fn window_text_matches_output_base(result: &WindowTextRecognition) -> InvokeCommandResult {
  let mut report = crate::commands::ocr::match_report(&result.matches.matches, None);
  report.fields.extend(window_report_fields(&result.window));
  Ok(InvokeCommandOutput::from_result(result)?.with_report(report))
}

fn window_text_overlay(matches: &auv_driver::OcrMatches, selected_index: Option<usize>) -> Overlay {
  let mut overlay = Overlay::new();
  for (index, matched) in matches.matches.iter().enumerate() {
    let style = if selected_index == Some(index) {
      OutlineStyle::selected()
    } else {
      OutlineStyle::default()
    };

    overlay =
      overlay.with_layer(Outline::new(matched.bounds).with_label(matched.text.clone()).with_style(style.with_padding(Insets::all(8.0))));
  }

  if let Some(matched) = selected_index.and_then(|index| matches.matches.get(index)) {
    overlay = overlay.with_layer(
      ClickTarget::new(ScreenPoint::from(matched.action_point())).with_cursor_label("auv · click").with_status("text click delivered"),
    );
  }
  overlay
}

fn show_options(motion_ms: u64, auto_removal_ms: u64) -> auv_driver::overlay::ShowOptions {
  auv_driver::overlay::ShowOptions::new()
    .with_motion_ease(Duration::from_millis(motion_ms), auv_driver::overlay::Easing::EaseInOutExpo)
    .with_auto_removal_after(Duration::from_millis(auto_removal_ms))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn window_selector(input: &InvokeCommandInput, title: Option<&str>) -> Result<auv_driver::WindowSelector, String> {
  use auv_driver::{App, TextMatcher, WindowSelector};

  let mut selector = WindowSelector {
    main_visible: true,
    ..WindowSelector::default()
  };
  if let Some(target) = input.application_target()? {
    // NOTICE: Windows reports no application id yet (TODO(windows-application-id)).
    // Its application identifier is the executable file name, the `app_name` of
    // `window.list`, which the driver matches ignoring case for both name and id
    // selectors.
    #[cfg(target_os = "windows")]
    {
      selector.app = Some(App::name(target));
    }
    // Linux reports the AT-SPI AccessibleId as the application id. Do not
    // guess application names when that id is absent; callers can select by
    // title.
    // TODO(application-name-target): application-name targets require an
    // owner-approved target contract, as in runner.rs.
    #[cfg(not(target_os = "windows"))]
    {
      selector.app = Some(App::id(target));
    }
  }
  if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
    selector.title = Some(TextMatcher::Contains(title.to_string()));
  }
  Ok(selector)
}

fn window_report_fields(window: &auv_driver::Window) -> Vec<InvokeReportField> {
  let mut fields = vec![
    InvokeReportField::new("Window ID", window.reference.id.clone()),
    InvokeReportField::new("Window frame", window.frame.report_value()),
  ];
  if let Some(title) = &window.title {
    fields.push(InvokeReportField::new("Window title", title));
  }
  if let Some(app_name) = &window.app_name {
    fields.push(InvokeReportField::new("Application", app_name));
  }
  if let Some(application_id) = &window.application_id {
    fields.push(InvokeReportField::new("Application ID", application_id));
  }
  fields
}

#[derive(TableRow)]
struct WindowRow {
  #[table(header = "REF")]
  reference: String,
  app: String,
  title: String,
  frame: String,
  #[table(wide, header = "APP ID")]
  application_id: String,
  #[table(wide, header = "PID")]
  process_id: String,
  #[table(wide)]
  flags: String,
}

fn window_list_report(windows: &[auv_driver::Window]) -> InvokeReport {
  let rows = windows
    .iter()
    .map(|window| {
      let mut flags = Vec::new();
      if window.is_main {
        flags.push("main");
      }
      flags.push(if window.is_visible {
        "visible"
      } else {
        "hidden"
      });
      for (fact, flag) in [
        (window.state.is_minimized, "minimized"),
        (window.state.is_maximized, "maximized"),
        (window.state.is_fullscreen, "fullscreen"),
        (window.state.is_focused, "focused"),
      ] {
        if fact == Some(true) {
          flags.push(flag);
        }
      }
      WindowRow {
        reference: window.reference.id.clone(),
        app: window.app_name.as_deref().report_or("unknown").to_string(),
        title: window.title.as_deref().report_or("untitled").to_string(),
        frame: window.frame.report_value(),
        application_id: window.application_id.as_deref().report_or("unknown").to_string(),
        process_id: window.process_id.map(|pid| pid.to_string()).unwrap_or_else(|| "unknown".to_string()),
        flags: flags.join(","),
      }
    })
    .collect::<Vec<_>>();
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} window(s)", windows.len()),
    )],
    tables: vec![InvokeReportTable::from_rows(&rows, TableOptions::default()).with_display_max_chars(vec![None, Some(18), Some(40), None])],
    wide_tables: vec![
      InvokeReportTable::from_rows(&rows, TableOptions::default().wide(true)).with_display_max_chars(vec![
        None,
        Some(18),
        Some(40),
        None,
        Some(32),
        None,
        None,
      ]),
    ],
    sections: Vec::new(),
  }
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
