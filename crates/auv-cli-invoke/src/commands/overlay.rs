use std::time::Duration;

use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, invoke_command};
use auv_driver::overlay::{
  Easing, Overlay, ShowOptions,
  components::{CaptureFrame, ClickTarget},
  layers::{Cursor, CursorImage, Outline, Status},
  style::{Color, CursorStyle, Insets, OutlineStyle, StatusStyle, Stroke},
};
use auv_driver::{Rect, ScreenPoint};
use clap::Args;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OverlayStatus {
  Disabled,
  #[cfg(all(target_os = "macos", feature = "overlay"))]
  Shown {
    layers: usize,
  },
  Unavailable {
    reason: String,
  },
}

impl OverlayStatus {
  pub(crate) fn report_field(&self) -> InvokeReportField {
    match self {
      Self::Disabled => InvokeReportField::new("Overlay", "disabled"),
      #[cfg(all(target_os = "macos", feature = "overlay"))]
      Self::Shown { layers } => InvokeReportField::new("Overlay", format!("shown ({layers} layers)")),
      Self::Unavailable { reason } => InvokeReportField::new("Overlay", format!("unavailable: {reason}")),
    }
  }
}

pub(crate) fn show_overlay(
  input: &InvokeCommandInput,
  session: &auv_driver::LocalDriverSession,
  overlay: Overlay,
  options: ShowOptions,
) -> Result<OverlayStatus, String> {
  if input.dry_run || !input.overlay_enabled()? {
    return Ok(OverlayStatus::Disabled);
  }

  #[cfg(all(target_os = "macos", feature = "overlay"))]
  {
    let layers = overlay.layers().len();
    Ok(match session.overlay().show(&overlay, options) {
      Ok(()) => OverlayStatus::Shown { layers },
      Err(error) => OverlayStatus::Unavailable {
        reason: error.to_string(),
      },
    })
  }

  #[cfg(not(all(target_os = "macos", feature = "overlay")))]
  {
    let _ = (session, overlay, options);
    Ok(OverlayStatus::Unavailable {
      reason: "the local driver has no compiled overlay adapter".to_string(),
    })
  }
}

pub fn group() -> CommandGroup {
  CommandGroup::new("overlay", "OVERLAY")
    .command(show_outline_invoke_command())
    .command(show_cursor_invoke_command())
    .command(show_status_invoke_command())
    .command(show_capture_frame_invoke_command())
    .command(show_click_target_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke overlay.outline --x 20 --y 20 --width 400 --height 240 --label Selection")]
struct OutlineArgs {
  #[arg(long)]
  x: f64,
  #[arg(long)]
  y: f64,
  #[arg(long)]
  width: f64,
  #[arg(long)]
  height: f64,
  #[arg(long)]
  label: Option<String>,
  #[arg(long)]
  #[serde(rename = "label-visible")]
  label_visible: Option<bool>,
  #[arg(long)]
  padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "border-color")]
  border_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "border-width")]
  border_width: Option<f64>,
  #[arg(long)]
  #[serde(rename = "corner-radius")]
  corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "motion-duration-ms")]
  motion_duration_ms: Option<u64>,
  #[arg(long)]
  #[serde(rename = "hold-duration-ms")]
  hold_duration_ms: Option<u64>,
}

#[invoke_command(
  id = "overlay.outline",
  group = "overlay",
  description = "Present one configurable outline layer for visual style inspection.",
  input = OutlineArgs,
)]
async fn show_outline(input: InvokeCommandInput, args: OutlineArgs) -> InvokeCommandResult {
  let plan = plan_outline(&input.command_id, args)?;
  debug_output(&input, plan.component, plan.overlay, plan.options)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke overlay.cursor --x 320 --y 240 --label AUV")]
struct CursorArgs {
  #[arg(long)]
  x: f64,
  #[arg(long)]
  y: f64,
  #[arg(long)]
  label: Option<String>,
  #[arg(long)]
  #[serde(rename = "label-visible")]
  label_visible: Option<bool>,
  #[arg(long)]
  svg: Option<String>,
  #[arg(long)]
  padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "foreground-color")]
  foreground_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "background-color")]
  background_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "corner-radius")]
  corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "sprite-size")]
  sprite_size: Option<f64>,
  #[arg(long)]
  #[serde(rename = "motion-duration-ms")]
  motion_duration_ms: Option<u64>,
  #[arg(long)]
  #[serde(rename = "hold-duration-ms")]
  hold_duration_ms: Option<u64>,
}

#[invoke_command(
  id = "overlay.cursor",
  group = "overlay",
  description = "Present one configurable cursor layer, optionally using runtime SVG source.",
  input = CursorArgs,
)]
async fn show_cursor(input: InvokeCommandInput, args: CursorArgs) -> InvokeCommandResult {
  let plan = plan_cursor(&input.command_id, args)?;
  debug_output(&input, plan.component, plan.overlay, plan.options)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke overlay.status \"Click delivered\" --x 320 --y 240")]
struct StatusArgs {
  #[arg(long)]
  x: f64,
  #[arg(long)]
  y: f64,
  /// Status text to present.
  #[arg(value_name = "TEXT")]
  text: String,
  #[arg(long)]
  padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "foreground-color")]
  foreground_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "background-color")]
  background_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "corner-radius")]
  corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "motion-duration-ms")]
  motion_duration_ms: Option<u64>,
  #[arg(long)]
  #[serde(rename = "hold-duration-ms")]
  hold_duration_ms: Option<u64>,
}

#[invoke_command(
  id = "overlay.status",
  group = "overlay",
  description = "Present one configurable status layer for visual style inspection.",
  input = StatusArgs,
)]
async fn show_status(input: InvokeCommandInput, args: StatusArgs) -> InvokeCommandResult {
  let plan = plan_status(&input.command_id, args)?;
  debug_output(&input, plan.component, plan.overlay, plan.options)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke overlay.captureFrame --x 20 --y 20 --width 800 --height 600")]
struct CaptureFrameArgs {
  #[arg(long)]
  x: f64,
  #[arg(long)]
  y: f64,
  #[arg(long)]
  width: f64,
  #[arg(long)]
  height: f64,
  #[arg(long)]
  label: Option<String>,
  #[arg(long)]
  #[serde(rename = "label-visible")]
  label_visible: Option<bool>,
  #[arg(long)]
  padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "border-color")]
  border_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "border-width")]
  border_width: Option<f64>,
  #[arg(long)]
  #[serde(rename = "corner-radius")]
  corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "motion-duration-ms")]
  motion_duration_ms: Option<u64>,
  #[arg(long)]
  #[serde(rename = "hold-duration-ms")]
  hold_duration_ms: Option<u64>,
}

#[invoke_command(
  id = "overlay.captureFrame",
  group = "overlay",
  description = "Present the reusable capture-frame component around a screen rectangle.",
  input = CaptureFrameArgs,
)]
async fn show_capture_frame(input: InvokeCommandInput, args: CaptureFrameArgs) -> InvokeCommandResult {
  let plan = plan_capture_frame(&input.command_id, args)?;
  debug_output(&input, plan.component, plan.overlay, plan.options)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke overlay.clickTarget --x 20 --y 20 --width 200 --height 80 --status Ready")]
struct ClickTargetArgs {
  #[arg(long)]
  x: f64,
  #[arg(long)]
  y: f64,
  #[arg(long)]
  width: f64,
  #[arg(long)]
  height: f64,
  #[arg(long)]
  #[serde(rename = "outline-label")]
  outline_label: Option<String>,
  #[arg(long)]
  #[serde(rename = "outline-label-visible")]
  outline_label_visible: Option<bool>,
  #[arg(long)]
  #[serde(rename = "cursor-label")]
  cursor_label: Option<String>,
  #[arg(long)]
  #[serde(rename = "cursor-label-visible")]
  cursor_label_visible: Option<bool>,
  #[arg(long)]
  status: Option<String>,
  #[arg(long)]
  #[serde(rename = "outline-padding")]
  outline_padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "border-color")]
  border_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "border-width")]
  border_width: Option<f64>,
  #[arg(long)]
  #[serde(rename = "outline-corner-radius")]
  outline_corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "status-padding")]
  status_padding: Option<f64>,
  #[arg(long)]
  #[serde(rename = "status-foreground-color")]
  status_foreground_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "status-background-color")]
  status_background_color: Option<String>,
  #[arg(long)]
  #[serde(rename = "status-corner-radius")]
  status_corner_radius: Option<f64>,
  #[arg(long)]
  #[serde(rename = "motion-duration-ms")]
  motion_duration_ms: Option<u64>,
  #[arg(long)]
  #[serde(rename = "hold-duration-ms")]
  hold_duration_ms: Option<u64>,
}

#[invoke_command(
  id = "overlay.clickTarget",
  group = "overlay",
  description = "Present the reusable click-target component with outline, cursor, and status layers.",
  input = ClickTargetArgs,
)]
async fn show_click_target(input: InvokeCommandInput, args: ClickTargetArgs) -> InvokeCommandResult {
  let plan = plan_click_target(&input.command_id, args)?;
  debug_output(&input, plan.component, plan.overlay, plan.options)
}

pub struct OverlayPlan {
  pub component: &'static str,
  pub overlay: Overlay,
  pub options: ShowOptions,
}

pub fn plan_overlay(input: &InvokeCommandInput) -> Result<OverlayPlan, String> {
  let args = input.typed_args.as_ref().ok_or_else(|| format!("{} omitted typed arguments", input.command_id))?;
  match input.command_id.as_str() {
    "overlay.outline" => {
      plan_outline(&input.command_id, args.get::<OutlineArgs>().cloned().ok_or("overlay.outline argument type mismatch")?)
    }
    "overlay.cursor" => plan_cursor(&input.command_id, args.get::<CursorArgs>().cloned().ok_or("overlay.cursor argument type mismatch")?),
    "overlay.status" => plan_status(&input.command_id, args.get::<StatusArgs>().cloned().ok_or("overlay.status argument type mismatch")?),
    "overlay.captureFrame" => {
      plan_capture_frame(&input.command_id, args.get::<CaptureFrameArgs>().cloned().ok_or("overlay.captureFrame argument type mismatch")?)
    }
    "overlay.clickTarget" => {
      plan_click_target(&input.command_id, args.get::<ClickTargetArgs>().cloned().ok_or("overlay.clickTarget argument type mismatch")?)
    }
    _ => Err(format!("{} is not an overlay planner command", input.command_id)),
  }
}

fn plan_outline(command_id: &str, args: OutlineArgs) -> Result<OverlayPlan, String> {
  let rect = rect_input(command_id, args.x, args.y, args.width, args.height)?;
  let style =
    outline_style(command_id, OutlineStyle::new(), args.padding, args.border_color.as_deref(), args.border_width, args.corner_radius)?;
  let mut outline = Outline::new(rect).with_style(style);
  if let Some(label) = args.label {
    outline = outline.with_label(label);
  }
  if args.label_visible.unwrap_or(false) {
    outline = outline.with_label_visible();
  }
  Ok(OverlayPlan {
    component: "Outline",
    overlay: Overlay::new().with_layer(outline),
    options: show_options(args.motion_duration_ms, args.hold_duration_ms),
  })
}

fn plan_cursor(command_id: &str, args: CursorArgs) -> Result<OverlayPlan, String> {
  let style = cursor_style(
    command_id,
    args.padding,
    args.foreground_color.as_deref(),
    args.background_color.as_deref(),
    args.corner_radius,
    args.sprite_size,
  )?;
  let mut cursor = Cursor::new(point_input(command_id, args.x, args.y)?).with_style(style);
  if let Some(label) = args.label {
    cursor = cursor.with_label(label);
  }
  if args.label_visible.unwrap_or(false) {
    cursor = cursor.with_label_visible();
  }
  if let Some(svg) = args.svg {
    if svg.len() > 256 * 1024 {
      return Err(format!("{command_id} cursor SVG exceeds 256 KiB"));
    }
    cursor = cursor.with_image(CursorImage::svg(svg));
  }
  Ok(OverlayPlan {
    component: "Cursor",
    overlay: Overlay::new().with_layer(cursor),
    options: show_options(args.motion_duration_ms, args.hold_duration_ms),
  })
}

fn plan_status(command_id: &str, args: StatusArgs) -> Result<OverlayPlan, String> {
  let style =
    status_style(command_id, args.padding, args.foreground_color.as_deref(), args.background_color.as_deref(), args.corner_radius)?;
  let status = Status::new(point_input(command_id, args.x, args.y)?, args.text).with_style(style);
  Ok(OverlayPlan {
    component: "Status",
    overlay: Overlay::new().with_layer(status),
    options: show_options(args.motion_duration_ms, args.hold_duration_ms),
  })
}

fn plan_capture_frame(command_id: &str, args: CaptureFrameArgs) -> Result<OverlayPlan, String> {
  let rect = rect_input(command_id, args.x, args.y, args.width, args.height)?;
  let style =
    outline_style(command_id, OutlineStyle::capture(), args.padding, args.border_color.as_deref(), args.border_width, args.corner_radius)?;
  let mut frame = CaptureFrame::new(rect).with_style(style);
  if let Some(label) = args.label {
    frame = frame.with_label(label);
  }
  if args.label_visible.unwrap_or(false) {
    frame = frame.with_label_visible();
  }
  Ok(OverlayPlan {
    component: "CaptureFrame",
    overlay: Overlay::new().with_layer(frame),
    options: show_options(args.motion_duration_ms, args.hold_duration_ms),
  })
}

fn plan_click_target(command_id: &str, args: ClickTargetArgs) -> Result<OverlayPlan, String> {
  let rect = rect_input(command_id, args.x, args.y, args.width, args.height)?;
  let point = ScreenPoint::new(rect.origin.x + rect.size.width / 2.0, rect.origin.y + rect.size.height / 2.0);
  let outline_style = outline_style(
    command_id,
    OutlineStyle::selected(),
    args.outline_padding,
    args.border_color.as_deref(),
    args.border_width,
    args.outline_corner_radius,
  )?;
  let mut target = ClickTarget::new(point)
    .with_outline(rect)
    .with_outline_style(outline_style)
    .with_status(args.status.as_deref().unwrap_or("click target"));
  if let Some(label) = args.outline_label {
    target = target.with_outline_label(label);
  }
  if args.outline_label_visible.unwrap_or(false) {
    target = target.with_outline_label_visible();
  }
  if let Some(label) = args.cursor_label {
    target = target.with_cursor_label(label);
  }
  if args.cursor_label_visible.unwrap_or(false) {
    target = target.with_cursor_label_visible();
  }
  target = target.with_status_style(status_style(
    command_id,
    args.status_padding,
    args.status_foreground_color.as_deref(),
    args.status_background_color.as_deref(),
    args.status_corner_radius,
  )?);
  let options = show_options(args.motion_duration_ms, args.hold_duration_ms);
  Ok(OverlayPlan {
    component: "ClickTarget",
    overlay: Overlay::new().with_layer(target),
    options,
  })
}

fn debug_output(input: &InvokeCommandInput, component: &str, overlay: Overlay, options: ShowOptions) -> InvokeCommandResult {
  if input.target_application_id.is_some() {
    return Err(format!("{} cannot use --target; overlays use global screen coordinates", input.command_id));
  }
  let layers = overlay.layers().len();
  let status = if input.dry_run || !input.overlay_enabled()? {
    OverlayStatus::Disabled
  } else {
    #[cfg(all(target_os = "macos", feature = "overlay"))]
    {
      match auv_driver::open_local() {
        Ok(session) => show_overlay(input, &session, overlay, options)?,
        Err(error) => OverlayStatus::Unavailable {
          reason: error.to_string(),
        },
      }
    }
    #[cfg(not(all(target_os = "macos", feature = "overlay")))]
    {
      let _ = (overlay, options);
      OverlayStatus::Unavailable {
        reason: "the local driver has no compiled overlay adapter".to_string(),
      }
    }
  };
  if let OverlayStatus::Unavailable { reason } = &status {
    return Err(format!("overlay.{component} could not be shown: {reason}"));
  }
  Ok(InvokeCommandOutput::completed().with_report(InvokeReport::new(
    vec![
      InvokeReportField::new("Component", component),
      InvokeReportField::new("Layers", layers.to_string()),
      InvokeReportField::new("Motion", format!("{} ms", options.motion().duration().as_millis())),
      InvokeReportField::new(
        "Hold",
        format!(
          "{} ms",
          match options.lifecycle().removal() {
            auv_driver::overlay::Removal::AutoAfter(duration) => duration,
            auv_driver::overlay::Removal::Manual => Duration::ZERO,
          }
          .as_millis()
        ),
      ),
      status.report_field(),
    ],
    Vec::new(),
  )))
}

pub fn selected_overlay_output(plan: &OverlayPlan, shown: bool) -> InvokeCommandResult {
  let hold = match plan.options.lifecycle().removal() {
    auv_driver::overlay::Removal::AutoAfter(duration) => duration,
    auv_driver::overlay::Removal::Manual => Duration::ZERO,
  };
  Ok(InvokeCommandOutput::completed().with_report(InvokeReport::new(
    vec![
      InvokeReportField::new("Component", plan.component),
      InvokeReportField::new("Layers", plan.overlay.layers().len().to_string()),
      InvokeReportField::new("Motion", format!("{} ms", plan.options.motion().duration().as_millis())),
      InvokeReportField::new("Hold", format!("{} ms", hold.as_millis())),
      InvokeReportField::new("Overlay", if shown { "shown" } else { "disabled" }),
    ],
    Vec::new(),
  )))
}

fn show_options(motion_duration_ms: Option<u64>, hold_duration_ms: Option<u64>) -> ShowOptions {
  ShowOptions::new()
    .with_motion_ease(Duration::from_millis(motion_duration_ms.unwrap_or(320)), Easing::EaseInOutExpo)
    .with_auto_removal_after(Duration::from_millis(hold_duration_ms.unwrap_or(2_000)))
}

fn rect_input(command_id: &str, x: f64, y: f64, width: f64, height: f64) -> Result<Rect, String> {
  if !x.is_finite() || !y.is_finite() {
    return Err(format!("{command_id} requires finite --x and --y"));
  }
  if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
    return Err(format!("{command_id} requires positive finite --width and --height"));
  }
  Ok(Rect::new(x, y, width, height))
}

fn point_input(command_id: &str, x: f64, y: f64) -> Result<ScreenPoint, String> {
  if !x.is_finite() || !y.is_finite() {
    return Err(format!("{command_id} requires finite --x and --y"));
  }
  Ok(ScreenPoint::new(x, y))
}

fn outline_style(
  command_id: &str,
  mut style: OutlineStyle,
  padding: Option<f64>,
  border_color: Option<&str>,
  border_width: Option<f64>,
  corner_radius: Option<f64>,
) -> Result<OutlineStyle, String> {
  if let Some(value) = optional_non_negative(command_id, "padding", padding)? {
    style = style.with_padding(Insets::all(value));
  }
  if border_color.is_some() || border_width.is_some() {
    let color = border_color.map(|raw| parse_color(command_id, "border-color", raw)).transpose()?.unwrap_or(style.stroke.color);
    let width = optional_non_negative(command_id, "border-width", border_width)?.unwrap_or(style.stroke.width);
    style = style.with_stroke(Stroke::new(color, width));
  }
  if let Some(value) = optional_non_negative(command_id, "corner-radius", corner_radius)? {
    style = style.with_corner_radius(value);
  }
  Ok(style)
}

fn cursor_style(
  command_id: &str,
  padding: Option<f64>,
  foreground_color: Option<&str>,
  background_color: Option<&str>,
  corner_radius: Option<f64>,
  sprite_size: Option<f64>,
) -> Result<CursorStyle, String> {
  let mut style = CursorStyle::auv();
  if let Some(value) = optional_non_negative(command_id, "padding", padding)? {
    style = style.with_label_padding(Insets::all(value));
  }
  if let Some(raw) = foreground_color {
    style = style.with_label_foreground(parse_color(command_id, "foreground-color", raw)?);
  }
  if let Some(raw) = background_color {
    style = style.with_label_background(parse_color(command_id, "background-color", raw)?);
  }
  if let Some(value) = optional_non_negative(command_id, "corner-radius", corner_radius)? {
    style = style.with_label_corner_radius(value);
  }
  if let Some(value) = optional_non_negative(command_id, "sprite-size", sprite_size)? {
    if value == 0.0 {
      return Err(format!("{command_id} requires --sprite-size greater than zero"));
    }
    style = style.with_sprite_size(value);
  }
  Ok(style)
}

fn status_style(
  command_id: &str,
  padding: Option<f64>,
  foreground_color: Option<&str>,
  background_color: Option<&str>,
  corner_radius: Option<f64>,
) -> Result<StatusStyle, String> {
  let mut style = StatusStyle::action();
  if let Some(value) = optional_non_negative(command_id, "padding", padding)? {
    style = style.with_padding(Insets::all(value));
  }
  if let Some(raw) = foreground_color {
    style = style.with_foreground(parse_color(command_id, "foreground-color", raw)?);
  }
  if let Some(raw) = background_color {
    style = style.with_background(parse_color(command_id, "background-color", raw)?);
  }
  if let Some(value) = optional_non_negative(command_id, "corner-radius", corner_radius)? {
    style = style.with_corner_radius(value);
  }
  Ok(style)
}

fn optional_non_negative(command_id: &str, name: &str, value: Option<f64>) -> Result<Option<f64>, String> {
  value
    .map(|value| {
      if value.is_finite() && value >= 0.0 {
        Ok(value)
      } else {
        Err(format!("{command_id} requires finite non-negative --{name}"))
      }
    })
    .transpose()
}

fn parse_color(command_id: &str, name: &str, raw: &str) -> Result<Color, String> {
  let hex = raw.strip_prefix('#').unwrap_or(raw);
  if hex.len() != 6 && hex.len() != 8 {
    return Err(format!("{command_id} requires --{name} as #RRGGBB or #RRGGBBAA"));
  }
  let byte = |range: std::ops::Range<usize>| {
    u8::from_str_radix(&hex[range], 16).map_err(|_| format!("{command_id} received invalid --{name} color {raw:?}"))
  };
  let red = byte(0..2)?;
  let green = byte(2..4)?;
  let blue = byte(4..6)?;
  let alpha = if hex.len() == 8 { byte(6..8)? } else { 255 };
  Ok(Color::rgba(f64::from(red) / 255.0, f64::from(green) / 255.0, f64::from(blue) / 255.0, f64::from(alpha) / 255.0))
}

#[cfg(test)]
#[path = "overlay_test.rs"]
mod tests;
