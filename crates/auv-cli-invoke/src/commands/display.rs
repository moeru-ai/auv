use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportValue, invoke_command,
};
use auv_cli_common::{TableRow, outputs::formats::table::TableOptions};
use clap::Args;

use auv_tracing::ArtifactMetadata;

use crate::artifact::emit_png_with_receipt;
#[cfg(target_os = "macos")]
use auv_driver::overlay::{Overlay, components::CaptureFrame};
#[cfg(target_os = "macos")]
use std::time::Duration;

pub fn group() -> CommandGroup {
  // TODO(invoke-display-stubs): point commands stay intentionally unregistered
  // until an owner-approved implementation has behavioral evidence.
  CommandGroup::new("display", "DISPLAY").command(capture_display_invoke_command()).command(list_displays_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke display.capture")]
struct CaptureDisplayArgs {}

#[invoke_command(
  id = "display.capture",
  group = "display",
  description = "Capture the primary display with its screenshot-to-logical coordinate contract.",
  input = CaptureDisplayArgs,
)]
async fn capture_display(input: InvokeCommandInput, _args: CaptureDisplayArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let (result, artifact) = capture_primary_display_recorded_with_session(&session).await?;
    let capture_overlay = Overlay::new().with_layer(
      CaptureFrame::new(result.display.frame)
        .with_label(result.display.name.clone().unwrap_or_else(|| format!("display {}", result.display.id))),
    );
    let overlay = super::overlay::show_overlay(
      &input,
      &session,
      capture_overlay,
      auv_driver::overlay::ShowOptions::new()
        .with_motion_ease(Duration::from_millis(120), auv_driver::overlay::Easing::EaseInOutExpo)
        .with_auto_removal_after(Duration::from_millis(180)),
    )?;
    let mut output = display_capture_output(&result, artifact)?;
    output.report.as_mut().expect("display capture output always has a report").fields.push(overlay.report_field());
    Ok(output)
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.capture is only available on macOS through auv-driver-macos".to_string())
  }
}

/// Records and projects a capture returned by either a local or remote Driver.
pub async fn recorded_display_capture_output(result: &auv_driver::DisplayCapture) -> InvokeCommandResult {
  let artifact = emit_png_with_receipt("auv.driver.display_capture", &result.capture.image).await;
  display_capture_output(result, artifact)
}

fn display_capture_output(result: &auv_driver::DisplayCapture, artifact: Option<ArtifactMetadata>) -> InvokeCommandResult {
  Ok(
    InvokeCommandOutput::from_result(&super::display_capture_result(&result.display, &result.capture))?
      .with_report(display_capture_report(result))
      .with_artifacts(artifact),
  )
}

pub async fn capture_primary_display() -> Result<auv_driver::DisplayCapture, String> {
  capture_primary_display_recorded().await.map(|(capture, _)| capture)
}

async fn capture_primary_display_recorded() -> Result<(auv_driver::DisplayCapture, Option<ArtifactMetadata>), String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    capture_primary_display_recorded_with_session(&session).await
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.capture is only available on macOS through auv-driver-macos".to_string())
  }
}

#[cfg(target_os = "macos")]
async fn capture_primary_display_recorded_with_session(
  session: &auv_driver::LocalDriverSession,
) -> Result<(auv_driver::DisplayCapture, Option<ArtifactMetadata>), String> {
  let result = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
  let artifact = emit_png_with_receipt("auv.driver.display_capture", &result.capture.image).await;
  Ok((result, artifact))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke display.list --json")]
struct ListDisplaysArgs {}

#[invoke_command(
  id = "display.list",
  group = "display",
  description = "List connected displays using the normalized AUV coordinate contract.",
  input = ListDisplaysArgs,
)]
async fn list_displays(input: InvokeCommandInput, _args: ListDisplaysArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let displays = observe_displays().await?;
  list_displays_output(&displays)
}

/// Builds the transport-independent direct result for `display.list`.
///
/// Local and daemon-backed frontends use this same projection so selecting a
/// Device changes placement without creating a second command result schema.
pub fn list_displays_output(displays: &auv_driver::ObservedDisplays) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(displays)?.with_report(display_list_report(&displays.displays)))
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

fn display_capture_report(result: &auv_driver::DisplayCapture) -> InvokeReport {
  let mut fields = vec![
    InvokeReportField::new("Display", result.display.name.clone().unwrap_or_else(|| format!("display {}", result.display.id))),
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

#[derive(TableRow)]
struct DisplayRow {
  #[table(header = "REF")]
  reference: String,
  role: &'static str,
  name: String,
  frame: String,
  #[table(display_with = |scale: &f64| format!("{scale:.3}"))]
  scale: f64,
  #[table(wide)]
  kind: &'static str,
}

fn display_list_report(displays: &[auv_driver::Display]) -> InvokeReport {
  let rows = displays
    .iter()
    .map(|display| DisplayRow {
      reference: display.id.clone(),
      role: if display.is_primary {
        "primary"
      } else {
        "secondary"
      },
      name: display.name.clone().unwrap_or_else(|| format!("display {}", display.id)),
      frame: display.frame.report_value(),
      scale: display.scale_factor,
      kind: match display.is_builtin {
        Some(true) => "built-in",
        Some(false) => "external",
        None => "unknown",
      },
    })
    .collect::<Vec<_>>();
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} display(s)", displays.len()),
    )],
    tables: vec![InvokeReportTable::from_rows(&rows, TableOptions::default())],
    wide_tables: vec![InvokeReportTable::from_rows(
      &rows,
      TableOptions::default().wide(true),
    )],
    sections: Vec::new(),
  }
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
