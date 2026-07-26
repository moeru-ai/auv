use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportTableRow, InvokeReportValue,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
};

#[cfg(target_os = "macos")]
use crate::artifact::emit_png;

pub fn group() -> CommandGroup {
  CommandGroup::new("display", "DISPLAY")
    .command(capture_display_invoke_command())
    .command(list_displays_invoke_command())
    .command(project_screenshot_point_invoke_command())
    .command(identify_point_invoke_command())
}

#[invoke_command(
  id = "display.capture",
  group = "display",
  description = "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
async fn capture_display(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = capture_primary_display().await?;
  Ok(
    InvokeCommandOutput::from_result(&super::display_capture_result(&result.display, &result.capture))?
      .with_report(display_capture_report(&result)),
  )
}

pub async fn capture_primary_display() -> Result<auv_driver::DisplayCapture, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
    emit_png("auv.driver.display_capture", &result.capture.image);
    Ok(result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.capture is only available on macOS through auv-driver-macos".to_string())
  }
}

#[invoke_command(
  id = "display.list",
  group = "display",
  description = "List connected displays using the normalized AUV coordinate contract.",
  args = NO_ARGS,
)]
async fn list_displays(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let displays = observe_displays().await?;
  Ok(InvokeCommandOutput::from_result(&displays)?.with_report(display_list_report(&displays.displays)))
}

pub async fn observe_displays() -> Result<auv_driver::ObservedDisplays, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.display().list().map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.list is only available on macOS through auv-driver-macos".to_string())
  }
}

#[invoke_command(
  id = "display.projectScreenshotPoint",
  group = "display",
  description = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  args = NO_ARGS,
)]
async fn project_screenshot_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-display-point): add screenshot x/y arguments and expose the
  // projection through DisplayApi before implementing this command.
  unimplemented!("display.projectScreenshotPoint")
}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  description = "Resolve a logical desktop point against the current macOS display layout.",
  args = NO_ARGS,
)]
async fn identify_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-display-point): add logical x/y arguments and expose display
  // point resolution through DisplayApi before implementing this command.
  unimplemented!("display.identifyPoint")
}

fn display_capture_report(result: &auv_driver::DisplayCapture) -> InvokeReport {
  let mut fields = vec![
    InvokeReportField::new("Display", display_label(&result.display)),
    InvokeReportField::new("Display ID", result.display.id.clone()),
    InvokeReportField::new("Display frame", result.display.frame.report_value()),
    InvokeReportField::new("Capture bounds", result.capture.bounds.report_value()),
    InvokeReportField::new("Pixel size", format!("{}x{}", result.capture.image.width(), result.capture.image.height())),
    InvokeReportField::new("Scale factor", format!("{:.3}", result.capture.scale_factor)),
  ];
  if let Some(reason) = result.capture.fallback_reason.as_deref() {
    fields.push(InvokeReportField::new("Fallback reason", reason));
  }
  InvokeReport::new(fields, Vec::new())
}

fn display_label(display: &auv_driver::Display) -> String {
  display.name.clone().unwrap_or_else(|| format!("display {}", display.id))
}

fn display_list_report(displays: &[auv_driver::Display]) -> InvokeReport {
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} display(s)", displays.len()),
    )],
    tables: vec![InvokeReportTable::new(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::new([
            display.id.clone(),
            if display.is_primary {
              "primary"
            } else {
              "secondary"
            }
            .to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
          ])
        })
        .collect(),
    )],
    wide_tables: vec![InvokeReportTable::new(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE", "KIND"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::new([
            display.id.clone(),
            if display.is_primary {
              "primary"
            } else {
              "secondary"
            }
            .to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
            match display.is_builtin {
              Some(true) => "built-in",
              Some(false) => "external",
              None => "unknown",
            }
            .to_string(),
          ])
        })
        .collect(),
    )],
    sections: Vec::new(),
  }
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
